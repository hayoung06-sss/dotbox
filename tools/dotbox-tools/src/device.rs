//! 기기 작업: 시리얼 포트 결정, 플래시(`dotbox flash`), 시리얼 모니터(`dotbox monitor`).

use std::fs;
use std::path::Path;
use std::process::Command;

use dotbox_core::display::Side;
use dotbox_core::pairing::RoleRecord;
use sha2::{Digest, Sha256};

use crate::cargo;
use crate::local::LocalConfig;
use crate::manifest::Project;
use crate::process;
use crate::{Error, Result};

/// 역할 레코드를 쓰는 `dotbox` 파티션의 오프셋. 프로젝트 템플릿의 `partitions.csv`와 같아야 한다.
pub const ROLE_PARTITION_OFFSET: u32 = 0x10000;

/// 모듈 이름(명령줄·설정 파일용).
pub fn side_key(side: Side) -> &'static str {
    match side {
        Side::Left => "left",
        Side::Right => "right",
    }
}

/// 모듈 이름(메시지용).
pub fn side_label(side: Side) -> &'static str {
    match side {
        Side::Left => "왼쪽",
        Side::Right => "오른쪽",
    }
}

/// 감지된 시리얼 포트.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialPortInfo {
    pub name: String,
    /// USB 장치 설명(제조사, 제품명).
    pub description: Option<String>,
}

impl SerialPortInfo {
    pub fn label(&self) -> String {
        match &self.description {
            Some(description) => format!("{} ({description})", self.name),
            None => self.name.clone(),
        }
    }
}

/// 연결된 USB 시리얼 포트를 찾는다. ESP32 개발 보드는 USB-시리얼 칩으로 연결된다.
pub fn detect_ports() -> Vec<SerialPortInfo> {
    let Ok(ports) = serialport::available_ports() else {
        return Vec::new();
    };
    ports
        .into_iter()
        .filter_map(|port| match port.port_type {
            serialport::SerialPortType::UsbPort(usb) => {
                let description = [usb.manufacturer, usb.product]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                Some(SerialPortInfo {
                    name: port.port_name,
                    description: (!description.is_empty()).then_some(description),
                })
            }
            _ => None,
        })
        .collect()
}

/// 한 모듈의 포트를 정한다(`docs/cli.md` 6.5).
///
/// 1. 명령줄 지정 → 2. 저장된 포트(감지 목록에 있어야 함) → 3. 감지된 포트가 정확히 하나이고 `allow_auto`일 때 그 포트.
pub fn resolve_port(
    side: Side,
    explicit: Option<&str>,
    saved: Option<&str>,
    detected: &[String],
    allow_auto: bool,
) -> Result<String> {
    let label = side_label(side);
    let key = side_key(side);
    if let Some(port) = explicit {
        return Ok(port.to_owned());
    }
    let listing = if detected.is_empty() {
        "감지된 포트 없음".to_owned()
    } else {
        format!("감지된 포트: {}", detected.join(", "))
    };
    if let Some(port) = saved {
        if detected.iter().any(|d| d == port) {
            return Ok(port.to_owned());
        }
        return Err(Error::new(format!(
            "{label} 모듈의 저장된 포트 {port}가 연결되어 있지 않습니다 ({listing})"
        ))
        .with_hint(format!(
            "케이블을 확인하거나 --{key}-port / -p로 포트를 지정하세요"
        )));
    }
    match detected {
        [only] if allow_auto => Ok(only.clone()),
        [] => Err(Error::new(format!("{label} 모듈의 포트를 찾지 못했습니다 ({listing})"))
            .with_hint("모듈을 USB로 연결하고 드라이버(CP210x, CH340)를 확인하세요")),
        _ => Err(Error::new(format!(
            "{label} 모듈이 어느 포트인지 알 수 없습니다 ({listing})"
        ))
        .with_hint(format!(
            "처음 한 번은 포트를 지정하세요. 예: dotbox flash --side both --left-port COM5 --right-port COM6 (또는 --side {key} -p <PORT>)"
        ))),
    }
}

