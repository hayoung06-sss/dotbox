//! 엔진 루프(`docs/game-api.md` 6~7장). 플랫폼 실행기는 [`Engine::step`]을 반복해서 부른다.

use crate::display::{FrameBuffer, PANEL_PIXELS, ScreenLayout, Side};
use crate::game::{Context, Games};
use crate::hal::{Buzzer, Display, InputSource, Link, Rgb};
use crate::input::{ControlState, EventTracker, Input, InputConfig, InputProcessor};
use crate::protocol::{
    self, BuildId, MAX_MESSAGE_LEN, Message, PROTOCOL_VERSION, RejectReason, is_newer,
};
use crate::sound::SoundPlayer;
use crate::state::{MAX_STATE_BYTES, StateWriter};
use crate::tick::FixedTicker;
use crate::{Error, Result};

/// 슬레이브가 연결 전 `Hello`를 보내는 간격.
pub const HELLO_INTERVAL_MS: u32 = 200;
/// 거부된 뒤 `Hello`를 다시 보내는 간격.
pub const REJECTED_RETRY_MS: u32 = 1000;
/// 이 시간 동안 원격 입력이 없으면 P2를 중립으로 바꾼다.
pub const INPUT_STALE_MS: u32 = 250;
/// 이 시간 동안 상대 메시지가 없으면 연결이 끊긴 것으로 본다.
pub const LINK_TIMEOUT_MS: u32 = 500;
/// 한 번의 `step`에서 처리하는 최대 수신 메시지 수.
pub const MAX_MESSAGES_PER_STEP: usize = 8;
/// 기본 밝기(%). 전원 때문에 15~25%로 제한한다.
pub const DEFAULT_BRIGHTNESS_PERCENT: u8 = 20;

/// 모듈 간 연결 상태(`docs/pairing.md` 4장).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// 상대를 찾는 중.
    Searching,
    /// 연결됨. 게임이 진행된다.
    Connected,
    /// 핸드셰이크가 거부됐다.
    Rejected(RejectReason),
}

/// 엔진 설정.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    /// 이 모듈의 역할. `Left`가 마스터다.
    pub role: Side,
    /// 펌웨어 빌드 식별자. 두 모듈이 같아야 연결된다.
    pub build_id: BuildId,
    /// 게임 난수 시드. 플랫폼이 하드웨어 난수나 시각으로 정한다.
    pub seed: u64,
    pub input: InputConfig,
    pub brightness_percent: u8,
}

impl EngineConfig {
    pub fn new(role: Side, build_id: BuildId) -> Self {
        Self {
            role,
            build_id,
            seed: 0,
            input: InputConfig::default(),
            brightness_percent: DEFAULT_BRIGHTNESS_PERCENT,
        }
    }
}

/// 엔진이 한 번의 `step`에서 쓰는 장치.
pub struct Io<'a> {
    pub display: &'a mut dyn Display,
    pub input: &'a mut dyn InputSource,
    pub link: &'a mut dyn Link,
    /// 부저가 없는 모듈은 `None`.
    pub buzzer: Option<&'a mut dyn Buzzer>,
}

/// 한 모듈의 엔진.
pub struct Engine {
    config: EngineConfig,
    games: Games,
    active: usize,
    ticker: FixedTicker,
    input: InputProcessor,
    trackers: [EventTracker; 2],
    ctx: Context,
    layout: ScreenLayout,
    link_state: LinkState,
    session: u8,
    next_session: u8,
    send_seq: u16,
    recv_seq: Option<u16>,
    /// 연결된 상대에게서 마지막으로 유효한 메시지를 받은 시각.
    peer_last_ms: u32,
    /// 마스터가 받은 원격(P2) 입력.
    remote: ControlState,
    remote_last_ms: u32,
    last_hello_ms: Option<u32>,
    frame: FrameBuffer,
    panel: [Rgb; PANEL_PIXELS],
    dirty: bool,
    started: bool,
    sound: SoundPlayer,
}

