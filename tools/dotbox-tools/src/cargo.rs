//! 시뮬레이터 실행(`dotbox run`)과 펌웨어 빌드(`dotbox build`).

use std::path::PathBuf;
use std::process::Command;

use crate::doctor::{self, Level};
use crate::manifest::Project;
use crate::process;
use crate::{Error, Result};

/// `cargo run --manifest-path sim/Cargo.toml [--release] [-- args]`.
pub fn sim_run_command(project: &Project, release: bool, args: &[String]) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(project.root())
        .arg("run")
        .arg("--manifest-path")
        .arg(project.sim_dir().join("Cargo.toml"));
    if release {
        command.arg("--release");
    }
    if !args.is_empty() {
        command.arg("--").args(args);
    }
    command
}

/// 시뮬레이터로 게임을 실행한다.
pub fn run_sim(project: &Project, release: bool, args: &[String], verbose: bool) -> Result<()> {
    process::run(&mut sim_run_command(project, release, args), verbose)
}

/// 펌웨어 디렉토리에서 `cargo build [--release] --message-format=json-render-diagnostics`.
/// 작업 디렉토리가 `firmware/`라서 `rust-toolchain.toml`과 `.cargo/config.toml`이 적용된다.
pub fn firmware_build_command(project: &Project, release: bool) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(project.firmware_dir())
        .arg("build")
        .arg("--message-format=json-render-diagnostics");
    if release {
        command.arg("--release");
    }
    command
}

/// cargo JSON 메시지에서 `bin_name` 바이너리의 실행 파일 경로를 찾는다.
pub fn find_executable(cargo_stdout: &str, bin_name: &str) -> Option<PathBuf> {
    cargo_stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|msg| msg["reason"] == "compiler-artifact" && msg["target"]["name"] == bin_name)
        .filter_map(|msg| msg["executable"].as_str().map(PathBuf::from))
        .next_back()
}

/// 펌웨어 빌드 전 점검. ESP-IDF 빌드를 확실히 깨뜨리는 문제가 있으면 오류.
pub fn precheck_firmware(project: &Project) -> Result<()> {
    if !project.has_firmware() {
        return Err(Error::new(format!(
            "{}에 펌웨어 crate가 없습니다",
            project.firmware_dir().display()
        ))
        .with_hint(
            "dotbox new에서 --no-firmware 없이 만든 프로젝트에서만 펌웨어를 빌드할 수 있습니다",
        ));
    }
    let config = std::fs::read_to_string(project.firmware_dir().join(".cargo/config.toml")).ok();
    let checks = doctor::check_project_path(project.root())
        .into_iter()
        .chain([doctor::check_target_dir(config.as_deref(), cfg!(windows))]);
    let errors: Vec<String> = checks
        .filter(|check| check.level == Level::Error)
        .map(|check| format!("[{}] {}", check.id, check.message))
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::new(format!(
            "펌웨어를 빌드할 수 없는 환경입니다: {}",
            errors.join("; ")
        ))
        .with_hint("dotbox doctor로 자세한 내용을 확인하세요"))
    }
}

/// 펌웨어를 빌드하고 ELF 경로를 돌려준다.
pub fn build_firmware(project: &Project, release: bool, verbose: bool) -> Result<PathBuf> {
    precheck_firmware(project)?;
    let stdout =
        process::run_capturing_stdout(&mut firmware_build_command(project, release), verbose)?;
    let bin_name = project.firmware_package();
    find_executable(&stdout, &bin_name).ok_or_else(|| {
        Error::new(format!(
            "빌드 결과에서 '{bin_name}' 실행 파일을 찾지 못했습니다"
        ))
        .with_hint("firmware/Cargo.toml의 [[bin]] name이 '<프로젝트 이름>-firmware'인지 확인하세요")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{MANIFEST_FILE, Manifest};
    use crate::process::describe;
    use std::fs;

    fn project(dir: &std::path::Path) -> Project {
        fs::write(dir.join(MANIFEST_FILE), Manifest::new("snake").to_toml()).unwrap();
        Project::load(dir).unwrap()
    }

    #[test]
    fn sim_command_shape() {
        let dir = tempfile::tempdir().unwrap();
        let project = project(dir.path());

        let command = sim_run_command(&project, false, &[]);
        let args: Vec<_> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(command.get_program(), "cargo");
        assert_eq!(args[0], "run");
        assert_eq!(args[1], "--manifest-path");
        assert!(args[2].ends_with("Cargo.toml"));
        assert_eq!(args.len(), 3);
        assert_eq!(command.get_current_dir(), Some(project.root()));

        let command = sim_run_command(&project, true, &["--seed".to_owned(), "7".to_owned()]);
        assert!(describe(&command).ends_with("--release -- --seed 7"));
    }

    #[test]
    fn firmware_command_runs_in_firmware_dir() {
        let dir = tempfile::tempdir().unwrap();
        let project = project(dir.path());
        let command = firmware_build_command(&project, true);
        assert_eq!(
            command.get_current_dir(),
            Some(project.firmware_dir().as_path())
        );
        assert_eq!(
            describe(&command),
            "cargo build --message-format=json-render-diagnostics --release"
        );
        assert!(!describe(&firmware_build_command(&project, false)).contains("--release"));
    }

    #[test]
    fn finds_executable_in_cargo_messages() {
        let stdout = [
            r#"{"reason":"compiler-artifact","target":{"name":"snake","kind":["lib"]},"executable":null}"#,
            "not json",
            r#"{"reason":"compiler-message","message":{}}"#,
            r#"{"reason":"compiler-artifact","target":{"name":"snake-firmware","kind":["bin"]},"executable":"C:/dbx/snake/xtensa-esp32-espidf/release/snake-firmware"}"#,
            r#"{"reason":"build-finished","success":true}"#,
        ]
        .join("\n");
        assert_eq!(
            find_executable(&stdout, "snake-firmware"),
            Some(PathBuf::from(
                "C:/dbx/snake/xtensa-esp32-espidf/release/snake-firmware"
            ))
        );
        assert_eq!(find_executable(&stdout, "other"), None);
        assert_eq!(find_executable("", "snake-firmware"), None);
    }

    #[test]
    fn precheck_requires_firmware_crate() {
        let dir = tempfile::tempdir().unwrap();
        let err = precheck_firmware(&project(dir.path())).unwrap_err();
        assert!(err.message().contains("펌웨어 crate가 없습니다"));
    }

    #[test]
    fn precheck_rejects_missing_target_dir_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let project = project(dir.path());
        let firmware = project.firmware_dir();
        fs::create_dir_all(firmware.join(".cargo")).unwrap();
        fs::write(firmware.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        fs::write(
            firmware.join(".cargo/config.toml"),
            "[build]\ntarget = \"x\"\n",
        )
        .unwrap();

        let result = precheck_firmware(&project);
        if cfg!(windows) {
            assert!(
                result
                    .unwrap_err()
                    .message()
                    .contains("firmware.target_dir")
            );
        }
    }
}
