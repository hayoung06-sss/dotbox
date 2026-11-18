//! 게임 인터페이스와 여러 게임 등록(`docs/game-api.md` 3~4장).

use crate::display::{FrameBuffer, PANEL_PIXELS, ScreenLayout};
use crate::hal::Rgb;
use crate::input::Input;
use crate::sound::Sound;
use crate::state::{GameState, MAX_STATE_BYTES, StateReader, StateWriter};
use crate::{Error, Result};

/// 메뉴에 보일 16×16 아이콘(행 우선).
pub type Icon = [Rgb; PANEL_PIXELS];

/// 게임 정보.
#[derive(Debug, Clone, Copy)]
pub struct GameConfig {
    /// 게임 이름. ASCII. 기록 저장 키로도 쓴다.
    pub name: &'static str,
    /// 시작 레이아웃.
    pub layout: ScreenLayout,
    /// 메뉴 아이콘. 없으면 기본 아이콘.
    pub icon: Option<&'static Icon>,
}

impl GameConfig {
    /// 기본 레이아웃(`Wide`)과 기본 아이콘을 쓰는 설정.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            layout: ScreenLayout::Wide,
            icon: None,
        }
    }
}

/// DotBox 게임. 마스터(왼쪽 모듈)가 `init`/`update`를 부르고, 양쪽 모듈이 `render`를 부른다.
pub trait Game: 'static {
    /// 화면을 그리는 데 필요한 모든 것. 매 틱 마스터가 슬레이브로 보낸다.
    type State: GameState;

    fn config(&self) -> GameConfig;

    /// 첫 상태를 만든다. 마스터에서 게임을 시작할 때마다 부른다.
    fn init(&mut self, ctx: &mut Context) -> Self::State;

    /// 한 틱을 진행한다. 마스터에서만 부른다.
    fn update(&mut self, state: &mut Self::State, input: &Input, ctx: &mut Context);

    /// 상태를 그린다. 양쪽 모듈에서 부른다. 프레임은 엔진이 비워서 넘긴다.
    fn render(state: &Self::State, frame: &mut FrameBuffer);
}

/// 마스터에서 게임이 쓰는 서비스.
#[derive(Debug)]
pub struct Context {
    tick: u64,
    rng: u64,
    layout: ScreenLayout,
    exit_requested: bool,
    sound_request: Option<&'static Sound>,
}

impl Context {
    /// `seed`로 난수를 초기화한다. 0이면 고정값을 쓴다.
    pub fn new(seed: u64, layout: ScreenLayout) -> Self {
        Self {
            tick: 0,
            rng: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
            layout,
            exit_requested: false,
            sound_request: None,
        }
    }

    /// 게임을 시작한 뒤 진행한 틱 수.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 게임용 난수(xorshift64*). 보안용이 아니다.
    pub fn random_u32(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        // 상위 32비트를 쓴다.
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }

    /// `0..bound` 난수. `bound`가 0이면 0.
    pub fn random_below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.random_u32() % bound
        }
    }

    /// 효과음을 낸다. 재생 중인 소리를 끊는다.
    pub fn play_sound(&mut self, sound: &'static Sound) {
        self.sound_request = Some(sound);
    }

    pub fn layout(&self) -> ScreenLayout {
        self.layout
    }

    /// 레이아웃을 바꾼다. 다음 렌더부터 적용된다.
    pub fn set_layout(&mut self, layout: ScreenLayout) {
        self.layout = layout;
    }

    /// 게임을 끝낸다.
    pub fn exit(&mut self) {
        self.exit_requested = true;
    }

    pub(crate) fn advance_tick(&mut self) {
        self.tick += 1;
    }

    pub(crate) fn take_exit(&mut self) -> bool {
        std::mem::take(&mut self.exit_requested)
    }

    pub(crate) fn take_sound(&mut self) -> Option<&'static Sound> {
        self.sound_request.take()
    }
}

/// 타입을 지운 게임. 엔진이 여러 게임을 한 목록에 담기 위해 쓴다.
pub(crate) trait ErasedGame {
    fn config(&self) -> GameConfig;
    fn start(&mut self, ctx: &mut Context);
    fn update(&mut self, input: &Input, ctx: &mut Context);
    /// 현재 상태를 인코딩한다. 상태가 없으면 아무것도 쓰지 않는다.
    fn encode_state(&self, out: &mut StateWriter<'_>) -> Result<()>;
    /// 받은 상태로 바꾼다(슬레이브).
    fn decode_state(&mut self, bytes: &[u8]) -> Result<()>;
    fn clear_state(&mut self);
    fn render(&self, frame: &mut FrameBuffer);
}

struct Slot<G: Game> {
    game: G,
    state: Option<G::State>,
}

impl<G: Game> ErasedGame for Slot<G> {
    fn config(&self) -> GameConfig {
        self.game.config()
    }