impl Engine {
    /// 엔진을 만든다. 게임이 하나도 없으면 [`Error::InvalidArgument`].
    pub fn new(games: impl Into<Games>, config: EngineConfig, now_ms: u32) -> Result<Self> {
        let games = games.into();
        let first = games.configs().next().ok_or(Error::InvalidArgument)?;
        Ok(Self {
            config,
            games,
            active: 0,
            ticker: FixedTicker::new(now_ms),
            input: InputProcessor::new(config.input),
            trackers: Default::default(),
            ctx: Context::new(config.seed, first.layout),
            layout: first.layout,
            link_state: LinkState::Searching,
            session: 0,
            next_session: 1,
            send_seq: 0,
            recv_seq: None,
            peer_last_ms: now_ms,
            remote: ControlState::default(),
            remote_last_ms: now_ms,
            last_hello_ms: None,
            frame: FrameBuffer::new(first.layout),
            panel: [Rgb::BLACK; PANEL_PIXELS],
            dirty: true,
            started: false,
            sound: SoundPlayer::default(),
        })
    }

    pub fn role(&self) -> Side {
        self.config.role
    }

    pub fn link_state(&self) -> LinkState {
        self.link_state
    }

    /// 실행 중인 게임 번호.
    pub fn active_game(&self) -> usize {
        self.active
    }

    pub fn games(&self) -> &Games {
        &self.games
    }

    /// 마지막으로 그린 전체 프레임(32×16). 시뮬레이터와 테스트용.
    pub fn frame(&self) -> &FrameBuffer {
        &self.frame
    }

    fn is_master(&self) -> bool {
        self.config.role == Side::Left
    }

    /// 루프 한 번을 실행한다.
    pub fn step(&mut self, now_ms: u32, io: &mut Io<'_>) -> Result<()> {
        if !self.started {
            io.display
                .set_brightness_percent(self.config.brightness_percent)?;
            self.started = true;
        }

        let mut rx = [0u8; MAX_MESSAGE_LEN];
        for _ in 0..MAX_MESSAGES_PER_STEP {
            match io.link.try_recv(&mut rx)? {
                Some(len) => self.handle_message(&rx[..len.min(rx.len())], now_ms, io.link),
                None => break,
            }
        }

        let raw = io.input.read()?;
        self.input.sample(raw, now_ms);
        let ticks = self.ticker.advance(now_ms);

        if self.is_master() {
            self.master_step(ticks, now_ms, io.link);
        } else {
            self.slave_step(ticks, now_ms, io.link);
        }

        if self.dirty {
            self.render(io.display)?;
        }
        if let Some(buzzer) = io.buzzer.as_deref_mut() {
            self.sound.update(now_ms, buzzer)?;
        }
        Ok(())
    }

    fn master_step(&mut self, ticks: u32, now_ms: u32, link: &mut dyn Link) {
        if self.link_state != LinkState::Connected {
            return;
        }
        if now_ms.wrapping_sub(self.remote_last_ms) > LINK_TIMEOUT_MS {
            self.disconnect();
            return;
        }
        for _ in 0..ticks {
            let local = self.input.state();
            let stale = now_ms.wrapping_sub(self.remote_last_ms) > INPUT_STALE_MS;
            let remote = if stale {
                // 중립. 누적 횟수는 유지해 다시 이어질 때 가짜 이벤트가 생기지 않게 한다.
                ControlState {
                    press_count: self.remote.press_count,
                    release_count: self.remote.release_count,
                    ..ControlState::default()
                }
            } else {
                self.remote
            };
            let input = Input {
                players: [
                    self.trackers[0].next(local, false),
                    self.trackers[1].next(remote, stale),
                ],
                ..Input::default()
            };
            if let Some(game) = self.games.get_mut(self.active) {
                game.update(&input, &mut self.ctx);
            }
            self.ctx.advance_tick();
            if let Some(sound) = self.ctx.take_sound() {
                self.sound.play(sound, now_ms);
            }
            if self.ctx.take_exit() {
                // 메뉴가 생기기 전까지는 첫 게임을 처음부터 다시 시작한다.
                self.active = 0;
                self.start_game(now_ms);
            }
        }
        if ticks > 0 {
            self.layout = self.ctx.layout();
            self.send_state(link);
            self.dirty = true;
        }
    }

