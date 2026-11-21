use rodio::{OutputStream, Sink, Source};
use std::time::Duration;

// PC 스피커 출력용 기본 샘플레이트 (44.1kHz)
const SAMPLE_RATE: u32 = 44_100;
// 귀가 아프지 않도록 구형파 기본 볼륨 제한 (15%)
const BUZZER_VOLUME: f32 = 0.15;

// 자주 쓰는 기본 음계 프리셋 (이름, 주파수 Hz)
pub const NOTE_PRESETS: [(&str, u16); 9] = [
    ("쉼표", 0),
    ("도(C5)", 523),
    ("레(D5)", 587),
    ("미(E5)", 659),
    ("파(F5)", 698),
    ("솔(G5)", 784),
    ("라(A5)", 880),
    ("시(B5)", 988),
    ("높은도(C6)", 1047),
];

// 단음 부저 음표 1개 (주파수 0Hz = 쉼표)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note {
    pub freq_hz: u16,     // 주파수 (Hz), 0이면 무음(쉼표)
    pub duration_ms: u16, // 재생 길이 (ms)
}

impl Note {
    // 새 음표 생성
    pub fn new(freq_hz: u16, duration_ms: u16) -> Self {
        Self {
            freq_hz,
            duration_ms,
        }
    }
}

// 효과음·멜로디 시퀀스를 관리하는 편집기 상태
#[derive(Clone, Debug)]
pub struct MelodyEditor {
    notes: Vec<Note>, // 순서대로 재생할 음표 목록
}

impl Default for MelodyEditor {
    // 앱 시작 시 테스트용 효과음(도-미-솔-높은도)으로 초기화
    fn default() -> Self {
        Self {
            notes: vec![
                Note::new(523, 120),  // 도 (120ms)
                Note::new(659, 120),  // 미 (120ms)
                Note::new(784, 120),  // 솔 (120ms)
                Note::new(1047, 240), // 높은도 (240ms)
            ],
        }
    }
}

impl MelodyEditor {
    // 현재 음표 목록 읽기 전용으로 반환
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    // 현재 음표 목록 수정 가능하게 반환 (UI 입력용)
    pub fn notes_mut(&mut self) -> &mut [Note] {
        &mut self.notes
    }

    // 시퀀스 끝에 새 음표 추가
    pub fn add_note(&mut self, freq_hz: u16, duration_ms: u16) {
        self.notes.push(Note::new(freq_hz, duration_ms)); // 벡터 끝에 삽입
    }

    // 지정한 위치의 음표 삭제 (범위 밖이면 무시)
    pub fn remove_note(&mut self, index: usize) {
        if index < self.notes.len() {
            self.notes.remove(index); // 해당 인덱스 요소 제거
        }
    }

    // 음표를 한 칸 위로 이동
    pub fn move_up(&mut self, index: usize) {
        // 첫 번째(0)가 아니고 범위 안일 때만 앞 요소와 자리 교체
        if index > 0 && index < self.notes.len() {
            self.notes.swap(index, index - 1);
        }
    }

    // 음표를 한 칸 아래로 이동
    pub fn move_down(&mut self, index: usize) {
        // 마지막 요소가 아닐 때만 뒤 요소와 자리 교체
        if index + 1 < self.notes.len() {
            self.notes.swap(index, index + 1);
        }
    }

    // 모든 음표 비우기
    pub fn clear(&mut self) {
        self.notes.clear();
    }

    // 전체 멜로디의 총 재생 시간(ms) 계산
    pub fn total_duration_ms(&self) -> u32 {
        // 각 음표의 duration_ms를 u32로 변환해 모두 합산
        self.notes.iter().map(|n| u32::from(n.duration_ms)).sum()
    }
}

// 수동 부저(PWM) 소리를 흉내 내는 단음 구형파(Square Wave) 오디오 소스
pub struct SquareWave {
    freq_hz: u16,          // 출력 주파수 (0이면 무음)
    duration_ms: u16,      // 재생 길이 (ms)
    total_samples: usize,  // 생성할 전체 샘플 개수
    current_sample: usize, // 현재까지 출력한 샘플 인덱스
}

impl SquareWave {
    // 주파수와 길이로 구형파 소스 생성
    pub fn new(freq_hz: u16, duration_ms: u16) -> Self {
        // 재생 시간(ms)에 해당하는 오디오 샘플 수 계산 (초당 44,100개 기준)
        let total_samples = (u64::from(SAMPLE_RATE) * u64::from(duration_ms) / 1000) as usize;
        Self {
            freq_hz,
            duration_ms,
            total_samples,
            current_sample: 0, // 0번 샘플부터 시작
        }
    }
}

