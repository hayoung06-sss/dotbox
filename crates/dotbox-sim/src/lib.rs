//! PC 시뮬레이터(`docs/game-api.md` 8장).
//!
//! 한 프로세스에서 마스터(왼쪽)와 슬레이브(오른쪽) 엔진을 둘 다 돌리고, 메모리 링크로 잇는다.
//! 그래서 기기와 같은 경로(직렬화 → 전송 → 역직렬화 → 렌더)를 PC에서 검증한다.

mod keys;
mod view;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dotbox_core::protocol::BuildId;
use dotbox_core::testing::{FakeDisplay, FakeInput, LinkSwitch, LoopbackLink, loopback_pair};
use dotbox_core::{Engine, EngineConfig, Games, Io, LinkState, Result, Side};
use minifb::{Key, KeyRepeat, Window, WindowOptions};

pub use keys::{KeyState, raw_input};
pub use view::{WINDOW_HEIGHT, WINDOW_WIDTH, compose};

/// 자동 테스트용: 이 환경 변수에 밀리초를 주면 창 없이 그 시간만큼 돌리고 끝낸다.
pub const HEADLESS_ENV: &str = "DOTBOX_SIM_HEADLESS_MS";

/// 시뮬레이터의 두 모듈이 쓰는 빌드 식별자. 같은 프로세스라 항상 같다.
const SIM_BUILD_ID: BuildId = *b"dotbxsim";

/// 루프 간격. 틱(16 ms)보다 잘게 돌려 입력과 수신을 자주 처리한다.
const STEP_INTERVAL: Duration = Duration::from_millis(2);

/// 한 모듈의 엔진과 가짜 장치.
struct Module {
    engine: Engine,
    display: FakeDisplay,
    input: FakeInput,
    link: LoopbackLink,
}

impl Module {
    fn step(&mut self, now_ms: u32) -> Result<()> {
        let mut io = Io {
            display: &mut self.display,
            input: &mut self.input,
            link: &mut self.link,
            buzzer: None,
        };
        self.engine.step(now_ms, &mut io)
    }
}

/// 마스터와 슬레이브 한 쌍.
struct Simulator {
    left: Module,
    right: Module,
    switch: LinkSwitch,
    started: Instant,
}

impl Simulator {
    fn new<F, T>(make_games: &F) -> Result<Self>
    where
        F: Fn() -> T,
        T: Into<Games>,
    {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        let (left_link, right_link, switch) = loopback_pair();
        let module = |role: Side, link: LoopbackLink| -> Result<Module> {
            let config = EngineConfig {
                seed,
                ..EngineConfig::new(role, SIM_BUILD_ID)
            };
            Ok(Module {
                engine: Engine::new(make_games(), config, 0)?,
                display: FakeDisplay::default(),
                input: FakeInput::default(),
                link,
            })
        };
        Ok(Self {
            left: module(Side::Left, left_link)?,
            right: module(Side::Right, right_link)?,
            switch,
            started: Instant::now(),
        })
    }

    fn now_ms(&self) -> u32 {
        // u32 밀리초는 약 49일마다 순환한다. 엔진은 순환을 처리한다.
        self.started.elapsed().as_millis() as u32
    }

    fn step(&mut self) -> Result<()> {
        let now = self.now_ms();
        self.right.step(now)?;
        self.left.step(now)
    }

    fn title(&self) -> String {
        let state = match self.left.engine.link_state() {
            LinkState::Searching => "연결 대기".to_owned(),
            LinkState::Connected => "연결됨".to_owned(),
            LinkState::Rejected(reason) => format!("거부됨: {reason:?}"),
        };
        let link = if self.switch.is_connected() {
            ""
        } else {
            " [링크 끊김]"
        };
        format!(
            "DotBox 시뮬레이터 - {state}{link}  (P1: WASD+Space, P2: 방향키+Enter, F2: 링크, Esc: 종료)"
        )
    }
}