    fn slave_step(&mut self, ticks: u32, now_ms: u32, link: &mut dyn Link) {
        match self.link_state {
            LinkState::Connected => {
                if now_ms.wrapping_sub(self.peer_last_ms) > LINK_TIMEOUT_MS {
                    self.disconnect();
                } else if ticks > 0 {
                    let message = Message::Input(self.input.state());
                    self.send(link, &message);
                }
            }
            LinkState::Searching | LinkState::Rejected(_) => {
                let interval = if self.link_state == LinkState::Searching {
                    HELLO_INTERVAL_MS
                } else {
                    REJECTED_RETRY_MS
                };
                let due = self
                    .last_hello_ms
                    .is_none_or(|last| now_ms.wrapping_sub(last) >= interval);
                if due {
                    self.last_hello_ms = Some(now_ms);
                    let message = Message::Hello {
                        protocol_version: PROTOCOL_VERSION,
                        role: 1,
                        build_id: self.config.build_id,
                    };
                    self.send_with_session(link, 0, &message);
                }
            }
        }
    }

    fn handle_message(&mut self, bytes: &[u8], now_ms: u32, link: &mut dyn Link) {
        // 형식이 틀린 메시지는 버린다(다른 기기의 패킷이나 손상).
        let Ok((header, message)) = protocol::decode(bytes) else {
            return;
        };
        match (self.is_master(), message) {
            (
                true,
                Message::Hello {
                    protocol_version,
                    role,
                    build_id,
                },
            ) => {
                let verdict = if role != 1 {
                    Err(RejectReason::RoleConflict)
                } else if protocol_version != PROTOCOL_VERSION {
                    Err(RejectReason::ProtocolMismatch)
                } else if build_id != self.config.build_id {
                    Err(RejectReason::BuildMismatch)
                } else {
                    Ok(())
                };
                match verdict {
                    Ok(()) => self.accept(now_ms, link),
                    Err(reason) => {
                        self.disconnect();
                        self.link_state = LinkState::Rejected(reason);
                        self.send_with_session(link, 0, &Message::Reject(reason));
                    }
                }
            }
            (true, Message::Input(control)) => {
                if self.accepts(header.session, header.seq) {
                    self.remote = control;
                    self.remote_last_ms = now_ms;
                    self.peer_last_ms = now_ms;
                }
            }
            (false, Message::Accept) => {
                self.session = header.session;
                self.recv_seq = None;
                self.link_state = LinkState::Connected;
                self.peer_last_ms = now_ms;
                self.dirty = true;
            }
            (false, Message::Reject(reason)) => {
                if self.link_state != LinkState::Connected {
                    self.link_state = LinkState::Rejected(reason);
                }
            }
            (
                false,
                Message::State {
                    game_index,
                    layout,
                    state,
                },
            ) => {
                if !self.accepts(header.session, header.seq) {
                    return;
                }
                let index = usize::from(game_index);
                let Some(game) = self.games.get_mut(index) else {
                    return;
                };
                if game.decode_state(state).is_err() {
                    return;
                }
                if index != self.active
                    && let Some(previous) = self.games.get_mut(self.active)
                {
                    previous.clear_state();
                }
                self.active = index;
                self.layout = layout;
                self.peer_last_ms = now_ms;
                self.dirty = true;
            }
            _ => {}
        }
    }

    /// 연결 중이고 현재 세션의 새 순번인지 확인하고 순번을 기록한다.
    fn accepts(&mut self, session: u8, seq: u16) -> bool {
        if self.link_state != LinkState::Connected || session != self.session {
            return false;
        }
        if self.recv_seq.is_some_and(|last| !is_newer(seq, last)) {
            return false;
        }
        self.recv_seq = Some(seq);
        true
    }

