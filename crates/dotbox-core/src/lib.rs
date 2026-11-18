//! 하드웨어 독립 DotBox 엔진.
//!
//! 게임은 [`Game`]을 구현하고, 플랫폼 실행기(`dotbox_sim::run`, `dotbox_esp::run`)에 넘긴다.
//! 구조와 규칙은 `docs/game-api.md`에 있다.

pub mod display;
mod engine;
mod error;
pub mod game;
pub mod hal;
pub mod input;
pub mod pairing;
pub mod protocol;
pub mod sound;
pub mod state;
pub mod testing;
pub mod tick;

pub use display::{FrameBuffer, ScreenLayout, Side};
pub use engine::{
    DEFAULT_BRIGHTNESS_PERCENT, Engine, EngineConfig, HELLO_INTERVAL_MS, INPUT_STALE_MS, Io,
    LINK_TIMEOUT_MS, LinkState,
};
pub use error::{Error, Result};
pub use game::{Context, Game, GameConfig, Games, Icon};
pub use hal::Rgb;
pub use input::{ButtonState, Direction, DockState, Input, PlayerInput};
pub use sound::{Note, Sound};
pub use state::{GameState, MAX_STATE_BYTES, StateReader, StateWriter};
