//! 엔진 동작 확인용 2인 예제.
//!
//! - P1(왼쪽 모듈, WASD)은 초록 점, P2(오른쪽 모듈, 방향키)는 파란 점을 움직인다.
//! - 방향을 누르고 있으면 계속 움직인다. 버튼을 누르면 색이 바뀌고 소리가 난다.
//! - 32×16 화면 전체를 오가므로 패널 경계(x = 15/16)를 넘는 동작도 확인할 수 있다.

use dotbox_core::{
    Context, FrameBuffer, Game, GameConfig, GameState, Input, Note, Result, Rgb, StateReader,
    StateWriter,
};

/// 방향을 누르고 있을 때 몇 틱마다 한 칸 움직일지.
const MOVE_EVERY_TICKS: u8 = 6;

const COLORS: [Rgb; 4] = [
    Rgb::new(0, 255, 0),
    Rgb::new(0, 128, 255),
    Rgb::new(255, 64, 0),
    Rgb::new(255, 255, 0),
];

static BEEP: [Note; 1] = [Note::new(880, 60)];

#[derive(Debug, Default)]
pub struct Hello {
    /// 플레이어별 이동 대기 틱(화면과 무관하므로 State에 넣지 않는다).
    cooldown: [u8; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dot {
    pub x: u8,
    pub y: u8,
    pub color: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelloState {
    pub dots: [Dot; 2],
}

impl GameState for HelloState {
    const MAX_BYTES: usize = 6;

    fn encode(&self, out: &mut StateWriter<'_>) -> Result<()> {
        for dot in &self.dots {
            out.u8(dot.x)?;
            out.u8(dot.y)?;
            out.u8(dot.color)?;
        }
        Ok(())
    }

    fn decode(input: &mut StateReader<'_>) -> Result<Self> {
        let mut dot = || -> Result<Dot> {
            Ok(Dot {
                x: input.u8()?,
                y: input.u8()?,
                color: input.u8()?,
            })
        };
        Ok(Self {
            dots: [dot()?, dot()?],
        })
    }
}

impl Hello {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Game for Hello {
    type State = HelloState;

    fn config(&self) -> GameConfig {
        GameConfig::new("hello")
    }

    fn init(&mut self, _ctx: &mut Context) -> HelloState {
        self.cooldown = [0; 2];
        HelloState {
            dots: [
                Dot {
                    x: 7,
                    y: 7,
                    color: 0,
                },
                Dot {
                    x: 24,
                    y: 7,
                    color: 1,
                },
            ],
        }
    }

    fn update(&mut self, state: &mut HelloState, input: &Input, ctx: &mut Context) {
        for (index, (dot, player)) in state.dots.iter_mut().zip(input.players).enumerate() {
            let cooldown = &mut self.cooldown[index];
            if let Some(direction) = player.direction {
                // 처음 누른 틱에 바로 한 칸, 그 뒤로는 MOVE_EVERY_TICKS마다 한 칸.
                *cooldown = cooldown.saturating_sub(1);
                if player.direction_pressed.is_some() || *cooldown == 0 {
                    let (dx, dy) = direction.delta();
                    dot.x = (i32::from(dot.x) + dx).clamp(0, 31) as u8;
                    dot.y = (i32::from(dot.y) + dy).clamp(0, 15) as u8;
                    *cooldown = MOVE_EVERY_TICKS;
                }
            } else {
                *cooldown = 0;
            }
            if player.button.pressed {
                dot.color = (dot.color + 1) % COLORS.len() as u8;
                ctx.play_sound(&BEEP);
            }
        }
    }

    fn render(state: &HelloState, frame: &mut FrameBuffer) {
        // 패널 경계 표시: 두 패널의 맞닿은 가장자리에 어두운 점선.
        for y in (0..16).step_by(2) {
            frame.set_pixel(15, y, Rgb::new(24, 24, 24));
            frame.set_pixel(16, y, Rgb::new(24, 24, 24));
        }
        for dot in &state.dots {
            let color = COLORS[usize::from(dot.color) % COLORS.len()];
            frame.set_pixel(i32::from(dot.x), i32::from(dot.y), color);
        }
    }
}

fn main() -> Result<()> {
    dotbox_sim::run(Hello::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotbox_core::testing::Harness;
    use dotbox_core::{ButtonState, Direction, PlayerInput};

    fn input(p1: PlayerInput, p2: PlayerInput) -> Input {
        Input {
            players: [p1, p2],
            ..Input::default()
        }
    }

    fn hold(direction: Direction, first_tick: bool) -> PlayerInput {
        PlayerInput {
            direction: Some(direction),
            direction_pressed: first_tick.then_some(direction),
            ..PlayerInput::default()
        }
    }

    #[test]
    fn players_move_their_own_dot() {
        let mut h = Harness::new(Hello::new(), 1);
        h.step(&input(
            hold(Direction::Left, true),
            hold(Direction::Down, true),
        ));
        assert_eq!((h.state().dots[0].x, h.state().dots[0].y), (6, 7));
        assert_eq!((h.state().dots[1].x, h.state().dots[1].y), (24, 8));
    }

    #[test]
    fn holding_repeats_movement() {
        let mut h = Harness::new(Hello::new(), 1);
        h.step(&input(hold(Direction::Right, true), PlayerInput::default()));
        h.run(
            u32::from(MOVE_EVERY_TICKS) * 3,
            &input(hold(Direction::Right, false), PlayerInput::default()),
        );
        assert_eq!(h.state().dots[0].x, 7 + 4);
    }

    #[test]
    fn dot_stays_on_screen_and_crosses_panels() {
        let mut h = Harness::new(Hello::new(), 1);
        for _ in 0..40 {
            h.step(&input(hold(Direction::Right, true), PlayerInput::default()));
            h.step(&input(PlayerInput::default(), PlayerInput::default()));
        }
        assert_eq!(h.state().dots[0].x, 31);
        assert_eq!(h.frame().pixel(31, 7), Some(COLORS[0]));
    }

    #[test]
    fn button_cycles_color() {
        let mut h = Harness::new(Hello::new(), 1);
        let press = PlayerInput {
            button: ButtonState {
                held: true,
                pressed: true,
                released: false,
            },
            ..PlayerInput::default()
        };
        h.step(&input(PlayerInput::default(), press));
        assert_eq!(h.state().dots[1].color, 2);
        assert_eq!(h.frame().pixel(24, 7), Some(COLORS[2]));
    }
}
