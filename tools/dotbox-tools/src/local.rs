//! 개인 설정 `.dotbox/local.toml`(포트, 마지막으로 구운 이미지).

use std::fs;
use std::path::PathBuf;

use dotbox_core::display::Side;
use serde::{Deserialize, Serialize};

use crate::manifest::Project;
use crate::{Error, Result};

/// 개인 설정 파일 이름.
pub const LOCAL_FILE: &str = "local.toml";

/// 모듈별 값.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerSide {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<String>,
}

impl PerSide {
    pub fn get(&self, side: Side) -> Option<&str> {
        match side {
            Side::Left => self.left.as_deref(),
            Side::Right => self.right.as_deref(),
        }
    }

    pub fn set(&mut self, side: Side, value: impl Into<String>) {
        let value = Some(value.into());
        match side {
            Side::Left => self.left = value,
            Side::Right => self.right = value,
        }
    }
}

/// `.dotbox/local.toml`의 내용.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalConfig {
    /// 모듈별 시리얼 포트.
    #[serde(default)]
    pub ports: PerSide,
    /// 모듈별로 마지막에 구운 이미지의 build_id.
    #[serde(default)]
    pub flashed: PerSide,
}

impl LocalConfig {
    pub fn path(project: &Project) -> PathBuf {
        project.local_dir().join(LOCAL_FILE)
    }

    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).map_err(|err| {
            Error::new(format!("{LOCAL_FILE} 형식 오류: {err}"))
                .with_hint("파일을 고치거나 지우세요. 지우면 포트를 다시 지정해야 합니다")
        })
    }

    /// 읽는다. 파일이 없으면 빈 설정이다.
    pub fn load(project: &Project) -> Result<Self> {
        let path = Self::path(project);
        match fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(Error::io("읽기", &path, err)),
        }
    }

    pub fn save(&self, project: &Project) -> Result<()> {
        let dir = project.local_dir();
        fs::create_dir_all(&dir).map_err(|err| Error::io("만들기", &dir, err))?;
        let path = Self::path(project);
        let text = toml::to_string(self).expect("local config is always serializable");
        fs::write(&path, text).map_err(|err| Error::io("쓰기", &path, err))
    }

    /// 두 모듈에 구운 이미지가 다른지. 한쪽이라도 기록이 없으면 비교하지 않는다.
    pub fn flashed_mismatch(&self) -> bool {
        matches!((&self.flashed.left, &self.flashed.right), (Some(l), Some(r)) if l != r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{MANIFEST_FILE, Manifest};

    #[test]
    fn empty_text_is_default() {
        assert_eq!(LocalConfig::parse("").unwrap(), LocalConfig::default());
    }

    #[test]
    fn parses_documented_example() {
        let config = LocalConfig::parse(
            "[ports]\nleft = \"COM5\"\nright = \"COM6\"\n[flashed]\nleft = \"aa\"\n",
        )
        .unwrap();
        assert_eq!(config.ports.get(Side::Left), Some("COM5"));
        assert_eq!(config.ports.get(Side::Right), Some("COM6"));
        assert_eq!(config.flashed.get(Side::Left), Some("aa"));
        assert_eq!(config.flashed.get(Side::Right), None);
    }

    #[test]
    fn mismatch_only_when_both_recorded_and_different() {
        let mut config = LocalConfig::default();
        assert!(!config.flashed_mismatch());
        config.flashed.set(Side::Left, "aa");
        assert!(!config.flashed_mismatch());
        config.flashed.set(Side::Right, "aa");
        assert!(!config.flashed_mismatch());
        config.flashed.set(Side::Right, "bb");
        assert!(config.flashed_mismatch());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(MANIFEST_FILE),
            Manifest::new("snake").to_toml(),
        )
        .unwrap();
        let project = Project::load(dir.path()).unwrap();

        assert_eq!(LocalConfig::load(&project).unwrap(), LocalConfig::default());

        let mut config = LocalConfig::default();
        config.ports.set(Side::Right, "COM7");
        config.flashed.set(Side::Right, "0123456789abcdef");
        config.save(&project).unwrap();
        assert_eq!(LocalConfig::load(&project).unwrap(), config);
    }
}
