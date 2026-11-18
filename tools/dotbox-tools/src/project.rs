//! 게임 프로젝트 생성(`dotbox new`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::manifest::{MANIFEST_FILE, Manifest, crate_ident, type_ident, validate_name};
use crate::paths::{PROJECT_PATH_MAX_CHARS, path_issues};
use crate::process::to_forward_slashes;
use crate::{Error, Result};

/// 공식 엔진 저장소.
pub const OFFICIAL_ENGINE_GIT: &str = "https://github.com/hayoung06-sss/dotbox.git";

/// Windows에서 ESP-IDF 도구를 설치할 짧은 경로.
const WINDOWS_IDF_TOOLS_DIR: &str = "custom:C:/dbx/espressif";

/// 게임이 엔진을 가져오는 위치.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineSource {
    /// git 저장소. `rev`가 없으면 기본 브랜치.
    Git { url: String, rev: Option<String> },
    /// 로컬 엔진 저장소 루트.
    Path(PathBuf),
}

impl EngineSource {
    /// 공식 저장소의 기본 브랜치.
    pub fn official() -> Self {
        Self::Git {
            url: OFFICIAL_ENGINE_GIT.to_owned(),
            rev: None,
        }
    }

    /// `crate_dir`(엔진 저장소 안의 상대 경로)에 있는 crate의 의존성 표기.
    fn dependency(&self, crate_dir: &str) -> String {
        let quote = |text: &str| toml::Value::String(text.to_owned()).to_string();
        match self {
            Self::Git { url, rev: None } => format!("{{ git = {} }}", quote(url)),
            Self::Git {
                url,
                rev: Some(rev),
            } => {
                format!("{{ git = {}, rev = {} }}", quote(url), quote(rev))
            }
            Self::Path(root) => {
                let path = format!("{}/{crate_dir}", to_forward_slashes(root));
                format!("{{ path = {} }}", quote(&path))
            }
        }
    }

    /// 로컬 경로라면 엔진 저장소인지 확인하고 절대 경로로 바꾼다.
    fn resolved(&self) -> Result<Self> {
        match self {
            Self::Git { .. } => Ok(self.clone()),
            Self::Path(root) => {
                let root =
                    std::path::absolute(root).map_err(|err| Error::io("경로 해석", root, err))?;
                if !root.join("crates/dotbox-core/Cargo.toml").is_file() {
                    return Err(Error::new(format!(
                        "{}는 DotBox 엔진 저장소가 아닙니다",
                        root.display()
                    ))
                    .with_hint("crates/dotbox-core가 있는 엔진 저장소 루트를 지정하세요"));
                }
                Ok(Self::Path(root))
            }
        }
    }
}

/// `dotbox new` 옵션.
#[derive(Debug, Clone)]
pub struct NewOptions {
    /// 만들 디렉토리.
    pub path: PathBuf,
    /// 프로젝트 이름. 없으면 `path`의 마지막 부분.
    pub name: Option<String>,
    pub engine: EngineSource,
    /// `firmware/`를 만들지.
    pub firmware: bool,
    /// `git init`을 할지.
    pub git_init: bool,
}

/// 만든 결과.
#[derive(Debug, Clone)]
pub struct Created {
    pub root: PathBuf,
    pub name: String,
    /// 계속 진행했지만 알려야 할 문제(경로, git).
    pub warnings: Vec<String>,
}

/// 템플릿에 채울 값.
#[derive(Debug, Clone)]
pub struct TemplateContext {
    pub name: String,
    pub engine: EngineSource,
    pub firmware: bool,
    /// 펌웨어 `[build] target-dir`. `None`이면 적지 않는다(cargo 기본값).
    pub firmware_target_dir: Option<String>,
    /// `ESP_IDF_TOOLS_INSTALL_DIR` 값.
    pub idf_tools_dir: String,
}

impl TemplateContext {
    /// 현재 호스트 OS에 맞는 기본값으로 만든다.
    pub fn for_host(name: &str, engine: EngineSource, firmware: bool) -> Self {
        let windows = cfg!(windows);
        Self {
            name: name.to_owned(),
            engine,
            firmware,
            firmware_target_dir: windows.then(|| format!("C:/dbx/{name}")),
            idf_tools_dir: if windows {
                WINDOWS_IDF_TOOLS_DIR.to_owned()
            } else {
                "global".to_owned()
            },
        }
    }
}

