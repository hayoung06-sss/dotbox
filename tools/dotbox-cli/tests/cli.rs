//! `dotbox` 바이너리를 실제로 실행하는 통합 테스트.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn dotbox() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dotbox"))
}

fn engine_repo() -> PathBuf {
    std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// 로컬 엔진을 쓰는 프로젝트를 만든다.
fn new_project(dir: &Path, name: &str, extra: &[&str]) -> PathBuf {
    let output = dotbox()
        .current_dir(dir)
        .args(["new", name, "--no-git", "--engine-path"])
        .arg(engine_repo())
        .args(extra)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    PathBuf::from(stdout(&output).trim())
}

#[test]
fn help_lists_commands() {
    let output = dotbox().arg("--help").output().unwrap();
    assert!(output.status.success());
    let help = stdout(&output);
    for command in [
        "new", "doctor", "run", "build", "flash", "monitor", "studio",
    ] {
        assert!(help.contains(command), "{command}");
    }
}

#[test]
fn version_matches_workspace() {
    let output = dotbox().arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        stdout(&output).trim(),
        format!("dotbox {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unknown_argument_is_usage_error() {
    let output = dotbox().args(["doctor", "--nope"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn flash_port_option_combinations_are_usage_errors() {
    for args in [
        &["flash", "--side", "both", "-p", "COM5"][..],
        &["flash", "--side", "left", "--left-port", "COM5"][..],
        &["flash", "--side", "both", "--monitor"][..],
    ] {
        let output = dotbox().args(args).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn project_commands_fail_outside_project() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        &["run"][..],
        &["build"][..],
        &["monitor"][..],
        &["flash", "--side", "left"][..],
    ] {
        let output = dotbox()
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        let err = stderr(&output);
        assert!(
            err.contains("오류:") && err.contains("dotbox.toml"),
            "{err}"
        );
        assert!(err.contains("도움말:"), "{err}");
    }
}

#[test]
fn new_rejects_invalid_name() {
    let dir = tempfile::tempdir().unwrap();
    let output = dotbox()
        .current_dir(dir.path())
        .args(["new", "My Game", "--no-git"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("프로젝트 이름"));
    assert!(!dir.path().join("My Game").exists());
}

#[test]
fn new_rejects_non_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("snake")).unwrap();
    std::fs::write(dir.path().join("snake/keep.txt"), "x").unwrap();
    let output = dotbox()
        .current_dir(dir.path())
        .args(["new", "snake", "--no-git"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn new_respects_project_base_option() {
    let dir = tempfile::tempdir().unwrap();
    let output = dotbox()
        .args(["-C"])
        .arg(dir.path())
        .args(["new", "pong", "--no-git", "--no-firmware", "-q"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(dir.path().join("pong/dotbox.toml").is_file());
    assert!(!dir.path().join("pong/firmware").exists());
    // -q면 안내 메시지가 없다. 결과 경로는 stdout으로 나온다.
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));
    assert!(stdout(&output).trim().ends_with("pong"));
}

#[test]
fn doctor_outside_project_host_only_passes() {
    let dir = tempfile::tempdir().unwrap();
    let output = dotbox()
        .current_dir(dir.path())
        .args(["doctor", "--host-only", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    let ids: Vec<_> = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids, ["rust.version", "project"]);
}

#[test]
fn doctor_with_explicit_non_project_fails() {
    let dir = tempfile::tempdir().unwrap();
    let output = dotbox()
        .args(["doctor", "-C"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn doctor_full_report_contains_all_project_checks() {
    let dir = tempfile::tempdir().unwrap();
    let root = new_project(dir.path(), "snake", &[]);
    let output = dotbox()
        .current_dir(root.join("game"))
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    let checks = report["checks"].as_array().unwrap();
    let ids: Vec<_> = checks.iter().map(|c| c["id"].as_str().unwrap()).collect();
    for id in [
        "rust.version",
        "esp.toolchain",
        "esp.espflash",
        "esp.ldproxy",
        "serial.ports",
        "project",
        "engine.version",
        "path.ascii",
        "path.spaces",
        "path.onedrive",
        "path.length",
        "firmware.target_dir",
    ] {
        assert!(ids.contains(&id), "{id} 없음: {ids:?}");
    }
    // 종료 코드는 error 항목 유무와 일치한다.
    let has_error = checks.iter().any(|c| c["level"] == "error");
    assert_eq!(output.status.success(), !has_error);
}

#[test]
fn studio_missing_gives_install_hint() {
    let dir = tempfile::tempdir().unwrap();
    let output = dotbox()
        .current_dir(dir.path())
        .env("PATH", dir.path())
        .arg("studio")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("cargo install"));
}

/// 끝에서 끝까지: 프로젝트를 만들고, 시뮬레이터를 빌드해 실행하고, 엔진 버전 점검을 통과한다.
#[test]
fn new_then_run_then_doctor() {
    let dir = tempfile::tempdir().unwrap();
    let root = new_project(dir.path(), "e2e-game", &[]);

    let run = dotbox()
        .current_dir(&root)
        .args(["run", "-q"])
        // 창 없이 짧게 돌리고 끝낸다(dotbox_sim::HEADLESS_ENV).
        .env("DOTBOX_SIM_HEADLESS_MS", "300")
        .output()
        .unwrap();
    assert!(run.status.success(), "{}", stderr(&run));
    // 마스터와 슬레이브 엔진이 연결까지 갔다.
    assert!(stderr(&run).contains("Connected"), "{}", stderr(&run));
    assert!(root.join("Cargo.lock").is_file());

    // 생성된 게임의 자체 테스트도 통과한다.
    let test = Command::new("cargo")
        .current_dir(&root)
        .args(["test", "--quiet"])
        .output()
        .unwrap();
    assert!(test.status.success(), "{}", stderr(&test));

    let doctor = dotbox()
        .current_dir(&root)
        .args(["doctor", "--host-only", "--json"])
        .output()
        .unwrap();
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let report: serde_json::Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    let engine = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "engine.version")
        .unwrap();
    assert_eq!(engine["level"], "ok", "{engine}");
}
