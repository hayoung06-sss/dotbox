//! 게임 프로젝트 설정 `dotbox.toml`과 프로젝트 찾기.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// 프로젝트 설정 파일 이름.
pub const MANIFEST_FILE: &str = "dotbox.toml";
/// 개인 설정 디렉토리 이름(커밋하지 않음).
pub const LOCAL_DIR: &str = ".dotbox";
/// 프로젝트 이름 최대 길이.
pub const NAME_MAX_CHARS: usize = 32;

/// `dotbox.toml`의 내용.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub project: ProjectSection,
    #[serde(default)]
    pub paths: PathsSection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSection {
    pub name: String,
}

/// 프로젝트 루트 기준 상대 경로.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PathsSection {
    pub game: PathBuf,
    pub sim: PathBuf,
    pub firmware: PathBuf,
    pub assets: PathBuf,
}

impl Default for PathsSection {
    fn default() -> Self {
        Self {
            game: "game".into(),
            sim: "sim".into(),
            firmware: "firmware".into(),
            assets: "assets".into(),
        }
    }
}

impl Manifest {
    /// 기본 경로를 쓰는 설정을 만든다.
    pub fn new(name: &str) -> Self {
        Self {
            project: ProjectSection {
                name: name.to_owned(),
            },
            paths: PathsSection::default(),
        }
    }

    /// TOML 문자열을 해석하고 이름 규칙을 검사한다.
    pub fn parse(text: &str) -> Result<Self> {
        let manifest: Self = toml::from_str(text)
            .map_err(|err| Error::new(format!("{MANIFEST_FILE} 형식 오류: {err}")))?;
        validate_name(&manifest.project.name)?;
        Ok(manifest)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("manifest is always serializable")
    }
}

/// 프로젝트 이름 규칙(`docs/cli.md` 5.4)을 검사한다.
pub fn validate_name(name: &str) -> Result<()> {
    let hint = "ASCII 소문자로 시작하고 소문자, 숫자, '-'만 쓰세요. 예: my-game";
    let invalid =
        |reason: &str| Err(Error::new(format!("프로젝트 이름 '{name}': {reason}")).with_hint(hint));

    if name.is_empty() {
        return invalid("비어 있습니다");
    }
    if name.chars().count() > NAME_MAX_CHARS {
        return invalid(&format!("{NAME_MAX_CHARS}자를 넘습니다"));
    }
    if !name.starts_with(|c: char| c.is_ascii_lowercase()) {
        return invalid("ASCII 소문자로 시작해야 합니다");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return invalid("쓸 수 없는 문자가 있습니다");
    }
    if name.ends_with('-') || name.contains("--") {
        return invalid("'-'로 끝나거나 '--'를 포함할 수 없습니다");
    }
    Ok(())
}

/// Rust 코드에서 쓰는 crate 이름(`my-game` → `my_game`).
pub fn crate_ident(name: &str) -> String {
    name.replace('-', "_")
}

