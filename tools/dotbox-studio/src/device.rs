use serialport::SerialPortType;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};

// 플래시 대상 모듈 선택 (좌측 / 우측 / 양쪽 동시)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlashTarget {
    #[default]
    Left, // 왼쪽 모듈 단독
    Right, // 오른쪽 모듈 단독
    Both,  // 양쪽 모듈 모두
}

impl FlashTarget {
    // CLI에 전달할 대상 인자 문자열 반환
    pub fn as_cli_arg(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Both => "both",
        }
    }
}

// PC에서 감지된 시리얼 포트 1개 정보
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedPort {
    pub name: String,        // 포트 이름 (예: COM3, /dev/ttyUSB0)
    pub description: String, // 장치 종류 및 제조사 설명
}

// 현재 PC에 연결된 시리얼 포트 목록 스캔 (실패 시 Result 반환)
pub fn scan_serial_ports() -> Result<Vec<DetectedPort>, String> {
    // OS에서 사용 가능한 시리얼 포트 목록 조회
    let ports = serialport::available_ports()
        .map_err(|e| format!("시리얼 포트 목록을 불러오지 못했습니다: {e}"))?;

    // 조회된 각 포트를 화면 표시용 구조체로 변환
    let mut result: Vec<DetectedPort> = ports
        .into_iter()
        .map(|info| {
            // 포트 유형(USB / 블루투스 등)에 따라 설명 문구 생성
            let description = match info.port_type {
                SerialPortType::UsbPort(usb) => {
                    let product = usb.product.unwrap_or_else(|| "USB 시리얼".to_string());
                    let maker = usb.manufacturer.unwrap_or_default();
                    if maker.is_empty() {
                        format!("{product} (VID:{:04X} PID:{:04X})", usb.vid, usb.pid)
                    } else {
                        format!(
                            "{product} - {maker} (VID:{:04X} PID:{:04X})",
                            usb.vid, usb.pid
                        )
                    }
                }
                SerialPortType::BluetoothPort => "블루투스 포트".to_string(),
                SerialPortType::PciPort => "PCI 포트".to_string(),
                SerialPortType::Unknown => "알 수 없는 포트".to_string(),
            };

            DetectedPort {
                name: info.port_name,
                description,
            }
        })
        .collect();

    // 포트 이름 기준으로 정렬해 매번 일관된 순서로 표시
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

// `dotbox doctor` 명령 인자 목록 생성
pub fn build_doctor_args() -> Vec<String> {
    vec!["doctor".to_string()]
}

// `dotbox run` 명령 인자 목록 생성
pub fn build_run_args() -> Vec<String> {
    vec!["run".to_string()]
}

// `dotbox flash` 명령 인자 목록 생성 (`docs/cli.md`의 flash 명세를 따른다)
// - 한쪽만 구울 때: `flash --side left -p COM3`
// - 양쪽을 구울 때: `flash --side both --left-port COM3 --right-port COM4`
// 포트 문자열이 비어 있으면 해당 옵션을 빼고 CLI의 자동 포트 결정에 맡긴다.
pub fn build_flash_args(target: FlashTarget, port: &str, right_port: &str) -> Vec<String> {
    let mut args = vec![
        "flash".to_string(),
        "--side".to_string(),
        target.as_cli_arg().to_string(),
    ];
    let mut push_port = |option: &str, value: &str| {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            args.push(option.to_string());
            args.push(trimmed.to_string());
        }
    };

    if target == FlashTarget::Both {
        push_port("--left-port", port);
        push_port("--right-port", right_port);
    } else {
        push_port("--port", port);
    }

    args
}

// 실행할 명령어와 인자를 로그 출력용 한 줄 문자열로 합치기
pub fn format_command(program: &str, args: &[String]) -> String {
    if args.is_empty() {
        program.to_string()
    } else {
        format!("{program} {}", args.join(" "))
    }
}

// CLI 명령을 백그라운드 스레드에서 실행하고 결과 수신 채널 반환 (GUI 멈춤 방지)
pub fn run_command_async(
    program: String,
    args: Vec<String>,
    work_dir: PathBuf,
) -> Receiver<String> {
    let (tx, rx) = mpsc::channel(); // 작업 완료 결과를 돌려받을 채널 생성

    // GUI 프레임을 막지 않도록 별도 스레드에서 프로세스 실행
    std::thread::spawn(move || {
        let cmd_line = format_command(&program, &args); // 실행한 명령 문자열 기록
        let output = Command::new(&program)
            .args(&args)
            .current_dir(&work_dir)
            .output();

        // 실행 성공/실패 결과를 읽기 쉬운 텍스트 로그로 정리
        let report = match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let status_text = if out.status.success() {
                    "성공 (종료 코드 0)".to_string()
                } else {
                    format!("실패 ({})", out.status)
                };

                let mut text = format!("$ {cmd_line}\n상태: {status_text}\n");
                if !stdout.trim().is_empty() {
                    text.push_str("\n[출력]\n");
                    text.push_str(&stdout);
                }
                if !stderr.trim().is_empty() {
                    text.push_str("\n[오류]\n");
                    text.push_str(&stderr);
                }
                text
            }
            Err(err) => {
                // dotbox 실행 파일이 없거나 경로가 잘못된 경우 안내 메시지 출력
                format!(
                    "$ {cmd_line}\n명령을 실행할 수 없습니다: {err}\n\
                     힌트: `dotbox` CLI가 PATH에 설치되어 있는지, 또는 프로젝트 경로가 올바른지 확인하세요."
                )
            }
        };

        // 결과를 메인 GUI 스레드로 전송 (창이 닫혔으면 조용히 무시)
        let _ = tx.send(report);
    });

    rx
}

// GUI와 실제 기기 없이 명령 인자 생성 로직만 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;

    // doctor 명령 인자가 올바르게 생성되는지 검증
    #[test]
    fn doctor_args_match_expected() {
        assert_eq!(build_doctor_args(), vec!["doctor"]);
    }

    // 단일 모듈 플래시 시 --side와 --port 옵션이 포함되는지 검증
    #[test]
    fn flash_single_side_includes_port() {
        let args = build_flash_args(FlashTarget::Left, "COM3", "COM9");
        assert_eq!(args, vec!["flash", "--side", "left", "--port", "COM3"]);
    }

    // 양쪽 동시 플래시 시 모듈별 포트 옵션을 쓰는지 검증
    #[test]
    fn flash_both_uses_per_side_ports() {
        let args = build_flash_args(FlashTarget::Both, "COM3", "COM4");
        assert_eq!(
            args,
            vec![
                "flash",
                "--side",
                "both",
                "--left-port",
                "COM3",
                "--right-port",
                "COM4"
            ]
        );
    }

    // 빈 포트 문자열은 포트 인자를 추가하지 않는지 검증 (자동 결정)
    #[test]
    fn flash_ignores_blank_port() {
        assert_eq!(
            build_flash_args(FlashTarget::Right, "   ", ""),
            vec!["flash", "--side", "right"]
        );
        assert_eq!(
            build_flash_args(FlashTarget::Both, "", " COM4 "),
            vec!["flash", "--side", "both", "--right-port", "COM4"]
        );
    }
}