    fn accept(&mut self, now_ms: u32, link: &mut dyn Link) {
        self.session = self.next_session;
        self.next_session = self.next_session.checked_add(1).unwrap_or(1);
        self.recv_seq = None;
        self.link_state = LinkState::Connected;
        self.remote = ControlState::default();
        self.remote_last_ms = now_ms;
        self.peer_last_ms = now_ms;
        self.send_with_session(link, self.session, &Message::Accept);
        self.active = 0;
        self.start_game(now_ms);
        self.send_state(link);
    }

    fn start_game(&mut self, now_ms: u32) {
        let seed = self
            .config
            .seed
            .wrapping_add(u64::from(self.session).wrapping_mul(0x9E37_79B9_7F4A_7C15))
            .wrapping_add(u64::from(now_ms));
        let layout = self
            .games
            .get(self.active)
            .map_or(ScreenLayout::Wide, |game| game.config().layout);
        self.ctx = Context::new(seed, layout);
        if let Some(game) = self.games.get_mut(self.active) {
            game.start(&mut self.ctx);
        }
        self.layout = self.ctx.layout();
        self.trackers[0].reset_baseline(self.input.state());
        self.trackers[1].reset_baseline(self.remote);
        self.ticker.reset(now_ms);
        self.sound.stop();
        self.dirty = true;
    }

    fn disconnect(&mut self) {
        self.link_state = LinkState::Searching;
        self.session = 0;
        self.recv_seq = None;
        self.last_hello_ms = None;
        if let Some(game) = self.games.get_mut(self.active) {
            game.clear_state();
        }
        self.sound.stop();
        self.dirty = true;
    }

    fn send_state(&mut self, link: &mut dyn Link) {
        let mut state = [0u8; MAX_STATE_BYTES];
        let mut writer = StateWriter::new(&mut state);
        let Some(game) = self.games.get(self.active) else {
            return;
        };
        if game.encode_state(&mut writer).is_err() {
            return;
        }
        let len = writer.len();
        let message = Message::State {
            game_index: u8::try_from(self.active).unwrap_or(0),
            layout: self.layout,
            state: &state[..len],
        };
        self.send(link, &message);
    }

    fn send(&mut self, link: &mut dyn Link, message: &Message<'_>) {
        self.send_with_session(link, self.session, message);
    }

    fn send_with_session(&mut self, link: &mut dyn Link, session: u8, message: &Message<'_>) {
        let mut buf = [0u8; MAX_MESSAGE_LEN];
        self.send_seq = self.send_seq.wrapping_add(1);
        if let Ok(len) = protocol::encode(session, self.send_seq, message, &mut buf) {
            // 송신 실패(큐 가득 참 등)는 다음 틱에 다시 보내므로 무시한다.
            let _ = link.send(&buf[..len]);
        }
    }

