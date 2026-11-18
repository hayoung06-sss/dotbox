//! 테스트와 시뮬레이터용 도구(`docs/game-api.md` 9장).
//!
//! - [`Harness`]: 화면 없이 게임 하나를 돌린다. 매 틱 상태 직렬화 왕복을 확인한다.
//! - [`loopback_pair`]: 메모리 안의 `Link` 한 쌍. 두 엔진을 이어 동기화 경로를 시험한다.
//! - [`FakeDisplay`], [`FakeInput`]: 가짜 장치.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::display::{FrameBuffer, PANEL_PIXELS};
use crate::game::{Context, Game};
use crate::hal::{Display, InputSource, Link, RawInput, Rgb};
use crate::input::Input;
use crate::state::{GameState, MAX_STATE_BYTES, StateReader, StateWriter};
use crate::{Error, Result};

/// 화면 없이 게임 하나를 돌리는 도구. 마스터 쪽 경로(`init`, `update`)와 `render`를 그대로 부른다.
pub struct Harness<G: Game> {
    game: G,
    state: G::State,
    ctx: Context,
    exited: bool,
}

impl<G: Game> Harness<G> {
    /// 게임을 시작한다(`init`). `seed`는 난수 시드다.
    pub fn new(mut game: G, seed: u64) -> Self {
        let mut ctx = Context::new(seed, game.config().layout);
        let state = game.init(&mut ctx);
        let harness = Self {
            game,
            state,
            ctx,
            exited: false,
        };
        harness.check_round_trip();
        harness
    }

    /// 한 틱을 진행한다. 상태 직렬화 왕복이 맞지 않으면 패닉한다.
    pub fn step(&mut self, input: &Input) {
        self.game.update(&mut self.state, input, &mut self.ctx);
        self.ctx.advance_tick();
        self.exited |= self.ctx.take_exit();
        let _ = self.ctx.take_sound();
        self.check_round_trip();
    }

    /// 같은 입력으로 `ticks`틱을 진행한다.
    pub fn run(&mut self, ticks: u32, input: &Input) {
        for _ in 0..ticks {
            self.step(input);
        }
    }

    pub fn state(&self) -> &G::State {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut G::State {
        &mut self.state
    }

    pub fn context(&mut self) -> &mut Context {
        &mut self.ctx
    }

    /// 지금까지 `exit()`를 불렀는지.
    pub fn exited(&self) -> bool {
        self.exited
    }

    /// 현재 상태를 그린 프레임. 레이아웃은 게임이 정한 것을 따른다.
    pub fn frame(&self) -> FrameBuffer {
        let mut frame = FrameBuffer::new(self.ctx.layout());
        G::render(&self.state, &mut frame);
        frame
    }

    fn check_round_trip(&self) {
        let first = encode(&self.state);
        let mut reader = StateReader::new(&first.0[..first.1]);
        let decoded = G::State::decode(&mut reader).expect("State::decode 실패");
        assert_eq!(reader.remaining(), 0, "State::decode가 바이트를 남겼습니다");
        let second = encode(&decoded);
        assert_eq!(
            first.0[..first.1],
            second.0[..second.1],
            "State 인코딩 → 디코딩 → 인코딩 결과가 다릅니다"
        );
        assert!(
            first.1 <= G::State::MAX_BYTES,
            "State 인코딩 크기({})가 MAX_BYTES({})를 넘습니다",
            first.1,
            G::State::MAX_BYTES
        );
    }
}

fn encode<S: GameState>(state: &S) -> ([u8; MAX_STATE_BYTES], usize) {
    let mut buf = [0; MAX_STATE_BYTES];
    let mut writer = StateWriter::new(&mut buf);
    state.encode(&mut writer).expect("State::encode 실패");
    let len = writer.len();
    (buf, len)
}

/// 마지막 출력 프레임을 기억하는 가짜 디스플레이(16×16 패널).
#[derive(Debug, Clone)]
pub struct FakeDisplay {
    frame: [Rgb; PANEL_PIXELS],
    pub brightness_percent: Option<u8>,
    pub write_count: usize,
}

impl Default for FakeDisplay {
    fn default() -> Self {
        Self {
            frame: [Rgb::BLACK; PANEL_PIXELS],
            brightness_percent: None,
            write_count: 0,
        }
    }
}

impl FakeDisplay {
    pub fn last_frame(&self) -> &[Rgb; PANEL_PIXELS] {
        &self.frame
    }
}

impl Display for FakeDisplay {
    fn width(&self) -> u16 {
        16
    }

    fn height(&self) -> u16 {
        16
    }

    fn write_frame(&mut self, pixels: &[Rgb]) -> Result<()> {
        if pixels.len() != PANEL_PIXELS {
            return Err(Error::InvalidArgument);
        }
        self.frame.copy_from_slice(pixels);
        self.write_count += 1;
        Ok(())
    }