/// 플래시할 모듈 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashTarget {
    pub side: Side,
    /// 명령줄로 지정한 포트.
    pub port: Option<String>,
}

/// 모든 대상의 포트를 정한다. 하나라도 실패하면 아무것도 정하지 않는다.
pub fn resolve_targets(
    targets: &[FlashTarget],
    local: &LocalConfig,
    detected: &[String],
) -> Result<Vec<(Side, String)>> {
    let allow_auto = targets.len() == 1;
    let resolved = targets
        .iter()
        .map(|target| {
            let port = resolve_port(
                target.side,
                target.port.as_deref(),
                local.ports.get(target.side),
                detected,
                allow_auto,
            )?;
            Ok((target.side, port))
        })
        .collect::<Result<Vec<_>>>()?;
    if let [(_, left), (_, right)] = resolved.as_slice()
        && left == right
    {
        return Err(
            Error::new(format!("두 모듈이 같은 포트({left})로 정해졌습니다"))
                .with_hint("--left-port와 --right-port를 서로 다르게 지정하세요"),
        );
    }
    Ok(resolved)
}

/// 펌웨어 ELF로 `build_id`(SHA-256 앞 8바이트, 16진수 16자)를 만든다.
pub fn build_id(elf: &[u8]) -> String {
    Sha256::digest(elf)[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn flash_command(port: &str, elf: &Path, partition_table: &Path, monitor: bool) -> Command {
    let mut command = Command::new("espflash");
    command
        .arg("flash")
        .arg("--port")
        .arg(port)
        .arg("--partition-table")
        .arg(partition_table);
    if monitor {
        command.arg("--monitor");
    }
    command.arg(elf);
    command
}

pub fn write_role_command(port: &str, role_file: &Path) -> Command {
    let mut command = Command::new("espflash");
    command
        .arg("write-bin")
        .arg("--port")
        .arg(port)
        .arg(format!("{ROLE_PARTITION_OFFSET:#x}"))
        .arg(role_file);
    command
}

pub fn monitor_command(port: &str) -> Command {
    let mut command = Command::new("espflash");
    command.arg("monitor").arg("--port").arg(port);
    command
}

/// 두 모듈 기록이 다를 때의 경고.
pub fn mismatch_warning(local: &LocalConfig) -> Option<String> {
    local.flashed_mismatch().then(|| {
        "왼쪽과 오른쪽 모듈의 펌웨어가 다릅니다. dotbox flash --side both로 다시 구우세요"
            .to_owned()
    })
}

/// `dotbox flash` 옵션.
#[derive(Debug, Clone)]
pub struct FlashOptions {
    pub targets: Vec<FlashTarget>,
    pub release: bool,
    /// 마지막 모듈을 구운 뒤 모니터를 연다. 대상이 하나일 때만 허용한다.
    pub monitor: bool,
    pub verbose: bool,
}

/// 플래시 결과.
#[derive(Debug, Clone)]
pub struct FlashReport {
    pub elf: std::path::PathBuf,
    pub build_id: String,
    pub flashed: Vec<(Side, String)>,
    pub warnings: Vec<String>,
}

/// 펌웨어를 한 번 빌드해 대상 모듈에 굽고 역할을 기록한다(`docs/cli.md` 6.5).
pub fn flash(project: &Project, options: &FlashOptions) -> Result<FlashReport> {
    if options.targets.is_empty() {
        return Err(Error::new("플래시할 모듈이 없습니다"));
    }
    if options.monitor && options.targets.len() > 1 {
        return Err(
            Error::new("양쪽 모듈을 동시에 모니터하는 기능은 아직 없습니다")
                .with_hint("--monitor 없이 구운 뒤 dotbox monitor --side left|right를 쓰세요"),
        );
    }

    let mut local = LocalConfig::load(project)?;
    let detected: Vec<String> = detect_ports().into_iter().map(|p| p.name).collect();
    let ports = resolve_targets(&options.targets, &local, &detected)?;

    let elf = cargo::build_firmware(project, options.release, options.verbose)?;
    let elf_bytes = fs::read(&elf).map_err(|err| Error::io("읽기", &elf, err))?;
    let build_id = build_id(&elf_bytes);
    let partition_table = project.firmware_dir().join("partitions.csv");
    let local_dir = project.local_dir();
    fs::create_dir_all(&local_dir).map_err(|err| Error::io("만들기", &local_dir, err))?;

    let mut flashed = Vec::new();
    for (index, (side, port)) in ports.iter().enumerate() {
        let label = side_label(*side);
        let is_last = index + 1 == ports.len();
        eprintln!("{label} 모듈({port})에 굽는 중...");
        // 역할을 먼저 쓰면 앱을 굽다가 실패했을 때 역할만 바뀐 기기가 남으므로 앱부터 굽는다.
        process::run(
            &mut flash_command(port, &elf, &partition_table, false),
            options.verbose,
        )?;
        let role_file = local_dir.join(format!("role-{}.bin", side_key(*side)));
        fs::write(&role_file, RoleRecord::new(*side).to_bytes())
            .map_err(|err| Error::io("쓰기", &role_file, err))?;
        process::run(&mut write_role_command(port, &role_file), options.verbose)?;

        local.ports.set(*side, port.clone());
        local.flashed.set(*side, build_id.clone());
        local.save(project)?;
        flashed.push((*side, port.clone()));

        if options.monitor && is_last {
            process::run(&mut monitor_command(port), options.verbose)?;
        }
    }

    let warnings = mismatch_warning(&local).into_iter().collect();
    Ok(FlashReport {
        elf,
        build_id,
        flashed,
        warnings,
    })
}

/// 시리얼 모니터를 연다.
pub fn monitor(project: &Project, side: Side, port: Option<&str>, verbose: bool) -> Result<()> {
    let local = LocalConfig::load(project)?;
    let detected: Vec<String> = detect_ports().into_iter().map(|p| p.name).collect();
    let port = resolve_port(side, port, local.ports.get(side), &detected, true)?;
    process::run(&mut monitor_command(&port), verbose)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::describe;

    fn ports(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn explicit_port_wins() {
        let port = resolve_port(
            Side::Left,
            Some("COM9"),
            Some("COM5"),
            &ports(&["COM5"]),
            true,
        );
        assert_eq!(port.unwrap(), "COM9");
    }

    #[test]
    fn saved_port_must_be_connected() {
        let detected = ports(&["COM5", "COM6"]);
        assert_eq!(
            resolve_port(Side::Left, None, Some("COM5"), &detected, false).unwrap(),
            "COM5"
        );
        let err = resolve_port(Side::Left, None, Some("COM7"), &detected, false).unwrap_err();
        assert!(err.message().contains("COM7"));
        assert!(err.hint().unwrap().contains("--left-port"));
    }

    #[test]
    fn auto_detects_single_port_only_when_allowed() {
        let single = ports(&["COM5"]);
        assert_eq!(
            resolve_port(Side::Right, None, None, &single, true).unwrap(),
            "COM5"
        );
        assert!(resolve_port(Side::Right, None, None, &single, false).is_err());
    }

    #[test]
    fn refuses_to_guess_between_ports() {
        let err =
            resolve_port(Side::Left, None, None, &ports(&["COM5", "COM6"]), true).unwrap_err();
        assert!(err.message().contains("알 수 없습니다"));
        assert!(err.hint().unwrap().contains("--left-port"));
        let err = resolve_port(Side::Left, None, None, &[], true).unwrap_err();
        assert!(err.message().contains("찾지 못했습니다"));
    }

    #[test]
    fn both_targets_use_saved_ports() {
        let mut local = LocalConfig::default();
        local.ports.set(Side::Left, "COM5");
        local.ports.set(Side::Right, "COM6");
        let targets = [
            FlashTarget {
                side: Side::Left,
                port: None,
            },
            FlashTarget {
                side: Side::Right,
                port: None,
            },
        ];
        let resolved = resolve_targets(&targets, &local, &ports(&["COM5", "COM6"])).unwrap();
        assert_eq!(
            resolved,
            vec![
                (Side::Left, "COM5".to_owned()),
                (Side::Right, "COM6".to_owned())
            ]
        );
    }

    #[test]
    fn both_targets_never_auto_detect() {
        // 포트가 두 개 감지되어도 어느 쪽이 왼쪽인지 모르면 실패한다.
        let targets = [
            FlashTarget {
                side: Side::Left,
                port: None,
            },
            FlashTarget {
                side: Side::Right,
                port: Some("COM6".to_owned()),
            },
        ];
        let err = resolve_targets(&targets, &LocalConfig::default(), &ports(&["COM5", "COM6"]))
            .unwrap_err();
        assert!(err.message().contains("왼쪽"));
    }

    #[test]
    fn both_targets_reject_same_port() {
        let targets = [
            FlashTarget {
                side: Side::Left,
                port: Some("COM5".to_owned()),
            },
            FlashTarget {
                side: Side::Right,
                port: Some("COM5".to_owned()),
            },
        ];
        let err = resolve_targets(&targets, &LocalConfig::default(), &[]).unwrap_err();
        assert!(err.message().contains("같은 포트"));
    }

    #[test]
    fn build_id_is_first_8_bytes_of_sha256() {
        // SHA-256("abc") = ba7816bf8f01cfea...
        assert_eq!(build_id(b"abc"), "ba7816bf8f01cfea");
        assert_eq!(build_id(b"abc").len(), 16);
        assert_ne!(build_id(b"abc"), build_id(b"abd"));
    }

    #[test]
    fn espflash_commands() {
        let elf = Path::new("fw.elf");
        let table = Path::new("partitions.csv");
        assert_eq!(
            describe(&flash_command("COM5", elf, table, false)),
            "espflash flash --port COM5 --partition-table partitions.csv fw.elf"
        );
        assert_eq!(
            describe(&flash_command("COM5", elf, table, true)),
            "espflash flash --port COM5 --partition-table partitions.csv --monitor fw.elf"
        );
        assert_eq!(
            describe(&write_role_command("COM5", Path::new("role-left.bin"))),
            "espflash write-bin --port COM5 0x10000 role-left.bin"
        );
        assert_eq!(
            describe(&monitor_command("COM6")),
            "espflash monitor --port COM6"
        );
    }

    #[test]
    fn mismatch_warning_text() {
        let mut local = LocalConfig::default();
        local.flashed.set(Side::Left, "aa");
        assert_eq!(mismatch_warning(&local), None);
        local.flashed.set(Side::Right, "bb");
        assert!(mismatch_warning(&local).unwrap().contains("--side both"));
    }

    #[test]
    fn side_names() {
        assert_eq!(side_key(Side::Left), "left");
        assert_eq!(side_key(Side::Right), "right");
        assert_eq!(side_label(Side::Right), "오른쪽");
    }

    #[test]
    fn flash_rejects_monitor_with_two_targets_before_doing_anything() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(crate::manifest::MANIFEST_FILE),
            crate::manifest::Manifest::new("snake").to_toml(),
        )
        .unwrap();
        let project = Project::load(dir.path()).unwrap();
        let err = flash(
            &project,
            &FlashOptions {
                targets: vec![
                    FlashTarget {
                        side: Side::Left,
                        port: Some("COM5".to_owned()),
                    },
                    FlashTarget {
                        side: Side::Right,
                        port: Some("COM6".to_owned()),
                    },
                ],
                release: true,
                monitor: true,
                verbose: false,
            },
        )
        .unwrap_err();
        assert!(err.message().contains("동시에 모니터"));
        assert!(!project.local_dir().exists());
    }
}
