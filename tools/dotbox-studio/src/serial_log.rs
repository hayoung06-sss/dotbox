use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

// 메모리 사용량을 제한하기 위한 모듈별 최대 보관 로그 줄 수
const MAX_LOG_LINES: usize = 2000;
// 기본 시리얼 통신 속도 (ESP32-S3 표준)
pub const DEFAULT_BAUD_RATE: u32 = 115_200;

// 시리얼 로그 1줄 데이터 (패닉·재부팅 강조 여부 포함)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerialLogLine {
    pub text: String,             // 수신된 로그 문자열
    pub is_reboot_or_panic: bool, // 패닉·치명적 오류·재부팅 감지 여부
}

// 패닉, Guru Meditation, 하드웨어 리셋 등 재시작 징후를 감지하는 순수 함수
pub fn is_reboot_or_panic_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("panic")
        || lower.contains("guru meditation")
        || lower.contains("rst:0x")
        || lower.contains("backtrace:")
        || lower.contains("abort()")
        || lower.contains("fatal")
        || lower.contains("rebooting")
        || line.contains("재부팅")
        || line.contains("패닉")
}

// 단일 모듈(왼쪽 또는 오른쪽)의 시리얼 포트 수신·일시정지·검색·저장을 관리하는 모니터
pub struct SerialMonitor {
    pub port_name: String,              // 연결할 시리얼 포트 이름 (예: COM5)
    pub baud_rate: u32,                 // 통신 속도 (기본 115200)
    pub paused: bool,                   // true면 화면 갱신 일시 정지 (내부 버퍼에 임시 보관)
    pub status: String,                 // 연결 상태 안내 메시지
    lines: Vec<SerialLogLine>,          // 화면에 표시되는 로그 목록
    pending: Vec<SerialLogLine>,        // 일시 정지 중 도착한 로그 임시 버퍼
    rx: Option<Receiver<String>>,       // 백그라운드 리더 스레드 수신 채널
    stop_flag: Option<Arc<AtomicBool>>, // 백그라운드 스레드 종료 신호
}

impl Default for SerialMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SerialMonitor {
    fn drop(&mut self) {
        self.disconnect();
    }
}

impl SerialMonitor {
    // 연결되지 않은 빈 시리얼 모니터 생성
    pub fn new() -> Self {
        Self {
            port_name: String::new(),
            baud_rate: DEFAULT_BAUD_RATE,
            paused: false,
            status: "연결 안 됨".to_string(),
            lines: Vec::new(),
            pending: Vec::new(),
            rx: None,
            stop_flag: None,
        }
    }

    // 현재 시리얼 포트에 연결되어 있는지 여부 반환
    pub fn is_connected(&self) -> bool {
        self.rx.is_some()
    }

    // 지정한 시리얼 포트를 열고 백그라운드 스레드에서 줄 단위 수신 시작
    pub fn connect(&mut self) {
        self.disconnect();

        let port_name = self.port_name.trim().to_string();
        if port_name.is_empty() {
            self.status = "오류: 포트 이름을 입력하세요.".to_string();
            return;
        }

        let baud = self.baud_rate;
        let port = match serialport::new(&port_name, baud)
            .timeout(Duration::from_millis(100))
            .open()
        {
            Ok(p) => p,
            Err(err) => {
                self.status = format!("연결 실패 ({port_name}): {err}");
                return;
            }
        };

        let (tx, rx) = mpsc::channel::<String>();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        std::thread::spawn(move || {
            let mut reader = BufReader::new(port);
            let mut buf = String::new();
            while !stop_clone.load(Ordering::Relaxed) {
                buf.clear();
                match reader.read_line(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let line = buf.trim_end_matches(&['\r', '\n'][..]).to_string();
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::TimedOut => continue,
                    Err(e) => {
                        let _ = tx.send(format!("[시리얼 읽기 오류] {e}"));
                        break;
                    }
                }
            }
        });

        self.rx = Some(rx);
        self.stop_flag = Some(stop);
        self.status = format!("연결됨 ({port_name} @ {baud})");
    }

    // 백그라운드 수신 스레드를 중지하고 포트 연결 해제
    pub fn disconnect(&mut self) {
        if let Some(flag) = self.stop_flag.take() {
            flag.store(true, Ordering::Relaxed);
        }
        if self.rx.take().is_some() {
            self.status = "연결 해제됨".to_string();
        }
    }

