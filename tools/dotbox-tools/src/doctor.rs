//! 개발 환경 점검(`dotbox doctor`). 검사 항목은 `docs/cli.md` 6.2.
//!
//! 판정은 문자열이나 값을 받는 순수 함수로 나눠 테스트하고, 외부 명령 실행은 [`run`]에만 둔다.

use std::path::Path;

use serde::Serialize;

use crate::device::{self, SerialPortInfo};
use crate::manifest::Project;
use crate::paths::{PROJECT_PATH_MAX_CHARS, PathIssue, TARGET_DIR_MAX_CHARS, path_issues};
use crate::process;

/// 엔진이 요구하는 최소 Rust 버전(edition 2024).
pub const MIN_RUST_VERSION: (u32, u32, u32) = (1, 85, 0);

/// 검사 결과 수준.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// 검사 항목 하나의 결과.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub id: &'static str,
    pub level: Level,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl Check {
    fn new(id: &'static str, level: Level, message: impl Into<String>) -> Self {
        Self {
            id,
            level,
            message: message.into(),
            hint: None,
        }
    }

    fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// 점검 결과 전체.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    pub fn has_errors(&self) -> bool {
        self.checks.iter().any(|c| c.level == Level::Error)
    }

    pub fn count(&self, level: Level) -> usize {
        self.checks.iter().filter(|c| c.level == level).count()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report is always serializable")
    }
}

/// 점검 옵션.
#[derive(Debug, Clone, Copy, Default)]
pub struct DoctorOptions<'a> {
    /// 점검할 프로젝트. 없으면 프로젝트 항목을 건너뛴다.
    pub project: Option<&'a Project>,
    /// 시뮬레이터 개발에 필요한 항목만 점검한다.
    pub host_only: bool,
}