    fn start(&mut self, ctx: &mut Context) {
        self.state = Some(self.game.init(ctx));
    }

    fn update(&mut self, input: &Input, ctx: &mut Context) {
        if let Some(state) = &mut self.state {
            self.game.update(state, input, ctx);
        }
    }

    fn encode_state(&self, out: &mut StateWriter<'_>) -> Result<()> {
        match &self.state {
            Some(state) => state.encode(out),
            None => Ok(()),
        }
    }

    fn decode_state(&mut self, bytes: &[u8]) -> Result<()> {
        let mut reader = StateReader::new(bytes);
        let state = G::State::decode(&mut reader)?;
        if reader.remaining() != 0 {
            return Err(Error::InvalidArgument);
        }
        self.state = Some(state);
        Ok(())
    }

    fn clear_state(&mut self) {
        self.state = None;
    }

    fn render(&self, frame: &mut FrameBuffer) {
        if let Some(state) = &self.state {
            G::render(state, frame);
        }
    }
}

/// 펌웨어에 넣을 게임 목록. 등록 순서가 게임 번호다.
#[derive(Default)]
pub struct Games {
    entries: Vec<Box<dyn ErasedGame>>,
}

impl Games {
    pub fn new() -> Self {
        Self::default()
    }

    /// 게임을 추가한다.
    ///
    /// # Panics
    ///
    /// 상태 최대 크기가 [`MAX_STATE_BYTES`]를 넘거나 게임이 255개를 넘으면 패닉한다.
    /// 프로그램 구성 오류라서 시작할 때 바로 드러나게 한다.
    #[must_use]
    pub fn with<G: Game>(mut self, game: G) -> Self {
        assert!(
            G::State::MAX_BYTES <= MAX_STATE_BYTES,
            "{}: State::MAX_BYTES({})가 MAX_STATE_BYTES({MAX_STATE_BYTES})를 넘습니다",
            game.config().name,
            G::State::MAX_BYTES
        );
        assert!(
            self.entries.len() < usize::from(u8::MAX),
            "게임은 255개까지 등록할 수 있습니다"
        );
        self.entries.push(Box::new(Slot { game, state: None }));
        self
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 등록 순서대로 게임 정보.
    pub fn configs(&self) -> impl Iterator<Item = GameConfig> + '_ {
        self.entries.iter().map(|entry| entry.config())
    }

    pub(crate) fn get_mut(&mut self, index: usize) -> Option<&mut (dyn ErasedGame + 'static)> {
        self.entries.get_mut(index).map(|entry| entry.as_mut())
    }

    pub(crate) fn get(&self, index: usize) -> Option<&(dyn ErasedGame + 'static)> {
        self.entries.get(index).map(|entry| entry.as_ref())
    }
}

impl std::fmt::Debug for Games {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.configs().map(|config| config.name))
            .finish()
    }
}

