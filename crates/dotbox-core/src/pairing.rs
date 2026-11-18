//! 모듈 역할 레코드.
//!
//! 두 모듈은 같은 펌웨어 이미지를 쓰고, 역할은 플래시의 `dotbox` 파티션에 저장한 레코드로 정한다
//! (`docs/pairing.md` 2.1). 펌웨어와 호스트 도구(`dotbox flash`)가 이 형식을 함께 쓴다.

use crate::display::Side;
use crate::{Error, Result};

/// 역할 레코드의 바이트 길이.
pub const ROLE_RECORD_LEN: usize = 16;

const MAGIC: [u8; 4] = *b"DBXR";
const FORMAT_VERSION: u8 = 1;
const CRC_OFFSET: usize = 12;

/// 플래시에 저장하는 모듈 역할.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleRecord {
    /// 이 모듈의 역할. `Left`가 마스터다.
    pub side: Side,
    /// 짝을 맺은 상대 모듈의 MAC. 모르면 모두 0이다.
    pub peer_mac: [u8; 6],
}

impl RoleRecord {
    /// 상대 MAC을 모르는 레코드를 만든다.
    pub const fn new(side: Side) -> Self {
        Self {
            side,
            peer_mac: [0; 6],
        }
    }

    /// 상대 MAC을 알고 있는지.
    pub fn has_peer(&self) -> bool {
        self.peer_mac != [0; 6]
    }

    /// 저장 형식으로 바꾼다. 리틀 엔디언이다.
    pub fn to_bytes(&self) -> [u8; ROLE_RECORD_LEN] {
        let mut bytes = [0; ROLE_RECORD_LEN];
        bytes[0..4].copy_from_slice(&MAGIC);
        bytes[4] = FORMAT_VERSION;
        bytes[5] = match self.side {
            Side::Left => 0,
            Side::Right => 1,
        };
        bytes[6..12].copy_from_slice(&self.peer_mac);
        let crc = crc32(&bytes[..CRC_OFFSET]);
        bytes[CRC_OFFSET..].copy_from_slice(&crc.to_le_bytes());
        bytes
    }

    /// 저장 형식에서 읽는다.
    ///
    /// 길이, 매직, 형식 버전, 역할 값, CRC 중 하나라도 맞지 않으면 [`Error::InvalidArgument`]를 돌려준다.
    /// 지워진 플래시(모두 0xFF)도 여기에 해당한다. 이때 펌웨어는 역할 선택 화면을 띄운다.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let bytes: &[u8; ROLE_RECORD_LEN] = bytes.try_into().map_err(|_| Error::InvalidArgument)?;
        if bytes[0..4] != MAGIC || bytes[4] != FORMAT_VERSION {
            return Err(Error::InvalidArgument);
        }
        let stored_crc = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        if stored_crc != crc32(&bytes[..CRC_OFFSET]) {
            return Err(Error::InvalidArgument);
        }
        let side = match bytes[5] {
            0 => Side::Left,
            1 => Side::Right,
            _ => return Err(Error::InvalidArgument),
        };
        let mut peer_mac = [0; 6];
        peer_mac.copy_from_slice(&bytes[6..12]);
        Ok(Self { side, peer_mac })
    }
}

/// CRC-32 (IEEE 802.3, 반사형, 다항식 0xEDB88320). 레코드가 작아서 표 없이 계산한다.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn round_trips_both_sides() {
        for side in [Side::Left, Side::Right] {
            let record = RoleRecord {
                side,
                peer_mac: [0x24, 0x6F, 0x28, 0x01, 0x02, 0x03],
            };
            assert_eq!(RoleRecord::from_bytes(&record.to_bytes()), Ok(record));
        }
    }

    #[test]
    fn layout_is_fixed() {
        let bytes = RoleRecord::new(Side::Right).to_bytes();
        assert_eq!(&bytes[0..4], b"DBXR");
        assert_eq!(bytes[4], 1);
        assert_eq!(bytes[5], 1);
        assert_eq!(&bytes[6..12], &[0; 6]);
        assert_eq!(
            u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            crc32(&bytes[..12])
        );
    }

    #[test]
    fn new_record_has_no_peer() {
        let record = RoleRecord::new(Side::Left);
        assert!(!record.has_peer());
        let paired = RoleRecord {
            peer_mac: [1, 0, 0, 0, 0, 0],
            ..record
        };
        assert!(paired.has_peer());
    }

    #[test]
    fn rejects_erased_flash() {
        assert_eq!(
            RoleRecord::from_bytes(&[0xFF; ROLE_RECORD_LEN]),
            Err(Error::InvalidArgument)
        );
    }

    #[test]
    fn rejects_wrong_length() {
        let bytes = RoleRecord::new(Side::Left).to_bytes();
        assert_eq!(
            RoleRecord::from_bytes(&bytes[..15]),
            Err(Error::InvalidArgument)
        );
        assert_eq!(RoleRecord::from_bytes(&[]), Err(Error::InvalidArgument));
    }

    #[test]
    fn rejects_corruption() {
        let good = RoleRecord::new(Side::Left).to_bytes();
        for index in 0..ROLE_RECORD_LEN {
            let mut bad = good;
            bad[index] ^= 0x01;
            assert_eq!(
                RoleRecord::from_bytes(&bad),
                Err(Error::InvalidArgument),
                "byte {index}"
            );
        }
    }

    #[test]
    fn rejects_unknown_role_even_with_valid_crc() {
        let mut bytes = RoleRecord::new(Side::Left).to_bytes();
        bytes[5] = 2;
        let crc = crc32(&bytes[..12]);
        bytes[12..].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(RoleRecord::from_bytes(&bytes), Err(Error::InvalidArgument));
    }
}