macro_rules! template {
    ($path:literal) => {
        (
            $path,
            include_str!(concat!("../templates/", $path, ".tmpl")),
        )
    };
}

const COMMON_TEMPLATES: &[(&str, &str)] = &[
    template!("Cargo.toml"),
    template!("README.md"),
    template!("game/Cargo.toml"),
    template!("game/src/lib.rs"),
    template!("sim/Cargo.toml"),
    template!("sim/src/main.rs"),
];

const FIRMWARE_TEMPLATES: &[(&str, &str)] = &[
    template!("firmware/Cargo.toml"),
    template!("firmware/build.rs"),
    template!("firmware/src/main.rs"),
    template!("firmware/rust-toolchain.toml"),
    template!("firmware/.cargo/config.toml"),
    template!("firmware/espflash.toml"),
    template!("firmware/sdkconfig.defaults"),
    template!("firmware/partitions.csv"),
];

const GITIGNORE: &str = include_str!("../templates/gitignore.tmpl");

fn render(template: &str, context: &TemplateContext) -> String {
    let target_dir_line = match &context.firmware_target_dir {
        Some(dir) => format!(
            "# 짧은 ASCII 경로. 길면 Windows 경로 길이 제한에 걸린다.\ntarget-dir = {}\n",
            toml::Value::String(dir.clone())
        ),
        None => String::new(),
    };
    template
        .replace("{{name}}", &context.name)
        .replace("{{crate}}", &crate_ident(&context.name))
        .replace("{{type}}", &type_ident(&context.name))
        .replace(
            "{{core_dep}}",
            &context.engine.dependency("crates/dotbox-core"),
        )
        .replace(
            "{{sim_dep}}",
            &context.engine.dependency("crates/dotbox-sim"),
        )
        .replace(
            "{{esp_dep}}",
            &context.engine.dependency("platform/dotbox-esp"),
        )
        .replace("{{target_dir_line}}", &target_dir_line)
        .replace("{{idf_tools_dir}}", &context.idf_tools_dir)
}

/// 만들 파일 목록(프로젝트 루트 기준 상대 경로, 내용). 디스크를 건드리지 않는다.
pub fn render_files(context: &TemplateContext) -> Vec<(PathBuf, String)> {
    let mut files = vec![
        (
            PathBuf::from(MANIFEST_FILE),
            Manifest::new(&context.name).to_toml(),
        ),
        (PathBuf::from(".gitignore"), GITIGNORE.to_owned()),
        (PathBuf::from("assets/.gitkeep"), String::new()),
    ];
    let templates = COMMON_TEMPLATES.iter().chain(
        context
            .firmware
            .then_some(FIRMWARE_TEMPLATES)
            .into_iter()
            .flatten(),
    );
    files
        .extend(templates.map(|(path, template)| (PathBuf::from(path), render(template, context))));
    files
}

fn ensure_empty_or_missing(path: &Path) -> Result<()> {
    match fs::read_dir(path) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                Err(
                    Error::new(format!("{}가 비어 있지 않습니다", path.display()))
                        .with_hint("새 디렉토리 이름을 지정하세요"),
                )
            } else {
                Ok(())
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) if path.is_file() => Err(Error::new(format!(
            "{}는 파일입니다: {err}",
            path.display()
        ))),
        Err(err) => Err(Error::io("읽기", path, err)),
    }
}