    fn render(&mut self, display: &mut dyn Display) -> Result<()> {
        if self.frame.layout() != self.layout {
            self.frame.set_layout(self.layout);
        }
        self.frame.clear();
        if self.link_state == LinkState::Connected
            && let Some(game) = self.games.get(self.active)
        {
            game.render(&mut self.frame);
        }
        self.frame.copy_panel(self.config.role, &mut self.panel)?;
        display.write_frame(&self.panel)?;
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::tests::DotGame;
    use crate::hal::RawInput;
    use crate::input::Direction;
    use crate::testing::{FakeDisplay, FakeInput, LinkSwitch, LoopbackLink, loopback_pair};

    const BUILD: BuildId = [7; 8];
    const CENTER: u16 = 2048;

    struct Module {
        engine: Engine,
        display: FakeDisplay,
        input: FakeInput,
        link: LoopbackLink,
    }

    impl Module {
        fn new(role: Side, build_id: BuildId, link: LoopbackLink) -> Self {
            Self {
                engine: Engine::new(DotGame, EngineConfig::new(role, build_id), 0).unwrap(),
                display: FakeDisplay::default(),
                input: FakeInput::default(),
                link,
            }
        }

        fn step(&mut self, now_ms: u32) {
            let mut io = Io {
                display: &mut self.display,
                input: &mut self.input,
                link: &mut self.link,
                buzzer: None,
            };
            self.engine.step(now_ms, &mut io).unwrap();
        }
    }

    struct Pair {
        left: Module,
        right: Module,
        switch: LinkSwitch,
        now: u32,
    }

    impl Pair {
        fn new(right_build: BuildId) -> Self {
            let (a, b, switch) = loopback_pair();
            Self {
                left: Module::new(Side::Left, BUILD, a),
                right: Module::new(Side::Right, right_build, b),
                switch,
                now: 0,
            }
        }

        /// 4 ms 간격으로 `ms`만큼 돌린다.
        fn run(&mut self, ms: u32) {
            for _ in 0..ms / 4 {
                self.now += 4;
                self.right.step(self.now);
                self.left.step(self.now);
            }
        }
    }

    fn joystick(direction: Option<Direction>, button: bool) -> RawInput {
        let (x, y) = match direction {
            None => (CENTER, CENTER),
            Some(Direction::Up) => (CENTER, 0),
            Some(Direction::Down) => (CENTER, 4095),
            Some(Direction::Left) => (0, CENTER),
            Some(Direction::Right) => (4095, CENTER),
        };
        RawInput {
            x,
            y,
            button_down: button,
        }
    }

    #[test]
    fn requires_at_least_one_game() {
        let result = Engine::new(Games::new(), EngineConfig::new(Side::Left, BUILD), 0);
        assert_eq!(result.err(), Some(Error::InvalidArgument));
    }

    #[test]
    fn modules_connect_and_render_same_frame() {
        let mut pair = Pair::new(BUILD);
        assert_eq!(pair.left.engine.link_state(), LinkState::Searching);
        pair.run(300);
        assert_eq!(pair.left.engine.link_state(), LinkState::Connected);
        assert_eq!(pair.right.engine.link_state(), LinkState::Connected);

        // 왼쪽 패널 (0,0)에 흰 점. 슬레이브도 같은 상태로 같은 전체 프레임을 그린다.
        assert_eq!(pair.left.display.last_frame()[0], Rgb::WHITE);
        assert_eq!(pair.right.engine.frame().pixel(0, 0), Some(Rgb::WHITE));
        assert_eq!(
            pair.left.display.brightness_percent,
            Some(DEFAULT_BRIGHTNESS_PERCENT)
        );
    }

    #[test]
    fn master_input_moves_dot_and_slave_follows() {
        let mut pair = Pair::new(BUILD);
        pair.run(300);
        pair.left.input.raw = joystick(Some(Direction::Right), false);
        pair.run(50);
        pair.left.input.raw = joystick(None, false);
        pair.run(50);
        assert_eq!(pair.left.engine.frame().pixel(1, 0), Some(Rgb::WHITE));
        assert_eq!(pair.right.engine.frame().pixel(1, 0), Some(Rgb::WHITE));
        assert_eq!(pair.right.engine.frame().pixel(0, 0), Some(Rgb::BLACK));
    }

    #[test]
    fn dot_crosses_into_right_panel() {
        let mut pair = Pair::new(BUILD);
        pair.run(300);
        for _ in 0..16 {
            pair.left.input.raw = joystick(Some(Direction::Right), false);
            pair.run(40);
            pair.left.input.raw = joystick(None, false);
            pair.run(40);
        }
        // x = 16은 오른쪽 패널의 (0, 0).
        assert_eq!(pair.right.display.last_frame()[0], Rgb::WHITE);
        assert!(
            pair.left
                .display
                .last_frame()
                .iter()
                .all(|&p| p == Rgb::BLACK)
        );
    }

    #[test]
    fn button_press_on_master_toggles_color_once() {
        let mut pair = Pair::new(BUILD);
        pair.run(300);
        pair.left.input.raw = joystick(None, true);
        pair.run(200);
        assert_eq!(
            pair.left.engine.frame().pixel(0, 0),
            Some(Rgb::new(255, 0, 0))
        );
        pair.left.input.raw = joystick(None, false);
        pair.run(100);
        assert_eq!(
            pair.left.engine.frame().pixel(0, 0),
            Some(Rgb::new(255, 0, 0))
        );
    }

    #[test]
    fn build_mismatch_is_rejected_and_screen_stays_dark() {
        let mut pair = Pair::new([9; 8]);
        pair.run(500);
        assert_eq!(
            pair.left.engine.link_state(),
            LinkState::Rejected(RejectReason::BuildMismatch)
        );
        assert_eq!(
            pair.right.engine.link_state(),
            LinkState::Rejected(RejectReason::BuildMismatch)
        );
        assert!(
            pair.left
                .display
                .last_frame()
                .iter()
                .all(|&p| p == Rgb::BLACK)
        );
    }

    #[test]
    fn two_masters_never_connect() {
        let (a, b, _switch) = loopback_pair();
        let mut x = Module::new(Side::Left, BUILD, a);
        let mut y = Module::new(Side::Left, BUILD, b);
        for now in (0..1000).step_by(4) {
            x.step(now);
            y.step(now);
        }
        assert_eq!(x.engine.link_state(), LinkState::Searching);
        assert_eq!(y.engine.link_state(), LinkState::Searching);
    }

    #[test]
    fn reconnects_after_link_loss_with_new_session() {
        let mut pair = Pair::new(BUILD);
        pair.run(300);
        let first_session = pair.left.engine.session;
        pair.switch.set_connected(false);
        pair.run(700);
        assert_eq!(pair.left.engine.link_state(), LinkState::Searching);
        assert_eq!(pair.right.engine.link_state(), LinkState::Searching);
        assert!(
            pair.right
                .display
                .last_frame()
                .iter()
                .all(|&p| p == Rgb::BLACK)
        );

        pair.switch.set_connected(true);
        pair.run(400);
        assert_eq!(pair.left.engine.link_state(), LinkState::Connected);
        assert_eq!(pair.right.engine.link_state(), LinkState::Connected);
        assert_ne!(pair.left.engine.session, first_session);
        assert_eq!(pair.right.engine.session, pair.left.engine.session);
    }

    #[test]
    fn remote_input_goes_stale_then_disconnects() {
        let mut pair = Pair::new(BUILD);
        pair.run(300);
        pair.switch.set_connected(false);
        // 슬레이브 입력이 끊겨도 250ms까지는 연결 유지.
        pair.run(240);
        assert_eq!(pair.left.engine.link_state(), LinkState::Connected);
        pair.run(400);
        assert_eq!(pair.left.engine.link_state(), LinkState::Searching);
    }

    #[test]
    fn stale_or_foreign_packets_are_ignored() {
        let mut pair = Pair::new(BUILD);
        pair.run(300);
        let session = pair.left.engine.session;
        // 다른 세션의 State를 슬레이브에 직접 넣는다.
        let mut buf = [0; MAX_MESSAGE_LEN];
        let fake = Message::State {
            game_index: 0,
            layout: ScreenLayout::Wide,
            state: &[5, 5, 0],
        };
        let len = protocol::encode(session.wrapping_add(1), 60000, &fake, &mut buf).unwrap();
        pair.right
            .engine
            .handle_message(&buf[..len], pair.now, &mut pair.right.link);
        assert_eq!(pair.right.engine.frame().pixel(5, 5), Some(Rgb::BLACK));
        // 쓰레기 바이트도 무시한다.
        pair.right
            .engine
            .handle_message(b"garbage", pair.now, &mut pair.right.link);
        pair.run(50);
        assert_eq!(pair.right.engine.link_state(), LinkState::Connected);
    }

    #[test]
    fn exit_restarts_game() {
        let mut pair = Pair::new(BUILD);
        pair.run(300);
        pair.left.input.raw = joystick(None, true);
        pair.run(100); // 빨간색으로
        pair.left.input.raw = joystick(Some(Direction::Up), true);
        pair.run(100); // y == 0에서 위 + 버튼 → exit → 다시 시작(흰색)
        assert_eq!(pair.left.engine.frame().pixel(0, 0), Some(Rgb::WHITE));
        assert_eq!(pair.left.engine.link_state(), LinkState::Connected);
    }
}
