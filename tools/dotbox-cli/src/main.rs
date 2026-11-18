//! `dotbox` 명령줄 도구. 인자를 해석해 `dotbox-tools`를 부르고 결과를 출력하기만 한다.
//! 명세: `docs/cli.md`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use dotbox_tools::device::{self, FlashOptions, FlashTarget};
use dotbox_tools::doctor::{self, DoctorOptions, Level};
use dotbox_tools::project::{self, EngineSource, NewOptions};
use dotbox_tools::{Project, Side, cargo, studio};

#[derive(Debug, Parser)]
#[command(name = "dotbox", version, about = "DotBox 게임 개발 도구", long_about = None)]
struct Cli {
    /// 게임 프로젝트 경로. 생략하면 현재 디렉토리에서 위로 찾는다. new에서는 기준 디렉토리
    #[arg(short = 'C', long = "project", global = true, value_name = "DIR")]
    project: Option<PathBuf>,

    /// 실행하는 외부 명령을 보여 준다
    #[arg(short, long, global = true)]
    verbose: bool,

    /// 오류와 경고 외 출력을 줄인다
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 새 게임 프로젝트를 만든다
    New {
        /// 만들 디렉토리. 마지막 부분이 프로젝트 이름이 된다
        path: PathBuf,
        /// 프로젝트 이름을 따로 지정한다
        #[arg(long)]
        name: Option<String>,
        /// 엔진 git 저장소
        #[arg(long, value_name = "URL", conflicts_with = "engine_path")]
        engine_git: Option<String>,
        /// 고정할 엔진 커밋이나 태그
        #[arg(long, value_name = "REV", conflicts_with = "engine_path")]
        engine_rev: Option<String>,
        /// git 대신 로컬 엔진 저장소를 쓴다
        #[arg(long, value_name = "DIR")]
        engine_path: Option<PathBuf>,
        /// firmware/를 만들지 않는다(시뮬레이터만)
        #[arg(long)]
        no_firmware: bool,
        /// git init을 하지 않는다
        #[arg(long)]
        no_git: bool,
    },
    /// 개발 환경을 점검한다
    Doctor {
        /// 시뮬레이터 개발에 필요한 항목만 점검한다
        #[arg(long)]
        host_only: bool,
        /// 결과를 JSON으로 출력한다
        #[arg(long)]
        json: bool,
    },
    /// 시뮬레이터로 게임을 실행한다
    Run {
        /// 릴리스 빌드로 실행한다
        #[arg(long)]
        release: bool,
        /// 게임 실행 파일에 넘길 인자
        #[arg(last = true, value_name = "ARGS")]
        args: Vec<String>,
    },
    /// 펌웨어를 빌드한다
    Build {
        /// 디버그 빌드(기본은 릴리스)
        #[arg(long)]
        debug: bool,
    },
    /// 펌웨어를 빌드해 모듈에 굽고 역할을 기록한다
    Flash {
        /// 대상 모듈
        #[arg(long, value_enum)]
        side: FlashSide,
        /// 시리얼 포트(--side left|right)
        #[arg(short, long, value_name = "PORT")]
        port: Option<String>,
        /// 왼쪽 모듈 포트(--side both)
        #[arg(long, value_name = "PORT")]
        left_port: Option<String>,
        /// 오른쪽 모듈 포트(--side both)
        #[arg(long, value_name = "PORT")]
        right_port: Option<String>,
        /// 굽고 나서 시리얼 모니터를 연다(--side left|right)
        #[arg(long)]
        monitor: bool,
        /// 디버그 빌드(기본은 릴리스)
        #[arg(long)]
        debug: bool,
    },
    /// 시리얼 로그를 본다
    Monitor {
        /// 대상 모듈
        #[arg(long, value_enum, default_value_t = OneSide::Left)]
        side: OneSide,
        /// 시리얼 포트
        #[arg(short, long, value_name = "PORT")]
        port: Option<String>,
    },
    /// DotBox Studio를 실행한다
    Studio {
        /// 열 디렉토리. 생략하면 현재 프로젝트
        dir: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum FlashSide {
    Left,
    Right,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OneSide {
    Left,
    Right,
}

impl From<OneSide> for Side {
    fn from(side: OneSide) -> Self {
        match side {
            OneSide::Left => Side::Left,
            OneSide::Right => Side::Right,
        }
    }
}

/// 출력 설정.
#[derive(Debug, Clone, Copy)]
struct Output {
    quiet: bool,
}

impl Output {
    fn info(self, message: impl AsRef<str>) {
        if !self.quiet {
            eprintln!("{}", message.as_ref());
        }
    }

    fn warn(self, message: impl AsRef<str>) {
        eprintln!("경고: {}", message.as_ref());
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("오류: {}", err.message());
            if let Some(hint) = err.hint() {
                eprintln!("도움말: {hint}");
            }
            ExitCode::FAILURE
        }
    }
}

/// 사용법 오류를 clap 형식으로 내고 종료 코드 2로 끝낸다.
fn usage_error(message: &str) -> ! {
    Cli::command()
        .error(clap::error::ErrorKind::ArgumentConflict, message)
        .exit()
}

fn current_dir() -> dotbox_tools::Result<PathBuf> {
    std::env::current_dir()
        .map_err(|err| dotbox_tools::Error::new(format!("현재 디렉토리를 알 수 없습니다: {err}")))
}

/// `-C`나 현재 디렉토리에서 프로젝트를 찾는다.
fn require_project(cli_project: Option<&Path>) -> dotbox_tools::Result<Project> {
    match cli_project {
        Some(dir) => Project::discover(dir),
        None => Project::discover(&current_dir()?),
    }
}

fn run(cli: Cli) -> dotbox_tools::Result<ExitCode> {
    let out = Output { quiet: cli.quiet };
    let verbose = cli.verbose;
    match cli.command {
        Command::New {
            path,
            name,
            engine_git,
            engine_rev,
            engine_path,
            no_firmware,
            no_git,
        } => {
            let path = match &cli.project {
                Some(base) => base.join(path),
                None => path,
            };
            let engine = match engine_path {
                Some(dir) => EngineSource::Path(dir),
                None => EngineSource::Git {
                    url: engine_git.unwrap_or_else(|| project::OFFICIAL_ENGINE_GIT.to_owned()),
                    rev: engine_rev,
                },
            };
            let created = project::create(&NewOptions {
                path,
                name,
                engine,
                firmware: !no_firmware,
                git_init: !no_git,
            })?;
            for warning in &created.warnings {
                out.warn(warning);
            }
            out.info(format!("프로젝트 '{}'를 만들었습니다.", created.name));
            out.info(format!(
                "다음 단계:\n  cd {}\n  dotbox doctor\n  dotbox run",
                created.root.display()
            ));
            println!("{}", created.root.display());
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor { host_only, json } => {
            let project = match &cli.project {
                Some(dir) => Some(Project::discover(dir)?),
                None => Project::find_root(&current_dir()?)
                    .map(|root| Project::load(&root))
                    .transpose()?,
            };
            let report = doctor::run(DoctorOptions {
                project: project.as_ref(),
                host_only,
            });
            if json {
                println!("{}", report.to_json());
            } else {
                print_report(&report, out);
            }
            Ok(if report.has_errors() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
        Command::Run { release, args } => {
            let project = require_project(cli.project.as_deref())?;
            cargo::run_sim(&project, release, &args, verbose)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Build { debug } => {
            let project = require_project(cli.project.as_deref())?;
            let elf = cargo::build_firmware(&project, !debug, verbose)?;
            let bytes =
                std::fs::read(&elf).map_err(|err| dotbox_tools::Error::io("읽기", &elf, err))?;
            out.info(format!("build_id: {}", device::build_id(&bytes)));
            println!("{}", elf.display());
            Ok(ExitCode::SUCCESS)
        }
        Command::Flash {
            side,
            port,
            left_port,
            right_port,
            monitor,
            debug,
        } => {
            let targets = flash_targets(side, port, left_port, right_port);
            if monitor && side == FlashSide::Both {
                usage_error(
                    "--monitor는 --side left|right에서만 쓸 수 있습니다(양쪽 동시 모니터는 아직 없음)",
                );
            }
            let project = require_project(cli.project.as_deref())?;
            let report = device::flash(
                &project,
                &FlashOptions {
                    targets,
                    release: !debug,
                    monitor,
                    verbose,
                },
            )?;
            for (side, port) in &report.flashed {
                out.info(format!("{} 모듈({port}) 완료", device::side_label(*side)));
            }
            out.info(format!("build_id: {}", report.build_id));
            for warning in &report.warnings {
                out.warn(warning);
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Monitor { side, port } => {
            let project = require_project(cli.project.as_deref())?;
            device::monitor(&project, side.into(), port.as_deref(), verbose)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Studio { dir } => {
            let dir = match dir {
                Some(dir) => dir,
                None => {
                    let start = match &cli.project {
                        Some(dir) => dir.clone(),
                        None => current_dir()?,
                    };
                    Project::find_root(&start).unwrap_or(start)
                }
            };
            studio::launch(&dir, verbose)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// `--side`와 포트 옵션 조합을 검사해 대상 목록을 만든다. 잘못된 조합은 사용법 오류다.
fn flash_targets(
    side: FlashSide,
    port: Option<String>,
    left_port: Option<String>,
    right_port: Option<String>,
) -> Vec<FlashTarget> {
    match side {
        FlashSide::Both => {
            if port.is_some() {
                usage_error("--side both에서는 -p 대신 --left-port와 --right-port를 쓰세요");
            }
            vec![
                FlashTarget {
                    side: Side::Left,
                    port: left_port,
                },
                FlashTarget {
                    side: Side::Right,
                    port: right_port,
                },
            ]
        }
        FlashSide::Left | FlashSide::Right => {
            if left_port.is_some() || right_port.is_some() {
                usage_error(
                    "--left-port와 --right-port는 --side both에서만 쓸 수 있습니다. -p를 쓰세요",
                );
            }
            let side = if side == FlashSide::Left {
                Side::Left
            } else {
                Side::Right
            };
            vec![FlashTarget { side, port }]
        }
    }
}

fn print_report(report: &doctor::Report, out: Output) {
    for check in &report.checks {
        if out.quiet && matches!(check.level, Level::Ok | Level::Info) {
            continue;
        }
        eprintln!(
            "[{:<5}] {:<20} {}",
            check.level.as_str(),
            check.id,
            check.message
        );
        if let Some(hint) = &check.hint
            && check.level != Level::Ok
        {
            eprintln!("{:28}→ {hint}", "");
        }
    }
    out.info(format!(
        "\n오류 {}개, 경고 {}개",
        report.count(Level::Error),
        report.count(Level::Warn)
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn flash_both_builds_two_targets() {
        let targets = flash_targets(FlashSide::Both, None, Some("COM5".into()), None);
        assert_eq!(
            targets,
            vec![
                FlashTarget {
                    side: Side::Left,
                    port: Some("COM5".into())
                },
                FlashTarget {
                    side: Side::Right,
                    port: None
                },
            ]
        );
    }

    #[test]
    fn flash_single_side_uses_port() {
        let targets = flash_targets(FlashSide::Right, Some("COM6".into()), None, None);
        assert_eq!(
            targets,
            vec![FlashTarget {
                side: Side::Right,
                port: Some("COM6".into())
            }]
        );
    }

    #[test]
    fn parses_run_trailing_args() {
        let cli = Cli::try_parse_from(["dotbox", "run", "--release", "--", "--seed", "7"]).unwrap();
        match cli.command {
            Command::Run { release, args } => {
                assert!(release);
                assert_eq!(args, ["--seed", "7"]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn engine_path_conflicts_with_git_options() {
        let result = Cli::try_parse_from([
            "dotbox",
            "new",
            "snake",
            "--engine-path",
            ".",
            "--engine-rev",
            "v0.1.0",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn flash_requires_side() {
        assert!(Cli::try_parse_from(["dotbox", "flash"]).is_err());
        assert!(Cli::try_parse_from(["dotbox", "flash", "--side", "middle"]).is_err());
    }

    #[test]
    fn monitor_defaults_to_left_and_rejects_both() {
        let cli = Cli::try_parse_from(["dotbox", "monitor"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Monitor {
                side: OneSide::Left,
                port: None
            }
        ));
        assert!(Cli::try_parse_from(["dotbox", "monitor", "--side", "both"]).is_err());
    }

    #[test]
    fn global_options_work_after_subcommand() {
        let cli = Cli::try_parse_from(["dotbox", "doctor", "-C", "game", "-q"]).unwrap();
        assert_eq!(cli.project, Some(PathBuf::from("game")));
        assert!(cli.quiet);
        assert!(Cli::try_parse_from(["dotbox", "doctor", "-q", "-v"]).is_err());
    }
}