impl Iterator for SquareWave {
    type Item = f32;

    // 오디오 장치로 보낼 다음 샘플 1개(-1.0 ~ 1.0) 생성
    fn next(&mut self) -> Option<f32> {
        // 지정된 길이가 끝나면 재생 종료(None 반환)
        if self.current_sample >= self.total_samples {
            return None;
        }
        let idx = self.current_sample; // 현재 샘플 번호 저장
        self.current_sample += 1; // 다음 호출을 위해 인덱스 증가

        // 주파수가 0이면 쉼표(무음 0.0) 출력
        if self.freq_hz == 0 {
            return Some(0.0);
        }

        // 현재 샘플의 위상(0.0 ~ 1.0) 계산
        let phase = (idx as f32 * f32::from(self.freq_hz) / SAMPLE_RATE as f32).fract();
        // 한 주기의 앞 절반은 +, 뒤 절반은 -로 출력해 구형파 생성
        let sample = if phase < 0.5 {
            BUZZER_VOLUME
        } else {
            -BUZZER_VOLUME
        };
        Some(sample)
    }
}

impl Source for SquareWave {
    // 프레임 길이 고정 없음
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    // 모노(1채널) 출력
    fn channels(&self) -> u16 {
        1
    }

    // 샘플레이트(44.1kHz) 반환
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    // 전체 재생 길이 반환
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_millis(u64::from(self.duration_ms)))
    }
}

// 음표 목록을 백그라운드 스레드에서 PC 스피커로 재생 (GUI 멈춤 방지)
pub fn play_melody_async(notes: Vec<Note>) {
    // 재생할 음표가 없으면 바로 종료
    if notes.is_empty() {
        return;
    }

    // 별도 스레드에서 오디오 스트림을 열어 순서대로 재생
    std::thread::spawn(move || {
        // 기본 오디오 출력 장치 열기 (장치 없으면 패닉 없이 종료)
        let Ok((_stream, stream_handle)) = OutputStream::try_default() else {
            return;
        };
        // 오디오 재생 대기열(Sink) 생성
        let Ok(sink) = Sink::try_new(&stream_handle) else {
            return;
        };

        // 모든 음표를 구형파 소스로 변환해 재생 대기열에 순서대로 추가
        for note in notes {
            sink.append(SquareWave::new(note.freq_hz, note.duration_ms));
        }

        // 마지막 음표 재생이 끝날 때까지 백그라운드 스레드 유지
        sink.sleep_until_end();
    });
}

// GUI와 스피커 없이 멜로디 편집·파형 로직만 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;

    // 음표 추가·삭제 및 총 재생 시간 계산 검증
    #[test]
    fn add_remove_and_total_duration() {
        let mut editor = MelodyEditor { notes: Vec::new() }; // 빈 편집기 생성
        editor.add_note(440, 100); // 라(A4) 100ms 추가
        editor.add_note(880, 250); // 라(A5) 250ms 추가

        // 개수(2개)와 총 길이(350ms) 확인
        assert_eq!(editor.notes().len(), 2);
        assert_eq!(editor.total_duration_ms(), 350);

        editor.remove_note(0); // 첫 번째 음표 삭제
        // 두 번째 음표만 남았는지 확인
        assert_eq!(editor.notes().len(), 1);
        assert_eq!(editor.notes()[0], Note::new(880, 250));
    }

    // 음표 순서 위아래 이동 검증
    #[test]
    fn move_note_up_and_down() {
        let mut editor = MelodyEditor { notes: Vec::new() }; // 빈 편집기 생성
        editor.add_note(523, 100); // 0번: 도
        editor.add_note(659, 100); // 1번: 미

        editor.move_up(1); // 1번(미)을 0번으로 이동
        assert_eq!(editor.notes()[0].freq_hz, 659);

        editor.move_down(0); // 0번(미)을 다시 1번으로 이동
        assert_eq!(editor.notes()[0].freq_hz, 523);
    }

    // 쉼표(0Hz)는 모든 샘플이 무음(0.0)으로 생성되는지 검증
    #[test]
    fn square_wave_rest_outputs_silence() {
        let samples: Vec<f32> = SquareWave::new(0, 10).collect(); // 10ms 쉼표 샘플 생성
        assert_eq!(samples.len(), 441); // 44,100Hz * 0.01s = 441개
        assert!(samples.iter().all(|&s| s == 0.0)); // 모든 샘플이 0.0인지 확인
    }
}