    // 일시 정지 토글 (재개 시 일시 정지 동안 쌓인 로그를 즉시 반영)
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if !self.paused {
            self.flush_pending();
        }
    }

    // 일시 정지 중 모아둔 로그를 메인 목록으로 반영
    fn flush_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let drained = std::mem::take(&mut self.pending);
        for item in drained {
            self.push_displayed_line(item);
        }
    }

    // 표시 목록에 1줄 추가 (최대 줄 수 초과 시 오래된 줄 제거)
    fn push_displayed_line(&mut self, item: SerialLogLine) {
        self.lines.push(item);
        if self.lines.len() > MAX_LOG_LINES {
            let overflow = self.lines.len() - MAX_LOG_LINES;
            self.lines.drain(0..overflow);
        }
    }

    // 새 로그 줄을 내부 상태에 반영 (일시 정지 중이면 pending 버퍼에 보관)
    pub fn push_line(&mut self, text: String) {
        let item = SerialLogLine {
            is_reboot_or_panic: is_reboot_or_panic_line(&text),
            text,
        };
        if self.paused {
            self.pending.push(item);
            if self.pending.len() > MAX_LOG_LINES {
                let overflow = self.pending.len() - MAX_LOG_LINES;
                self.pending.drain(0..overflow);
            }
        } else {
            self.push_displayed_line(item);
        }
    }

    // 매 프레임 백그라운드 채널에서 도착한 줄을 비차단으로 수집
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else {
            return false;
        };

        let mut received = Vec::new();
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok(line) => received.push(line),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        for line in received {
            self.push_line(line);
        }

        if disconnected {
            self.rx = None;
            self.stop_flag = None;
            self.status = "연결 끊김".to_string();
            return false;
        }

        true
    }

    // 표시 중인 로그 및 대기 버퍼 모두 지우기
    pub fn clear(&mut self) {
        self.lines.clear();
        self.pending.clear();
    }

    // 검색어(대소문자 무시)로 필터링된 로그 목록 반환
    pub fn filtered_lines<'a>(&'a self, query: &str) -> Vec<&'a SerialLogLine> {
        let q = query.trim().to_ascii_lowercase();
        if q.is_empty() {
            return self.lines.iter().collect();
        }
        self.lines
            .iter()
            .filter(|line| line.text.to_ascii_lowercase().contains(&q))
            .collect()
    }

    // 일시 정지 중 대기 중인 줄 수 반환
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    // 현재 누적된 로그를 텍스트 파일로 저장
    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("로그 폴더 생성 실패 ({}): {e}", parent.display()))?;
        }
        let mut content = String::new();
        for line in &self.lines {
            content.push_str(&line.text);
            content.push('\n');
        }
        fs::write(path, content)
            .map_err(|e| format!("시리얼 로그 저장 실패 ({}): {e}", path.display()))
    }
}

// GUI와 실제 시리얼 장치 없이 패닉 감지·일시정지·검색·파일 저장을 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_panic_and_reboot_lines() {
        assert!(is_reboot_or_panic_line("panicked at 'index out of bounds'"));
        assert!(is_reboot_or_panic_line(
            "Guru Meditation Error: Core 0 panic'ed"
        ));
        assert!(is_reboot_or_panic_line("rst:0x1 (POWERON_RESET),boot:0x8"));
        assert!(!is_reboot_or_panic_line("INFO dotbox: frame 60fps"));
    }

    #[test]
    fn pause_buffers_lines_and_flushes_on_resume() {
        let mut mon = SerialMonitor::new();
        mon.push_line("boot ok".to_string());
        assert_eq!(mon.filtered_lines("").len(), 1);

        mon.toggle_pause();
        mon.push_line("while paused".to_string());
        assert_eq!(mon.filtered_lines("").len(), 1);
        assert_eq!(mon.pending_count(), 1);

        mon.toggle_pause();
        assert_eq!(mon.filtered_lines("").len(), 2);
        assert_eq!(mon.pending_count(), 0);
    }

    #[test]
    fn filter_and_save_log_round_trip() {
        let mut mon = SerialMonitor::new();
        mon.push_line("INFO game started".to_string());
        mon.push_line("WARN low battery".to_string());

        let filtered = mon.filtered_lines("battery");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].text, "WARN low battery");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serial_left.log");
        mon.save_to_file(&path).unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.contains("INFO game started"));
        assert!(saved.contains("WARN low battery"));
    }
}