/// Rust 타입 이름(`my-game` → `MyGame`, `pong-2p` → `Pong2p`).
pub fn type_ident(name: &str) -> String {
    name.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// 찾아서 읽은 게임 프로젝트.
#[derive(Debug, Clone)]
pub struct Project {
    root: PathBuf,
    manifest: Manifest,
}

impl Project {
    /// `root/dotbox.toml`을 읽는다.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(MANIFEST_FILE);
        let text = fs::read_to_string(&path).map_err(|err| Error::io("읽기", &path, err))?;
        let manifest = Manifest::parse(&text)?;
        let root = std::path::absolute(root).map_err(|err| Error::io("경로 해석", root, err))?;
        Ok(Self { root, manifest })
    }

    /// `start`에서 위로 올라가며 `dotbox.toml`이 있는 디렉토리를 찾는다.
    pub fn find_root(start: &Path) -> Option<PathBuf> {
        let start = std::path::absolute(start).ok()?;
        start
            .ancestors()
            .find(|dir| dir.join(MANIFEST_FILE).is_file())
            .map(Path::to_path_buf)
    }

    /// `start`에서 프로젝트를 찾아 읽는다. 없으면 오류.
    pub fn discover(start: &Path) -> Result<Self> {
        match Self::find_root(start) {
            Some(root) => Self::load(&root),
            None => Err(Error::new(format!(
                "{}에서 게임 프로젝트({MANIFEST_FILE})를 찾지 못했습니다",
                start.display()
            ))
            .with_hint("프로젝트 디렉토리에서 실행하거나 -C <DIR>로 지정하세요. 새로 만들려면 dotbox new <PATH>")),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn name(&self) -> &str {
        &self.manifest.project.name
    }

    pub fn game_dir(&self) -> PathBuf {
        self.root.join(&self.manifest.paths.game)
    }

    pub fn sim_dir(&self) -> PathBuf {
        self.root.join(&self.manifest.paths.sim)
    }

    pub fn firmware_dir(&self) -> PathBuf {
        self.root.join(&self.manifest.paths.firmware)
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.root.join(&self.manifest.paths.assets)
    }

    pub fn local_dir(&self) -> PathBuf {
        self.root.join(LOCAL_DIR)
    }

    /// 펌웨어 디렉토리가 있는지(`dotbox new --no-firmware`면 없다).
    pub fn has_firmware(&self) -> bool {
        self.firmware_dir().join("Cargo.toml").is_file()
    }

    /// 펌웨어 바이너리 패키지 이름.
    pub fn firmware_package(&self) -> String {
        format!("{}-firmware", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_names() {
        for name in ["a", "snake", "pong-2p", "my-game", "a1-b2"] {
            assert_eq!(validate_name(name), Ok(()), "{name}");
        }
        assert_eq!(validate_name(&"a".repeat(NAME_MAX_CHARS)), Ok(()));
    }

    #[test]
    fn rejects_invalid_names() {
        for name in [
            "", "Snake", "1snake", "-snake", "snake-", "my--game", "my_game", "my game", "게임",
        ] {
            let err = validate_name(name).unwrap_err();
            assert!(err.hint().is_some(), "{name}");
        }
        assert!(validate_name(&"a".repeat(NAME_MAX_CHARS + 1)).is_err());
    }

    #[test]
    fn type_ident_is_upper_camel_case() {
        assert_eq!(type_ident("my-game"), "MyGame");
        assert_eq!(type_ident("snake"), "Snake");
        assert_eq!(type_ident("pong-2p"), "Pong2p");
        assert_eq!(type_ident("a1-b2"), "A1B2");
    }

    #[test]
    fn crate_ident_replaces_dashes() {
        assert_eq!(crate_ident("my-game-2"), "my_game_2");
    }

    #[test]
    fn parses_minimal_manifest_with_default_paths() {
        let manifest = Manifest::parse("[project]\nname = \"snake\"\n").unwrap();
        assert_eq!(manifest, Manifest::new("snake"));
    }

    #[test]
    fn parses_custom_paths() {
        let manifest =
            Manifest::parse("[project]\nname = \"snake\"\n[paths]\nsim = \"desktop\"\n").unwrap();
        assert_eq!(manifest.paths.sim, PathBuf::from("desktop"));
        assert_eq!(manifest.paths.game, PathBuf::from("game"));
    }

    #[test]
    fn rejects_unknown_keys_and_bad_names() {
        assert!(Manifest::parse("[project]\nname = \"snake\"\nnmae = \"x\"\n").is_err());
        assert!(Manifest::parse("[project]\nname = \"Snake\"\n").is_err());
        assert!(Manifest::parse("[paths]\ngame = \"g\"\n").is_err());
    }

    #[test]
    fn manifest_round_trips_through_toml() {
        let manifest = Manifest::new("pong");
        assert_eq!(Manifest::parse(&manifest.to_toml()).unwrap(), manifest);
    }

    #[test]
    fn discovers_project_from_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(MANIFEST_FILE),
            Manifest::new("snake").to_toml(),
        )
        .unwrap();
        let nested = dir.path().join("game/src");
        fs::create_dir_all(&nested).unwrap();

        let project = Project::discover(&nested).unwrap();
        assert_eq!(project.name(), "snake");
        assert_eq!(project.root(), std::path::absolute(dir.path()).unwrap());
        assert_eq!(project.sim_dir(), project.root().join("sim"));
        assert_eq!(project.firmware_package(), "snake-firmware");
        assert!(!project.has_firmware());
    }

    #[test]
    fn discover_fails_outside_project() {
        let dir = tempfile::tempdir().unwrap();
        let err = Project::discover(dir.path()).unwrap_err();
        assert!(err.message().contains(MANIFEST_FILE));
        assert!(err.hint().is_some());
    }
}
