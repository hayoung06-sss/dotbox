//! DotBox 호스트 개발 도구 라이브러리.
//!
//! `dotbox` CLI와 DotBox Studio가 함께 쓰는 실제 구현이다. 명령과 함수의 대응은 `docs/cli.md` 7장에 있다.
//! 게임의 `build.rs`도 이 crate를 쓰므로 GUI·터미널 관련 의존성을 넣지 않는다.

pub mod cargo;
pub mod device;
pub mod doctor;
mod error;
pub mod local;
pub mod manifest;
pub mod paths;
pub mod process;
pub mod project;
pub mod studio;

pub use dotbox_core::display::Side;
pub use error::{Error, Result};
pub use manifest::Project;

/// 도구 버전. 엔진 workspace 버전과 같다.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
