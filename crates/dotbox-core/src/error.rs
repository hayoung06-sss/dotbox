use std::fmt;

/// 엔진 공통 오류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// 인자가 범위를 벗어났거나 형식이 잘못됐다.
    InvalidArgument,
    /// 지금 상태에서는 할 수 없는 작업이다.
    InvalidState,
    /// 빌드나 보드가 지원하지 않는 기능이다.
    Unsupported,
    /// 메모리가 부족하다.
    OutOfMemory,
    /// 큐가 가득 찼다.
    QueueFull,
    /// 제한 시간을 넘겼다.
    Timeout,
    /// 상대 장치와 연결이 끊겼다.
    Disconnected,
    /// 장치 입출력에 실패했다.
    Io,
}

/// `Result<T, dotbox_core::Error>`의 줄임.
pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidArgument => "invalid argument",
            Self::InvalidState => "invalid state",
            Self::Unsupported => "unsupported",
            Self::OutOfMemory => "out of memory",
            Self::QueueFull => "queue full",
            Self::Timeout => "timeout",
            Self::Disconnected => "disconnected",
            Self::Io => "I/O error",
        };
        f.write_str(message)
    }
}

impl std::error::Error for Error {}