/// 게임을 PC 창에서 실행한다. `make_games`는 게임(또는 [`Games`] 목록)을 만드는 함수다.
///
/// ```no_run
/// # use dotbox_core::Games;
/// # fn make() -> Games { Games::new() }
/// dotbox_sim::run(make).unwrap();
/// ```
pub fn run<F, T>(make_games: F) -> Result<()>
where
    F: Fn() -> T,
    T: Into<Games>,
{
    let mut sim = Simulator::new(&make_games)?;

    if let Some(ms) = std::env::var(HEADLESS_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        return run_headless(&mut sim, Duration::from_millis(ms));
    }

    let mut window = Window::new(
        &sim.title(),
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        WindowOptions::default(),
    )
    .map_err(|err| {
        eprintln!("창을 만들지 못했습니다: {err}");
        dotbox_core::Error::Unsupported
    })?;
    window.set_target_fps(0);

    let mut buffer = vec![0u32; WINDOW_WIDTH * WINDOW_HEIGHT];
    let mut title = String::new();
    let mut last_draw = Instant::now() - Duration::from_secs(1);

    while window.is_open() && !window.is_key_down(Key::Escape) {
        if window.is_key_pressed(Key::F2, KeyRepeat::No) {
            sim.switch.set_connected(!sim.switch.is_connected());
        }
        sim.left.input.raw = raw_input(keys::left_keys(|k| window.is_key_down(k)));
        sim.right.input.raw = raw_input(keys::right_keys(|k| window.is_key_down(k)));
        sim.step()?;

        // 창은 약 60fps로 다시 그린다. 그 사이에도 엔진은 계속 돈다.
        if last_draw.elapsed() >= Duration::from_millis(16) {
            compose(
                sim.left.display.last_frame(),
                sim.right.display.last_frame(),
                &mut buffer,
            );
            window
                .update_with_buffer(&buffer, WINDOW_WIDTH, WINDOW_HEIGHT)
                .map_err(|_| dotbox_core::Error::Io)?;
            let new_title = sim.title();
            if new_title != title {
                window.set_title(&new_title);
                title = new_title;
            }
            last_draw = Instant::now();
        } else {
            window.update();
            std::thread::sleep(STEP_INTERVAL);
        }
    }
    Ok(())
}

fn run_headless(sim: &mut Simulator, duration: Duration) -> Result<()> {
    let started = Instant::now();
    while started.elapsed() < duration {
        sim.step()?;
        std::thread::sleep(STEP_INTERVAL);
    }
    eprintln!(
        "dotbox-sim: 창 없이 {} ms 실행, 연결 상태 {:?}",
        duration.as_millis(),
        sim.left.engine.link_state()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotbox_core::{
        Context, FrameBuffer, Game, GameConfig, GameState, Input, StateReader, StateWriter,
    };

    struct Blink;

    struct Lit(bool);

    impl GameState for Lit {
        const MAX_BYTES: usize = 1;
        fn encode(&self, out: &mut StateWriter<'_>) -> Result<()> {
            out.bool(self.0)
        }
        fn decode(input: &mut StateReader<'_>) -> Result<Self> {
            Ok(Self(input.bool()?))
        }
    }

    impl Game for Blink {
        type State = Lit;
        fn config(&self) -> GameConfig {
            GameConfig::new("blink")
        }
        fn init(&mut self, _ctx: &mut Context) -> Lit {
            Lit(true)
        }
        fn update(&mut self, _state: &mut Lit, _input: &Input, _ctx: &mut Context) {}
        fn render(state: &Lit, frame: &mut FrameBuffer) {
            if state.0 {
                frame.set_pixel(31, 15, dotbox_core::Rgb::WHITE);
            }
        }
    }

    #[test]
    fn simulator_connects_both_engines() {
        let mut sim = Simulator::new(&|| Blink).unwrap();
        // 실제 시계 대신 직접 시각을 넣어 결정적으로 돌린다.
        for now in (0..400).step_by(4) {
            sim.right.step(now).unwrap();
            sim.left.step(now).unwrap();
        }
        assert_eq!(sim.left.engine.link_state(), LinkState::Connected);
        assert_eq!(sim.right.engine.link_state(), LinkState::Connected);
        // (31, 15)는 오른쪽 패널의 마지막 픽셀이다. 슬레이브가 받은 상태로 그렸다.
        assert_eq!(sim.right.display.last_frame()[255], dotbox_core::Rgb::WHITE);
        assert!(sim.title().contains("연결됨"));
    }

    #[test]
    fn headless_run_finishes() {
        let mut sim = Simulator::new(&|| Blink).unwrap();
        run_headless(&mut sim, Duration::from_millis(50)).unwrap();
    }
}
