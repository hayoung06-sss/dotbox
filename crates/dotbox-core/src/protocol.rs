//! 모듈 간 메시지 형식(`docs/game-api.md` 7장).

use crate::display::ScreenLayout;
use crate::input::ControlState;
use crate::state::{MAX_STATE_BYTES, StateReader, StateWriter};
use crate::{Error, Result};

/// 메시지 형식 버전. 형식이 바뀌면 올린다.
pub const PROTOCOL_VERSION: u16 = 1;
/// 공통 헤더 크기.
pub const HEADER_LEN: usize = 6;
/// 메시지 최대 크기(헤더 + State 머리 2바이트 + 상태).
pub const MAX_MESSAGE_LEN: usize = HEADER_LEN + 2 + MAX_STATE_BYTES;

const MAGIC: [u8; 2] = *b"DB";

/// ESP-NOW 한 패킷의 최대 데이터 크기.
pub const ESP_NOW_MAX_PAYLOAD: usize = 250;
const _: () = assert!(
    MAX_MESSAGE_LEN <= ESP_NOW_MAX_PAYLOAD,
    "메시지가 ESP-NOW 패킷보다 큽니다"
);

/// 빌드 식별자(펌웨어 이미지 해시 앞 8바이트, `docs/pairing.md` 3.1).
pub type BuildId = [u8; 8];

/// 메시지 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Hello = 0x01,
    Accept = 0x02,
    Reject = 0x03,
    Input = 0x10,
    State = 0x11,
}

impl MessageKind {
    fn from_u8(value: u8) -> Result<Self> {
        Ok(match value {
            0x01 => Self::Hello,
            0x02 => Self::Accept,
            0x03 => Self::Reject,
            0x10 => Self::Input,
            0x11 => Self::State,
            _ => return Err(Error::InvalidArgument),
        })
    }
}

/// 공통 헤더.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub kind: MessageKind,
    /// 세션 ID. 연결 전 메시지는 0.
    pub session: u8,
    pub seq: u16,
}

/// 핸드셰이크 거부 사유.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// 두 모듈이 같은 역할이다.
    RoleConflict = 1,
    /// 메시지 형식 버전이 다르다.
    ProtocolMismatch = 2,
    /// 펌웨어 이미지가 다르다.
    BuildMismatch = 3,
}

impl RejectReason {
    fn from_u8(value: u8) -> Result<Self> {
        Ok(match value {
            1 => Self::RoleConflict,
            2 => Self::ProtocolMismatch,
            3 => Self::BuildMismatch,
            _ => return Err(Error::InvalidArgument),
        })
    }
}

/// 받은 메시지. `State`의 상태 바이트는 받은 버퍼를 빌린다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message<'a> {
    Hello {
        protocol_version: u16,
        /// 보낸 모듈의 역할(0 왼쪽, 1 오른쪽).
        role: u8,
        build_id: BuildId,
    },
    Accept,
    Reject(RejectReason),
    Input(ControlState),
    State {
        game_index: u8,
        layout: ScreenLayout,
        state: &'a [u8],
    },
}

impl Message<'_> {
    pub fn kind(&self) -> MessageKind {
        match self {
            Self::Hello { .. } => MessageKind::Hello,
            Self::Accept => MessageKind::Accept,
            Self::Reject(_) => MessageKind::Reject,
            Self::Input(_) => MessageKind::Input,
            Self::State { .. } => MessageKind::State,
        }
    }
}

pub(crate) fn layout_to_u8(layout: ScreenLayout) -> u8 {
    match layout {
        ScreenLayout::Wide => 0,
        ScreenLayout::Mirrored => 1,
    }
}

fn layout_from_u8(value: u8) -> Result<ScreenLayout> {
    match value {
        0 => Ok(ScreenLayout::Wide),
        1 => Ok(ScreenLayout::Mirrored),
        _ => Err(Error::InvalidArgument),
    }
}

/// 메시지를 `out`에 쓰고 길이를 돌려준다.
pub fn encode(session: u8, seq: u16, message: &Message<'_>, out: &mut [u8]) -> Result<usize> {
    let mut w = StateWriter::new(out);
    w.bytes(&MAGIC)?;
    w.u8(message.kind() as u8)?;
    w.u8(session)?;
    w.u16(seq)?;
    match message {
        Message::Hello {
            protocol_version,
            role,
            build_id,
        } => {
            w.u16(*protocol_version)?;
            w.u8(*role)?;
            w.bytes(build_id)?;
        }
        Message::Accept => {}
        Message::Reject(reason) => w.u8(*reason as u8)?,
        Message::Input(control) => control.encode(&mut w)?,
        Message::State {
            game_index,
            layout,
            state,
        } => {
            if state.len() > MAX_STATE_BYTES {
                return Err(Error::InvalidArgument);
            }
            w.u8(*game_index)?;
            w.u8(layout_to_u8(*layout))?;
            w.bytes(state)?;
        }
    }
    Ok(w.len())
}