/// `rustc --version` 출력에서 버전을 읽는다. 예: `rustc 1.97.1 (8bab26f4f 2026-07-14)`.
pub fn parse_rustc_version(output: &str) -> Option<(u32, u32, u32)> {
    let version = output.split_whitespace().nth(1)?;
    let version = version.split('-').next()?; // 1.86.0-nightly
    let mut parts = version.split('.').map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

pub fn check_rust_version(rustc_output: Option<&str>) -> Check {
    const ID: &str = "rust.version";
    let (major, minor, patch) = MIN_RUST_VERSION;
    let install_hint = "https://rustup.rs 에서 Rust를 설치하고 rustup update stable을 실행하세요";
    let Some(output) = rustc_output else {
        return Check::new(ID, Level::Error, "rustc를 실행하지 못했습니다").hint(install_hint);
    };
    match parse_rustc_version(output) {
        Some(version) if version >= MIN_RUST_VERSION => {
            Check::new(ID, Level::Ok, output.trim().to_owned())
        }
        Some(_) => Check::new(
            ID,
            Level::Error,
            format!("{} (필요: {major}.{minor}.{patch} 이상)", output.trim()),
        )
        .hint("rustup update stable"),
        None => Check::new(
            ID,
            Level::Error,
            format!("rustc 버전을 읽지 못했습니다: {}", output.trim()),
        )
        .hint(install_hint),
    }
}

/// `rustup toolchain list` 출력에서 esp 툴체인을 찾는다.
pub fn check_esp_toolchain(rustup_output: Option<&str>) -> Check {
    const ID: &str = "esp.toolchain";
    let hint = "cargo install espup && espup install";
    let Some(output) = rustup_output else {
        return Check::new(ID, Level::Error, "rustup을 실행하지 못했습니다")
            .hint("https://rustup.rs 에서 rustup을 설치하세요");
    };
    let found = output
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .any(|name| name == "esp" || name.starts_with("esp-"));
    if found {
        Check::new(ID, Level::Ok, "esp 툴체인이 설치되어 있습니다")
    } else {
        Check::new(ID, Level::Error, "esp 툴체인이 없습니다").hint(hint)
    }
}

pub fn check_program(id: &'static str, name: &str, found: Option<&Path>, hint: &str) -> Check {
    match found {
        Some(path) => Check::new(id, Level::Ok, format!("{name}: {}", path.display())),
        None => Check::new(id, Level::Error, format!("{name}이(가) PATH에 없습니다")).hint(hint),
    }
}

pub fn check_serial_ports(ports: &[SerialPortInfo]) -> Check {
    const ID: &str = "serial.ports";
    if ports.is_empty() {
        return Check::new(ID, Level::Info, "연결된 USB 시리얼 포트가 없습니다")
            .hint("모듈을 USB로 연결하고 드라이버(CP210x, CH340)를 확인하세요");
    }
    let list: Vec<String> = ports.iter().map(SerialPortInfo::label).collect();
    Check::new(
        ID,
        Level::Info,
        format!("USB 시리얼 포트: {}", list.join(", ")),
    )
}

pub fn check_project_found(project: Option<&Project>) -> Check {
    match project {
        Some(project) => Check::new(
            "project",
            Level::Info,
            format!(
                "프로젝트 '{}': {}",
                project.name(),
                project.root().display()
            ),
        ),
        None => Check::new(
            "project",
            Level::Info,
            "프로젝트 밖에서 실행해 프로젝트 항목을 건너뜁니다",
        ),
    }
}

/// 경로 검사 규칙: (검사 ID, 해당 문제인지, 실패 수준, 통과 메시지).
type PathRule = (&'static str, fn(&PathIssue) -> bool, Level, &'static str);

/// 프로젝트 경로 검사. 규칙마다 결과를 하나씩 낸다.
pub fn check_project_path(root: &Path) -> Vec<Check> {
    let issues = path_issues(root, PROJECT_PATH_MAX_CHARS);
    let rules: [PathRule; 4] = [
        (
            "path.ascii",
            |i| *i == PathIssue::NonAscii,
            Level::Error,
            "경로에 ASCII만 있습니다",
        ),
        (
            "path.spaces",
            |i| *i == PathIssue::Spaces,
            Level::Error,
            "경로에 공백이 없습니다",
        ),
        (
            "path.onedrive",
            |i| *i == PathIssue::OneDrive,
            Level::Error,
            "OneDrive 밖에 있습니다",
        ),
        (
            "path.length",
            |i| matches!(i, PathIssue::TooLong { .. }),
            Level::Warn,
            "경로 길이가 적당합니다",
        ),
    ];
    rules
        .into_iter()
        .map(|(id, matches, level, ok_message)| {
            match issues.iter().find(|issue| matches(issue)) {
                Some(issue) => Check::new(id, level, issue.message()).hint(
                    "C:\\dev\\game처럼 짧은 ASCII 경로로 프로젝트를 옮기세요. 시뮬레이터는 상관없지만 펌웨어 빌드가 실패할 수 있습니다",
                ),
                None => Check::new(id, Level::Ok, ok_message),
            }
        })
        .collect()
}

/// `firmware/.cargo/config.toml`의 `[build] target-dir` 검사.
/// `config`가 `None`이면 파일이 없는 것이다. `windows`는 호스트가 Windows인지.
pub fn check_target_dir(config: Option<&str>, windows: bool) -> Check {
    const ID: &str = "firmware.target_dir";
    let hint =
        "firmware/.cargo/config.toml의 [build]에 target-dir = \"C:/dbx/<게임 이름>\"을 적으세요";
    let table = match config.map(str::parse::<toml::Table>) {
        None => {
            return Check::new(ID, Level::Error, "firmware/.cargo/config.toml이 없습니다")
                .hint(hint);
        }
        Some(Err(err)) => {
            return Check::new(ID, Level::Error, format!("config.toml 형식 오류: {err}"));
        }
        Some(Ok(table)) => table,
    };
    let target_dir = table
        .get("build")
        .and_then(|build| build.get("target-dir"))
        .and_then(|value| value.as_str());
    let Some(target_dir) = target_dir else {
        return if windows {
            Check::new(
                ID,
                Level::Error,
                "펌웨어 target-dir이 설정되어 있지 않습니다",
            )
            .hint(hint)
        } else {
            Check::new(
                ID,
                Level::Ok,
                "target-dir 미설정(Windows가 아니라 기본값 사용)",
            )
        };
    };
    let issues = path_issues(Path::new(target_dir), TARGET_DIR_MAX_CHARS);
    if let Some(issue) = issues.iter().find(|i| i.is_fatal()) {
        Check::new(
            ID,
            Level::Error,
            format!("target-dir {target_dir}: {}", issue.message()),
        )
        .hint(hint)
    } else if let Some(issue) = issues.first() {
        Check::new(
            ID,
            Level::Warn,
            format!("target-dir {target_dir}: {}", issue.message()),
        )
        .hint(hint)
    } else {
        Check::new(ID, Level::Ok, format!("target-dir {target_dir}"))
    }
}

/// `Cargo.lock`의 `dotbox-core` 버전과 CLI 버전 비교. `lock`이 `None`이면 파일이 없는 것이다.
pub fn check_engine_version(lock: Option<&str>, tool_version: &str) -> Check {
    const ID: &str = "engine.version";
    let Some(lock) = lock else {
        return Check::new(
            ID,
            Level::Info,
            "Cargo.lock이 없어 엔진 버전을 확인하지 못했습니다",
        )
        .hint("dotbox run으로 한 번 빌드한 뒤 다시 확인하세요");
    };
    let table = match lock.parse::<toml::Table>() {
        Ok(table) => table,
        Err(err) => return Check::new(ID, Level::Warn, format!("Cargo.lock 형식 오류: {err}")),
    };
    let engine_version = table
        .get("package")
        .and_then(|p| p.as_array())
        .into_iter()
        .flatten()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("dotbox-core"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str());
    match engine_version {
        None => Check::new(ID, Level::Warn, "Cargo.lock에 dotbox-core가 없습니다"),
        Some(version) if version == tool_version => {
            Check::new(ID, Level::Ok, format!("엔진 {version}, CLI {tool_version}"))
        }
        Some(version) => Check::new(
            ID,
            Level::Warn,
            format!("엔진 {version}와 CLI {tool_version}의 버전이 다릅니다"),
        )
        .hint("CLI를 엔진 버전에 맞춰 다시 설치하거나 게임의 엔진 의존성을 갱신하세요"),
    }
}

/// 환경을 실제로 조사해 점검한다.
pub fn run(options: DoctorOptions<'_>) -> Report {
    let mut checks = Vec::new();

    let rustc = process::probe_stdout("rustc", &["--version"]);
    checks.push(check_rust_version(rustc.as_deref()));

    if !options.host_only {
        let rustup = process::probe_stdout("rustup", &["toolchain", "list"]);
        checks.push(check_esp_toolchain(rustup.as_deref()));
        checks.push(check_program(
            "esp.espflash",
            "espflash",
            process::find_program("espflash").as_deref(),
            "cargo install espflash",
        ));
        checks.push(check_program(
            "esp.ldproxy",
            "ldproxy",
            process::find_program("ldproxy").as_deref(),
            "cargo install ldproxy",
        ));
        checks.push(check_serial_ports(&device::detect_ports()));
    }

    checks.push(check_project_found(options.project));
    if let Some(project) = options.project {
        let lock = std::fs::read_to_string(project.root().join("Cargo.lock")).ok();
        checks.push(check_engine_version(
            lock.as_deref(),
            env!("CARGO_PKG_VERSION"),
        ));
        if !options.host_only {
            checks.extend(check_project_path(project.root()));
            if project.has_firmware() {
                let config_path = project.firmware_dir().join(".cargo/config.toml");
                let config = std::fs::read_to_string(config_path).ok();
                checks.push(check_target_dir(config.as_deref(), cfg!(windows)));
            }
        }
    }

    Report { checks }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rustc_versions() {
        assert_eq!(
            parse_rustc_version("rustc 1.97.1 (8bab26f4f 2026-07-14)"),
            Some((1, 97, 1))
        );
        assert_eq!(
            parse_rustc_version("rustc 1.86.0-nightly (abc 2025-01-01)"),
            Some((1, 86, 0))
        );
        assert_eq!(parse_rustc_version("rustc"), None);
        assert_eq!(parse_rustc_version("rustc x.y.z"), None);
    }

    #[test]
    fn rust_version_levels() {
        assert_eq!(
            check_rust_version(Some("rustc 1.85.0 (x)")).level,
            Level::Ok
        );
        assert_eq!(
            check_rust_version(Some("rustc 1.84.1 (x)")).level,
            Level::Error
        );
        assert_eq!(check_rust_version(Some("garbage")).level, Level::Error);
        let missing = check_rust_version(None);
        assert_eq!(missing.level, Level::Error);
        assert!(missing.hint.is_some());
    }

    #[test]
    fn esp_toolchain_detection() {
        let with = "stable-x86_64-pc-windows-msvc (default)\nesp\n";
        let without = "stable-x86_64-pc-windows-msvc (default)\nnightly-x86_64\n";
        assert_eq!(check_esp_toolchain(Some(with)).level, Level::Ok);
        assert_eq!(check_esp_toolchain(Some(without)).level, Level::Error);
        assert_eq!(check_esp_toolchain(Some("")).level, Level::Error);
        assert_eq!(check_esp_toolchain(None).level, Level::Error);
        // 이름에 esp가 들어간 다른 툴체인을 잘못 잡지 않는다.
        assert_eq!(check_esp_toolchain(Some("respect\n")).level, Level::Error);
    }

    #[test]
    fn program_check() {
        let found = check_program(
            "esp.espflash",
            "espflash",
            Some(Path::new("C:/x/espflash.exe")),
            "h",
        );
        assert_eq!(found.level, Level::Ok);
        let missing = check_program("esp.espflash", "espflash", None, "cargo install espflash");
        assert_eq!(missing.level, Level::Error);
        assert_eq!(missing.hint.as_deref(), Some("cargo install espflash"));
    }

    #[test]
    fn project_path_produces_one_check_per_rule() {
        let checks = check_project_path(Path::new("C:/dev/snake"));
        let ids: Vec<_> = checks.iter().map(|c| c.id).collect();
        assert_eq!(
            ids,
            ["path.ascii", "path.spaces", "path.onedrive", "path.length"]
        );
        assert!(checks.iter().all(|c| c.level == Level::Ok));
    }

    #[test]
    fn project_path_problems() {
        let checks = check_project_path(Path::new("C:/Users/홍길동/OneDrive/내 게임"));
        let level = |id| checks.iter().find(|c| c.id == id).unwrap().level;
        assert_eq!(level("path.ascii"), Level::Error);
        assert_eq!(level("path.spaces"), Level::Error);
        assert_eq!(level("path.onedrive"), Level::Error);
        assert_eq!(level("path.length"), Level::Ok);

        let long = format!("C:/{}", "a".repeat(PROJECT_PATH_MAX_CHARS));
        let checks = check_project_path(Path::new(&long));
        assert_eq!(
            checks.iter().find(|c| c.id == "path.length").unwrap().level,
            Level::Warn
        );
    }

    #[test]
    fn target_dir_checks() {
        let config = |dir: &str| format!("[build]\ntarget = \"x\"\ntarget-dir = \"{dir}\"\n");
        assert_eq!(
            check_target_dir(Some(&config("C:/dbx/snake")), true).level,
            Level::Ok
        );
        assert_eq!(
            check_target_dir(Some(&config("C:/dbx/a-very-long-game-name-here")), true).level,
            Level::Warn
        );
        assert_eq!(
            check_target_dir(Some(&config("C:/My dbx/a")), true).level,
            Level::Error
        );
        assert_eq!(
            check_target_dir(Some(&config("C:/OneDrive/a")), true).level,
            Level::Error
        );

        let no_dir = "[build]\ntarget = \"x\"\n";
        assert_eq!(check_target_dir(Some(no_dir), true).level, Level::Error);
        assert_eq!(check_target_dir(Some(no_dir), false).level, Level::Ok);
        assert_eq!(check_target_dir(None, true).level, Level::Error);
        assert_eq!(check_target_dir(Some("[build"), true).level, Level::Error);
    }

    #[test]
    fn engine_version_checks() {
        let lock = |version: &str| {
            format!(
                "version = 4\n\n[[package]]\nname = \"dotbox-core\"\nversion = \"{version}\"\n\n[[package]]\nname = \"snake\"\nversion = \"0.1.0\"\n"
            )
        };
        assert_eq!(
            check_engine_version(Some(&lock("0.1.0")), "0.1.0").level,
            Level::Ok
        );
        assert_eq!(
            check_engine_version(Some(&lock("0.2.0")), "0.1.0").level,
            Level::Warn
        );
        assert_eq!(check_engine_version(None, "0.1.0").level, Level::Info);
        assert_eq!(
            check_engine_version(Some("version = 4\n"), "0.1.0").level,
            Level::Warn
        );
        assert_eq!(
            check_engine_version(Some("[[[bad"), "0.1.0").level,
            Level::Warn
        );
    }

    #[test]
    fn serial_ports_are_informational() {
        assert_eq!(check_serial_ports(&[]).level, Level::Info);
        let ports = [SerialPortInfo {
            name: "COM5".to_owned(),
            description: Some("CP2102".to_owned()),
        }];
        let check = check_serial_ports(&ports);
        assert_eq!(check.level, Level::Info);
        assert!(check.message.contains("COM5"));
    }

    #[test]
    fn report_summary_and_json() {
        let report = Report {
            checks: vec![
                Check::new("a", Level::Ok, "fine"),
                Check::new("b", Level::Error, "broken").hint("fix it"),
            ],
        };
        assert!(report.has_errors());
        assert_eq!(report.count(Level::Ok), 1);

        let json: serde_json::Value = serde_json::from_str(&report.to_json()).unwrap();
        assert_eq!(json["checks"][0]["level"], "ok");
        assert!(json["checks"][0].get("hint").is_none());
        assert_eq!(json["checks"][1]["hint"], "fix it");
    }

    #[test]
    fn host_only_run_skips_esp_checks() {
        let report = run(DoctorOptions {
            project: None,
            host_only: true,
        });
        let ids: Vec<_> = report.checks.iter().map(|c| c.id).collect();
        assert_eq!(ids, ["rust.version", "project"]);
        // 이 테스트를 돌리는 환경에는 Rust가 있다.
        assert_eq!(report.checks[0].level, Level::Ok);
    }
}