/// 새 게임 프로젝트를 만든다.
pub fn create(options: &NewOptions) -> Result<Created> {
    let root = std::path::absolute(&options.path)
        .map_err(|err| Error::io("경로 해석", &options.path, err))?;
    let name = match &options.name {
        Some(name) => name.clone(),
        None => root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| {
                Error::new(format!(
                    "{}에서 프로젝트 이름을 정할 수 없습니다",
                    root.display()
                ))
                .with_hint("--name으로 지정하세요")
            })?,
    };
    validate_name(&name)?;
    ensure_empty_or_missing(&root)?;
    let engine = options.engine.resolved()?;

    let context = TemplateContext::for_host(&name, engine, options.firmware);
    for (relative, content) in render_files(&context) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| Error::io("만들기", parent, err))?;
        }
        fs::write(&path, content).map_err(|err| Error::io("쓰기", &path, err))?;
    }

    let mut warnings = Vec::new();
    if options.firmware {
        for issue in path_issues(&root, PROJECT_PATH_MAX_CHARS) {
            warnings.push(format!(
                "{}. 시뮬레이터는 동작하지만 펌웨어 빌드는 실패할 수 있습니다",
                issue.message()
            ));
        }
    }
    if options.git_init {
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&root)
            .status();
        match status {
            Ok(status) if status.success() => {}
            Ok(_) => warnings.push("git init이 실패했습니다".to_owned()),
            Err(_) => warnings.push("git을 찾지 못해 git init을 건너뛰었습니다".to_owned()),
        }
    }

    Ok(Created {
        root,
        name,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::ROLE_PARTITION_OFFSET;

    fn context(firmware: bool) -> TemplateContext {
        TemplateContext {
            name: "my-game".to_owned(),
            engine: EngineSource::official(),
            firmware,
            firmware_target_dir: Some("C:/dbx/my-game".to_owned()),
            idf_tools_dir: WINDOWS_IDF_TOOLS_DIR.to_owned(),
        }
    }

    fn file<'a>(files: &'a [(PathBuf, String)], path: &str) -> &'a str {
        files
            .iter()
            .find(|(p, _)| p == Path::new(path))
            .map(|(_, content)| content.as_str())
            .unwrap_or_else(|| panic!("{path} 없음"))
    }

    #[test]
    fn all_placeholders_are_filled() {
        for (path, content) in render_files(&context(true)) {
            assert!(
                !content.contains("{{"),
                "{} 에 남은 자리표시자",
                path.display()
            );
        }
    }

    #[test]
    fn firmware_files_are_optional() {
        let with = render_files(&context(true));
        let without = render_files(&context(false));
        assert!(with.iter().any(|(p, _)| p.starts_with("firmware")));
        assert!(!without.iter().any(|(p, _)| p.starts_with("firmware")));
        assert_eq!(with.len() - without.len(), FIRMWARE_TEMPLATES.len());
    }

    #[test]
    fn generated_toml_files_are_valid() {
        for (path, content) in render_files(&context(true)) {
            if path.extension().is_some_and(|e| e == "toml") {
                content
                    .parse::<toml::Table>()
                    .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            }
        }
    }

    #[test]
    fn manifest_uses_project_name() {
        let files = render_files(&context(false));
        let manifest = Manifest::parse(file(&files, MANIFEST_FILE)).unwrap();
        assert_eq!(manifest.project.name, "my-game");
    }

    #[test]
    fn rust_sources_use_crate_ident() {
        let files = render_files(&context(true));
        assert!(file(&files, "sim/src/main.rs").contains("my_game::MyGame::new"));
        assert!(file(&files, "firmware/src/main.rs").contains("my_game::MyGame::new"));
        assert!(file(&files, "game/src/lib.rs").contains("pub struct MyGame;"));
    }

    #[test]
    fn engine_git_dependency() {
        let files = render_files(&context(false));
        let root: toml::Table = file(&files, "Cargo.toml").parse().unwrap();
        let deps = &root["workspace"]["dependencies"];
        assert_eq!(
            deps["dotbox-core"]["git"].as_str(),
            Some(OFFICIAL_ENGINE_GIT)
        );
        assert!(deps["dotbox-core"].get("rev").is_none());
        assert_eq!(deps["my-game"]["path"].as_str(), Some("game"));
    }

    #[test]
    fn engine_git_dependency_with_rev() {
        let engine = EngineSource::Git {
            url: "https://example.com/e.git".to_owned(),
            rev: Some("v0.1.0".to_owned()),
        };
        let dep: toml::Table = format!("d = {}", engine.dependency("crates/dotbox-core"))
            .parse()
            .unwrap();
        assert_eq!(dep["d"]["rev"].as_str(), Some("v0.1.0"));
    }

    #[test]
    fn engine_path_dependency_uses_forward_slashes() {
        let engine = EngineSource::Path(PathBuf::from(r"E:\DotBoxEngine"));
        let dep: toml::Table = format!("d = {}", engine.dependency("platform/dotbox-esp"))
            .parse()
            .unwrap();
        assert_eq!(
            dep["d"]["path"].as_str(),
            Some("E:/DotBoxEngine/platform/dotbox-esp")
        );
    }

    #[test]
    fn target_dir_is_written_only_when_set() {
        let mut ctx = context(true);
        let files = render_files(&ctx);
        let config: toml::Table = file(&files, "firmware/.cargo/config.toml").parse().unwrap();
        assert_eq!(
            config["build"]["target-dir"].as_str(),
            Some("C:/dbx/my-game")
        );
        assert_eq!(
            config["build"]["target"].as_str(),
            Some("xtensa-esp32-espidf")
        );

        ctx.firmware_target_dir = None;
        let files = render_files(&ctx);
        let config: toml::Table = file(&files, "firmware/.cargo/config.toml").parse().unwrap();
        assert!(config["build"].get("target-dir").is_none());
    }

    #[test]
    fn role_partition_offset_matches_device_constant() {
        let files = render_files(&context(true));
        let row = file(&files, "firmware/partitions.csv")
            .lines()
            .find(|line| line.starts_with("dotbox,"))
            .expect("dotbox 파티션 없음");
        let offset = row.split(',').map(str::trim).nth(3).unwrap();
        assert_eq!(offset, format!("{ROLE_PARTITION_OFFSET:#x}"));
    }

    #[test]
    fn host_context_matches_platform() {
        let ctx = TemplateContext::for_host("snake", EngineSource::official(), true);
        assert_eq!(ctx.firmware_target_dir.is_some(), cfg!(windows));
    }

    fn engine_repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn create_writes_project() {
        let dir = tempfile::tempdir().unwrap();
        let created = create(&NewOptions {
            path: dir.path().join("snake"),
            name: None,
            engine: EngineSource::Path(engine_repo()),
            firmware: true,
            git_init: false,
        })
        .unwrap();

        assert_eq!(created.name, "snake");
        for path in [
            MANIFEST_FILE,
            ".gitignore",
            "assets/.gitkeep",
            "game/src/lib.rs",
            "sim/src/main.rs",
            "firmware/partitions.csv",
            "firmware/.cargo/config.toml",
        ] {
            assert!(created.root.join(path).is_file(), "{path}");
        }
        crate::manifest::Project::load(&created.root).unwrap();
    }

    #[test]
    fn create_uses_explicit_name() {
        let dir = tempfile::tempdir().unwrap();
        let created = create(&NewOptions {
            path: dir.path().join("Some Folder"),
            name: Some("pong".to_owned()),
            engine: EngineSource::official(),
            firmware: false,
            git_init: false,
        })
        .unwrap();
        assert_eq!(created.name, "pong");
        // 경로 검사는 펌웨어가 있을 때만 경고한다.
        assert!(created.warnings.is_empty());
    }

    #[test]
    fn create_warns_about_bad_path_with_firmware() {
        let dir = tempfile::tempdir().unwrap();
        let created = create(&NewOptions {
            path: dir.path().join("my folder"),
            name: Some("pong".to_owned()),
            engine: EngineSource::official(),
            firmware: true,
            git_init: false,
        })
        .unwrap();
        assert!(created.warnings.iter().any(|w| w.contains("공백")));
    }

    #[test]
    fn create_rejects_non_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("existing.txt"), "x").unwrap();
        let err = create(&NewOptions {
            path: dir.path().to_path_buf(),
            name: Some("snake".to_owned()),
            engine: EngineSource::official(),
            firmware: false,
            git_init: false,
        })
        .unwrap_err();
        assert!(err.message().contains("비어 있지 않습니다"));
    }

    #[test]
    fn create_accepts_existing_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("snake");
        fs::create_dir(&target).unwrap();
        let created = create(&NewOptions {
            path: target,
            name: None,
            engine: EngineSource::official(),
            firmware: false,
            git_init: false,
        })
        .unwrap();
        assert!(created.root.join(MANIFEST_FILE).is_file());
    }

    #[test]
    fn create_rejects_invalid_name_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Snake");
        assert!(
            create(&NewOptions {
                path: target.clone(),
                name: None,
                engine: EngineSource::official(),
                firmware: false,
                git_init: false,
            })
            .is_err()
        );
        assert!(!target.exists());
    }

    #[test]
    fn create_rejects_non_engine_path() {
        let dir = tempfile::tempdir().unwrap();
        let err = create(&NewOptions {
            path: dir.path().join("snake"),
            name: None,
            engine: EngineSource::Path(dir.path().to_path_buf()),
            firmware: false,
            git_init: false,
        })
        .unwrap_err();
        assert!(err.message().contains("엔진 저장소가 아닙니다"));
        assert!(!dir.path().join("snake").exists());
    }
}