/// 메시지를 읽는다. 매직, 종류, 길이가 맞지 않으면 [`Error::InvalidArgument`].
pub fn decode(bytes: &[u8]) -> Result<(Header, Message<'_>)> {
    let mut r = StateReader::new(bytes);
    if r.array::<2>()? != MAGIC {
        return Err(Error::InvalidArgument);
    }
    let kind = MessageKind::from_u8(r.u8()?)?;
    let session = r.u8()?;
    let seq = r.u16()?;
    let message = match kind {
        MessageKind::Hello => Message::Hello {
            protocol_version: r.u16()?,
            role: r.u8()?,
            build_id: r.array()?,
        },
        MessageKind::Accept => Message::Accept,
        MessageKind::Reject => Message::Reject(RejectReason::from_u8(r.u8()?)?),
        MessageKind::Input => Message::Input(ControlState::decode(&mut r)?),
        MessageKind::State => {
            let game_index = r.u8()?;
            let layout = layout_from_u8(r.u8()?)?;
            let len = r.remaining();
            if len > MAX_STATE_BYTES {
                return Err(Error::InvalidArgument);
            }
            Message::State {
                game_index,
                layout,
                state: r.bytes(len)?,
            }
        }
    };
    if kind != MessageKind::State && r.remaining() != 0 {
        return Err(Error::InvalidArgument);
    }
    Ok((Header { kind, session, seq }, message))
}

/// 순번이 `last`보다 새것인지(순환 비교).
pub fn is_newer(seq: u16, last: u16) -> bool {
    (seq.wrapping_sub(last) as i16) > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Direction;

    fn round_trip(message: Message<'_>) {
        let mut buf = [0; MAX_MESSAGE_LEN];
        let len = encode(7, 0xABCD, &message, &mut buf).unwrap();
        let (header, decoded) = decode(&buf[..len]).unwrap();
        assert_eq!(header.kind, message.kind());
        assert_eq!(header.session, 7);
        assert_eq!(header.seq, 0xABCD);
        assert_eq!(decoded, message);
    }

    #[test]
    fn all_messages_round_trip() {
        round_trip(Message::Hello {
            protocol_version: PROTOCOL_VERSION,
            role: 1,
            build_id: [1, 2, 3, 4, 5, 6, 7, 8],
        });
        round_trip(Message::Accept);
        round_trip(Message::Reject(RejectReason::BuildMismatch));
        round_trip(Message::Input(ControlState {
            direction: Some(Direction::Down),
            axis_x: -3,
            axis_y: 100,
            button_held: true,
            press_count: 9,
            release_count: 8,
        }));
        round_trip(Message::State {
            game_index: 2,
            layout: ScreenLayout::Mirrored,
            state: &[9; MAX_STATE_BYTES],
        });
        round_trip(Message::State {
            game_index: 0,
            layout: ScreenLayout::Wide,
            state: &[],
        });
    }

    #[test]
    fn header_layout_is_fixed() {
        let mut buf = [0; MAX_MESSAGE_LEN];
        let len = encode(3, 0x0102, &Message::Accept, &mut buf).unwrap();
        assert_eq!(&buf[..len], &[b'D', b'B', 0x02, 3, 0x02, 0x01]);
        assert_eq!(len, HEADER_LEN);
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(&[]).is_err());
        assert!(decode(b"XX\x02\x00\x00\x00").is_err());
        assert!(decode(b"DB\x7f\x00\x00\x00").is_err());
        // 길이가 남거나 모자라면 거부한다.
        assert!(decode(b"DB\x02\x00\x00\x00\x00").is_err());
        assert!(decode(b"DB\x03\x00\x00\x00").is_err());
        assert!(decode(b"DB\x03\x00\x00\x00\x09").is_err());
        assert!(decode(b"DB\x11\x00\x00\x00\x00\x05").is_err());
    }

    #[test]
    fn rejects_oversized_state() {
        let mut buf = [0; MAX_MESSAGE_LEN + 10];
        let big = [0; MAX_STATE_BYTES + 1];
        let message = Message::State {
            game_index: 0,
            layout: ScreenLayout::Wide,
            state: &big,
        };
        assert!(encode(1, 1, &message, &mut buf).is_err());
    }

    #[test]
    fn sequence_comparison_wraps() {
        assert!(is_newer(1, 0));
        assert!(!is_newer(0, 0));
        assert!(!is_newer(0, 1));
        assert!(is_newer(0, u16::MAX));
        assert!(is_newer(100, 65000));
    }
}
