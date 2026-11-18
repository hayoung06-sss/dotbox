//! 외부 프로그램 실행 도우미.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::{Error, Result};

/// `PATH`(`path_var`)에서 실행 파일을 찾는다. Windows에서는 `.exe`를 붙여 찾는다.
pub fn find_program_in(name: &str, path_var: &OsStr) -> Option<PathBuf> {
    let file_name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(path_var)
        .map(|dir| dir.join(&file_name))
        .find(|candidate| candidate.is_file())
}

/// 현재 환경의 `PATH`에서 실행 파일을 찾는다.
pub fn find_program(name: &str) -> Option<PathBuf> {
    find_program_in(name, &std::env::var_os("PATH")?)
}

/// 명령을 사람이 읽을 수 있게 한 줄로 만든다(`-v` 출력과 테스트용).
pub fn describe(command: &Command) -> String {
    let mut parts = vec![command.get_program().to_string_lossy().into_owned()];
    parts.extend(command.get_args().map(|arg| {
        let arg = arg.to_string_lossy();
        if arg.contains(' ') || arg.is_empty() {
            format!("\"{arg}\"")
        } else {
            arg.into_owned()
        }
    }));
    parts.join(" ")
}

fn announce(command: &Command, verbose: bool) {
    if verbose {
        match command.get_current_dir() {
            Some(dir) => eprintln!("실행: {} (위치: {})", describe(command), dir.display()),
            None => eprintln!("실행: {}", describe(command)),
        }
    }
}

fn spawn_error(command: &Command, err: std::io::Error) -> Error {
    let program = command.get_program().to_string_lossy().into_owned();
    if err.kind() == std::io::ErrorKind::NotFound {
        Error::new(format!("'{program}' 명령을 찾지 못했습니다"))
            .with_hint("dotbox doctor로 개발 환경을 점검하세요")
    } else {
        Error::new(format!("'{program}' 실행 실패: {err}"))
    }
}

fn status_error(command: &Command, code: Option<i32>) -> Error {
    let code = code.map_or_else(|| "신호로 종료".to_owned(), |c| format!("종료 코드 {c}"));
    Error::new(format!("명령 실패({code}): {}", describe(command)))
}

/// 표준 입출력을 그대로 이어서 실행하고 성공 여부를 돌려준다.
pub fn run(command: &mut Command, verbose: bool) -> Result<()> {
    announce(command, verbose);
    let status = command.status().map_err(|err| spawn_error(command, err))?;
    if status.success() {
        Ok(())
    } else {
        Err(status_error(command, status.code()))
    }
}

/// stdout을 받아 오며 실행한다. stderr는 그대로 터미널에 보낸다.
pub fn run_capturing_stdout(command: &mut Command, verbose: bool) -> Result<String> {
    announce(command, verbose);
    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|err| spawn_error(command, err))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(status_error(command, output.status.code()))
    }
}

/// 조용히 실행해 출력을 받는다. 실행하지 못하면 `None`(점검용).
pub fn probe(program: &str, args: &[&str]) -> Option<Output> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()
}

/// 성공한 경우의 stdout 문자열.
pub fn probe_stdout(program: &str, args: &[&str]) -> Option<String> {
    probe(program, args)
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 경로를 TOML 문자열에 넣기 좋게 `/` 구분자로 바꾼다.
pub fn to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_program_only_in_given_path() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir
            .path()
            .join(format!("dotbox-fake{}", std::env::consts::EXE_SUFFIX));
        fs::write(&exe, b"").unwrap();
        let path_var = std::env::join_paths([dir.path()]).unwrap();

        assert_eq!(find_program_in("dotbox-fake", &path_var), Some(exe));
        assert_eq!(find_program_in("dotbox-missing", &path_var), None);
        assert_eq!(find_program_in("dotbox-fake", OsStr::new("")), None);
    }

    #[test]
    fn describe_quotes_arguments_with_spaces() {
        let mut command = Command::new("espflash");
        command.args(["flash", "--port", "COM5", "C:/a b/fw"]);
        assert_eq!(
            describe(&command),
            "espflash flash --port COM5 \"C:/a b/fw\""
        );
    }

    #[test]
    fn run_reports_missing_program() {
        let err = run(
            &mut Command::new("dotbox-definitely-missing-program"),
            false,
        )
        .unwrap_err();
        assert!(err.message().contains("찾지 못했습니다"));
        assert!(err.hint().is_some());
    }

    #[test]
    fn forward_slashes() {
        assert_eq!(
            to_forward_slashes(Path::new(r"C:\dbx\snake")),
            "C:/dbx/snake"
        );
    }
}
