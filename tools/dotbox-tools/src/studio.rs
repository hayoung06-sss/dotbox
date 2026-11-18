//! DotBox Studio 실행(`dotbox studio`). Studio는 별도 바이너리라 CLI는 GUI 의존성이 없다.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::process;
use crate::{Error, Result};

/// Studio 실행 파일 이름.
pub const STUDIO_PROGRAM: &str = "dotbox-studio";

/// `path_var`에서 Studio를 찾아 실행할 명령을 만든다.
pub fn studio_command(dir: &Path, path_var: &OsStr) -> Result<Command> {
    let program = find(path_var)?;
    let mut command = Command::new(program);
    command.arg(dir);
    Ok(command)
}

fn find(path_var: &OsStr) -> Result<PathBuf> {
    process::find_program_in(STUDIO_PROGRAM, path_var).ok_or_else(|| {
        Error::new(format!("{STUDIO_PROGRAM}을(를) PATH에서 찾지 못했습니다")).with_hint(
            "cargo install --git https://github.com/hayoung06-sss/dotbox.git dotbox-studio",
        )
    })
}

/// Studio를 `dir`로 실행하고 끝날 때까지 기다린다.
pub fn launch(dir: &Path, verbose: bool) -> Result<()> {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    process::run(&mut studio_command(dir, &path_var)?, verbose)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn missing_studio_gives_install_hint() {
        let dir = tempfile::tempdir().unwrap();
        let path_var = std::env::join_paths([dir.path()]).unwrap();
        let err = studio_command(Path::new("."), &path_var).unwrap_err();
        assert!(err.hint().unwrap().contains("cargo install"));
    }

    #[test]
    fn passes_project_dir_to_studio() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir
            .path()
            .join(format!("{STUDIO_PROGRAM}{}", std::env::consts::EXE_SUFFIX));
        fs::write(&exe, b"").unwrap();
        let path_var = std::env::join_paths([dir.path()]).unwrap();

        let command = studio_command(Path::new("C:/games/snake"), &path_var).unwrap();
        assert_eq!(command.get_program(), exe.as_os_str());
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, [OsStr::new("C:/games/snake")]);
    }
}