impl<G: Game> From<G> for Games {
    fn from(game: G) -> Self {
        Games::new().with(game)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::input::Direction;

    /// 테스트용 게임: 점 하나를 P1 방향으로 움직이고 버튼으로 색을 바꾼다.
    pub struct DotGame;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DotState {
        pub x: u8,
        pub y: u8,
        pub red: bool,
    }

    impl GameState for DotState {
        const MAX_BYTES: usize = 3;
        fn encode(&self, out: &mut StateWriter<'_>) -> Result<()> {
            out.u8(self.x)?;
            out.u8(self.y)?;
            out.bool(self.red)
        }
        fn decode(input: &mut StateReader<'_>) -> Result<Self> {
            Ok(Self {
                x: input.u8()?,
                y: input.u8()?,
                red: input.bool()?,
            })
        }
    }

    impl Game for DotGame {
        type State = DotState;
        fn config(&self) -> GameConfig {
            GameConfig::new("dot")
        }
        fn init(&mut self, _ctx: &mut Context) -> DotState {
            DotState {
                x: 0,
                y: 0,
                red: false,
            }
        }
        fn update(&mut self, state: &mut DotState, input: &Input, ctx: &mut Context) {
            let p1 = input.players[0];
            if let Some(d) = p1.direction_pressed {
                let (dx, dy) = d.delta();
                state.x = (i32::from(state.x) + dx).clamp(0, 31) as u8;
                state.y = (i32::from(state.y) + dy).clamp(0, 15) as u8;
            }
            if p1.button.pressed {
                state.red = !state.red;
            }
            if p1.direction == Some(Direction::Up) && p1.button.held && state.y == 0 {
                ctx.exit();
            }
        }
        fn render(state: &DotState, frame: &mut FrameBuffer) {
            let color = if state.red {
                Rgb::new(255, 0, 0)
            } else {
                Rgb::WHITE
            };
            frame.set_pixel(i32::from(state.x), i32::from(state.y), color);
        }
    }

    struct TooBig;
    struct BigState;
    impl GameState for BigState {
        const MAX_BYTES: usize = MAX_STATE_BYTES + 1;
        fn encode(&self, _out: &mut StateWriter<'_>) -> Result<()> {
            Ok(())
        }
        fn decode(_input: &mut StateReader<'_>) -> Result<Self> {
            Ok(Self)
        }
    }
    impl Game for TooBig {
        type State = BigState;
        fn config(&self) -> GameConfig {
            GameConfig::new("too-big")
        }
        fn init(&mut self, _ctx: &mut Context) -> BigState {
            BigState
        }
        fn update(&mut self, _s: &mut BigState, _i: &Input, _c: &mut Context) {}
        fn render(_s: &BigState, _f: &mut FrameBuffer) {}
    }

    #[test]
    #[should_panic(expected = "MAX_STATE_BYTES")]
    fn rejects_oversized_state() {
        let _ = Games::new().with(TooBig);
    }

    #[test]
    fn registry_keeps_order() {
        let games = Games::new().with(DotGame).with(DotGame);
        assert_eq!(games.len(), 2);
        let names: Vec<_> = games.configs().map(|c| c.name).collect();
        assert_eq!(names, ["dot", "dot"]);
        let single: Games = DotGame.into();
        assert_eq!(single.len(), 1);
    }

    #[test]
    fn erased_slot_round_trips_state() {
        let mut games = Games::from(DotGame);
        let mut ctx = Context::new(1, ScreenLayout::Wide);
        let slot = games.get_mut(0).unwrap();
        let mut empty = [0; 4];
        let mut w = StateWriter::new(&mut empty);
        slot.encode_state(&mut w).unwrap();
        assert!(w.is_empty(), "시작 전에는 상태가 없다");
        slot.start(&mut ctx);

        let mut buf = [0; MAX_STATE_BYTES];
        let mut w = StateWriter::new(&mut buf);
        slot.encode_state(&mut w).unwrap();
        let len = w.len();
        assert_eq!(len, 3);

        let mut other = Games::from(DotGame);
        let remote = other.get_mut(0).unwrap();
        remote.decode_state(&buf[..len]).unwrap();
        // 남는 바이트가 있으면 거부한다.
        assert!(remote.decode_state(&buf[..len + 1]).is_err());

        let mut frame = FrameBuffer::default();
        remote.render(&mut frame);
        assert_eq!(frame.pixel(0, 0), Some(Rgb::WHITE));
    }

    #[test]
    fn random_is_deterministic_per_seed_and_bounded() {
        let mut a = Context::new(42, ScreenLayout::Wide);
        let mut b = Context::new(42, ScreenLayout::Wide);
        let seq_a: Vec<_> = (0..5).map(|_| a.random_u32()).collect();
        let seq_b: Vec<_> = (0..5).map(|_| b.random_u32()).collect();
        assert_eq!(seq_a, seq_b);
        assert!(seq_a.windows(2).any(|w| w[0] != w[1]));
        let mut c = Context::new(0, ScreenLayout::Wide);
        for _ in 0..100 {
            assert!(c.random_below(7) < 7);
        }
        assert_eq!(c.random_below(0), 0);
    }

    #[test]
    fn context_requests_are_taken_once() {
        static S: [crate::sound::Note; 1] = [crate::sound::Note::new(440, 10)];
        let mut ctx = Context::new(1, ScreenLayout::Wide);
        ctx.exit();
        ctx.play_sound(&S);
        assert!(ctx.take_exit());
        assert!(!ctx.take_exit());
        assert!(ctx.take_sound().is_some());
        assert!(ctx.take_sound().is_none());
        ctx.set_layout(ScreenLayout::Mirrored);
        assert_eq!(ctx.layout(), ScreenLayout::Mirrored);
    }
}
