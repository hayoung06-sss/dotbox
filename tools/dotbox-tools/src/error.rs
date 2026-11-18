use std::fmt;
use std::path::Path;

/// 도구 작업의 오류. 사람이 읽을 원인과, 있으면 해결 방법을 담는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
    hint: Option<String>,
}

/// `Result<T, dotbox_tools::Error>`의 줄임.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            hint: None,
        }
    }

    /// 해결 방법을 붙인다.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// 파일 입출력 오류를 경로와 함께 감싼다.
    pub fn io(action: &str, path: &Path, err: std::io::Error) -> Self {
        Self::new(format!("{} {action} 실패: {err}", path.display()))
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}