    fn set_brightness_percent(&mut self, percent: u8) -> Result<()> {
        if percent > 100 {
            return Err(Error::InvalidArgument);
        }
        self.brightness_percent = Some(percent);
        Ok(())
    }
}

/// 값을 직접 정하는 가짜 조이스틱. 기본값은 중앙, 버튼 뗌.
#[derive(Debug, Clone, Copy)]
pub struct FakeInput {
    pub raw: RawInput,
}

impl Default for FakeInput {
    fn default() -> Self {
        Self {
            raw: RawInput {
                x: 2048,
                y: 2048,
                button_down: false,
            },
        }
    }
}

impl InputSource for FakeInput {
    fn read(&mut self) -> Result<RawInput> {
        Ok(self.raw)
    }
}

type Queue = Arc<Mutex<VecDeque<Vec<u8>>>>;

/// 메모리 안의 `Link`. [`loopback_pair`]로 만든다. 테스트와 시뮬레이터 전용이다(송신마다 할당한다).
#[derive(Debug, Clone)]
pub struct LoopbackLink {
    outbox: Queue,
    inbox: Queue,
    connected: Arc<AtomicBool>,
}

/// 두 링크의 연결을 끊고 잇는 스위치.
#[derive(Debug, Clone)]
pub struct LinkSwitch {
    connected: Arc<AtomicBool>,
}

impl LinkSwitch {
    /// 끊으면 보내는 메시지가 사라진다.
    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

/// 서로 이어진 링크 한 쌍과 스위치.
pub fn loopback_pair() -> (LoopbackLink, LoopbackLink, LinkSwitch) {
    let a_to_b: Queue = Arc::default();
    let b_to_a: Queue = Arc::default();
    let connected = Arc::new(AtomicBool::new(true));
    (
        LoopbackLink {
            outbox: a_to_b.clone(),
            inbox: b_to_a.clone(),
            connected: connected.clone(),
        },
        LoopbackLink {
            outbox: b_to_a,
            inbox: a_to_b,
            connected: connected.clone(),
        },
        LinkSwitch { connected },
    )
}

impl Link for LoopbackLink {
    fn send(&mut self, data: &[u8]) -> Result<()> {
        if self.connected.load(Ordering::Relaxed) {
            self.outbox
                .lock()
                .map_err(|_| Error::Io)?
                .push_back(data.to_vec());
        }
        Ok(())
    }

    fn try_recv(&mut self, buf: &mut [u8]) -> Result<Option<usize>> {
        let Some(message) = self.inbox.lock().map_err(|_| Error::Io)?.pop_front() else {
            return Ok(None);
        };
        let target = buf.get_mut(..message.len()).ok_or(Error::InvalidArgument)?;
        target.copy_from_slice(&message);
        Ok(Some(message.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::tests::{DotGame, DotState};
    use crate::input::{ButtonState, Direction, PlayerInput};

    fn p1(input: PlayerInput) -> Input {
        Input {
            players: [input, PlayerInput::default()],
            ..Input::default()
        }
    }

    #[test]
    fn harness_runs_game() {
        let mut h = Harness::new(DotGame, 1);
        h.step(&p1(PlayerInput {
            direction: Some(Direction::Right),
            direction_pressed: Some(Direction::Right),
            ..PlayerInput::default()
        }));
        assert_eq!(
            *h.state(),
            DotState {
                x: 1,
                y: 0,
                red: false
            }
        );
        h.step(&p1(PlayerInput {
            button: ButtonState {
                held: true,
                pressed: true,
                released: false,
            },
            ..PlayerInput::default()
        }));
        assert!(h.state().red);
        assert_eq!(h.frame().pixel(1, 0), Some(Rgb::new(255, 0, 0)));
        assert_eq!(h.context().tick(), 2);
        assert!(!h.exited());
    }

    #[test]
    fn loopback_delivers_in_order_and_respects_switch() {
        let (mut a, mut b, switch) = loopback_pair();
        a.send(b"one").unwrap();
        a.send(b"two").unwrap();
        let mut buf = [0; 8];
        assert_eq!(b.try_recv(&mut buf).unwrap(), Some(3));
        assert_eq!(&buf[..3], b"one");
        assert_eq!(b.try_recv(&mut buf).unwrap(), Some(3));
        assert_eq!(&buf[..3], b"two");
        assert_eq!(b.try_recv(&mut buf).unwrap(), None);

        switch.set_connected(false);
        b.send(b"lost").unwrap();
        assert_eq!(a.try_recv(&mut buf).unwrap(), None);
        assert!(!switch.is_connected());
    }

    #[test]
    fn loopback_rejects_small_buffer() {
        let (mut a, mut b, _switch) = loopback_pair();
        a.send(b"too long").unwrap();
        let mut buf = [0; 2];
        assert_eq!(b.try_recv(&mut buf), Err(Error::InvalidArgument));
    }

    #[test]
    fn fake_display_validates_frame() {
        let mut d = FakeDisplay::default();
        assert!(d.write_frame(&[Rgb::WHITE; 3]).is_err());
        d.write_frame(&[Rgb::WHITE; PANEL_PIXELS]).unwrap();
        assert_eq!(d.write_count, 1);
        assert!(d.set_brightness_percent(101).is_err());
    }
}
