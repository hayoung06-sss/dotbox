use serialport::SerialPortType;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
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

// `dotbox run` 명령 인자 목록 생성
pub fn build_run_args() -> Vec<String> {
    vec!["run".to_string()]
}

// `dotbox flash` 명령 인자 목록 생성 (`docs/cli.md`의 flash 명세를 따른다)
// - 한쪽만 구울 때: `flash --side left --port COM3`
// - 양쪽을 구울 때: `flash --side both --left-port COM3 --right-port COM4`
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

// 백그라운드에서 실행 중인 자식 프로세스와 줄 단위 출력 채널 핸들
pub struct RunningCommand {
    child: Child,         // 중지(kill) 및 종료 여부 확인용 자식 프로세스 핸들
    rx: Receiver<String>, // stdout·stderr 리더 스레드가 보내는 줄 단위 로그 채널
}

impl RunningCommand {
    // 현재까지 도착한 모든 출력 줄을 비차단으로 수집
    pub fn poll_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        while let Ok(line) = self.rx.try_recv() {
            lines.push(line);
        }
        lines
    }

    // 프로세스가 종료되었는지 확인 (종료됐으면 상태 문자열 반환, 실행 중이면 None)
    pub fn try_finish(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    Some("상태: 성공 (종료 코드 0)".to_string())
                } else {
                    Some(format!("상태: 실패 ({status})"))
                }
            }
            Ok(None) => None, // 아직 실행 중
            Err(err) => Some(format!("상태 확인 실패: {err}")),
        }
    }

    // 실행 중인 자식 프로세스를 강제 종료하고 정리 ([■ 중지] 버튼용)
    pub fn stop(&mut self) {
        let _ = self.child.kill(); // 프로세스 종료 신호 전송
        let _ = self.child.wait(); // 좀비 프로세스 방지 대기
    }
}

// CLI 명령을 자식 프로세스로 띄우고 stdout·stderr를 줄 단위로 스트리밍
pub fn spawn_command_streaming(
    program: &str,
    args: &[String],
    work_dir: &Path,
) -> Result<RunningCommand, String> {
    let cmd_line = format_command(program, args);
    let mut child = Command::new(program)
        .args(args)
        .current_dir(work_dir)
        .stdout(Stdio::piped()) // 표준 출력 파이프 연결
        .stderr(Stdio::piped()) // 표준 오류 파이프 연결
        .spawn()
        .map_err(|err| {
            format!(
                "$ {cmd_line}\n명령을 실행할 수 없습니다: {err}\n\
                 힌트: `dotbox` CLI가 PATH에 설치되어 있는지 확인하세요."
            )
        })?;

    let (tx, rx) = mpsc::channel::<String>();

    // stdout을 줄 단위로 읽어 채널에 전송하는 리더 스레드
    if let Some(stdout) = child.stdout.take() {
        let tx_out = tx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_out.send(line);
            }
        });
    }

    // stderr를 줄 단위로 읽어 채널에 전송하는 리더 스레드
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        });
    }

    Ok(RunningCommand { child, rx })
}

// GUI와 실제 기기 없이 명령 인자 생성·실행 실패 처리를 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;

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

    // 존재하지 않는 CLI 실행 시 패닉 없이 안내 오류 문자열을 반환하는지 검증
    #[test]
    fn spawn_missing_binary_returns_error_message() {
        let result = spawn_command_streaming(
            "nonexistent-dotbox-binary-xyz",
            &build_run_args(),
            Path::new("."),
        );
        assert!(result.is_err());
    }
}
