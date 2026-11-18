//! 단음 효과음. 왼쪽 모듈의 수동 부저로 낸다.

use crate::Result;
use crate::hal::Buzzer;

/// 음표 하나. `freq_hz`가 0이면 쉼표다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pub freq_hz: u16,
    pub duration_ms: u16,
}

impl Note {
    pub const fn new(freq_hz: u16, duration_ms: u16) -> Self {
        Self {
            freq_hz,
            duration_ms,
        }
    }

    pub const fn rest(duration_ms: u16) -> Self {
        Self::new(0, duration_ms)
    }
}

/// 효과음. 펌웨어에 들어가는 정적 데이터다.
pub type Sound = [Note];

/// 효과음 하나를 시간에 맞춰 재생한다. 새 요청은 이전 소리를 끊는다.
#[derive(Debug, Default)]
pub struct SoundPlayer {
    sound: Option<&'static Sound>,
    index: usize,
    note_started_ms: u32,
    /// 부저에 마지막으로 보낸 주파수(같은 값을 반복해서 보내지 않기 위해).
    output_hz: Option<u16>,
}

impl SoundPlayer {
    pub fn play(&mut self, sound: &'static Sound, now_ms: u32) {
        self.sound = Some(sound);
        self.index = 0;
        self.note_started_ms = now_ms;
        self.output_hz = None;
    }

    pub fn stop(&mut self) {
        self.sound = None;
        self.output_hz = None;
    }

    pub fn is_playing(&self) -> bool {
        self.sound.is_some()
    }

    /// 지금 내야 할 주파수. 재생 중이 아니거나 쉼표면 `None`.
    /// 루프가 늦어 지난 음표는 건너뛰고 현재 시각에 맞춘다.
    pub fn advance(&mut self, now_ms: u32) -> Option<u16> {
        let sound = self.sound?;
        loop {
            let Some(note) = sound.get(self.index) else {
                self.sound = None;
                return None;
            };
            let elapsed = now_ms.wrapping_sub(self.note_started_ms);
            let duration = u32::from(note.duration_ms);
            if elapsed < duration {
                return (note.freq_hz != 0).then_some(note.freq_hz);
            }
            self.index += 1;
            self.note_started_ms = self.note_started_ms.wrapping_add(duration);
        }
    }

    /// [`advance`](Self::advance)한 결과를 부저에 반영한다. 바뀔 때만 부저를 부른다.
    pub fn update(&mut self, now_ms: u32, buzzer: &mut dyn Buzzer) -> Result<()> {
        let target = self.advance(now_ms);
        let current = self.output_hz.filter(|&hz| hz != 0);
        if target == current && self.output_hz.is_some() {
            return Ok(());
        }
        match target {
            Some(hz) => buzzer.play_tone(u32::from(hz))?,
            None => buzzer.stop()?,
        }
        self.output_hz = Some(target.unwrap_or(0));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static BEEP: [Note; 3] = [Note::new(440, 100), Note::rest(50), Note::new(880, 100)];

    #[derive(Default)]
    struct FakeBuzzer {
        calls: Vec<Option<u32>>,
    }

    impl Buzzer for FakeBuzzer {
        fn play_tone(&mut self, freq_hz: u32) -> Result<()> {
            self.calls.push(Some(freq_hz));
            Ok(())
        }
        fn stop(&mut self) -> Result<()> {
            self.calls.push(None);
            Ok(())
        }
    }

    #[test]
    fn plays_notes_in_order() {
        let mut p = SoundPlayer::default();
        p.play(&BEEP, 1000);
        assert_eq!(p.advance(1000), Some(440));
        assert_eq!(p.advance(1099), Some(440));
        assert_eq!(p.advance(1100), None); // 쉼표
        assert!(p.is_playing());
        assert_eq!(p.advance(1150), Some(880));
        assert_eq!(p.advance(1250), None);
        assert!(!p.is_playing());
    }

    #[test]
    fn skips_notes_when_late() {
        let mut p = SoundPlayer::default();
        p.play(&BEEP, 0);
        assert_eq!(p.advance(160), Some(880));
    }

    #[test]
    fn new_sound_interrupts() {
        static LOW: [Note; 1] = [Note::new(220, 1000)];
        let mut p = SoundPlayer::default();
        p.play(&BEEP, 0);
        p.play(&LOW, 10);
        assert_eq!(p.advance(20), Some(220));
    }

    #[test]
    fn handles_clock_wraparound() {
        let mut p = SoundPlayer::default();
        p.play(&BEEP, u32::MAX - 10);
        assert_eq!(p.advance(50), Some(440));
    }

    #[test]
    fn update_calls_buzzer_only_on_change() {
        let mut p = SoundPlayer::default();
        let mut buzzer = FakeBuzzer::default();
        p.update(0, &mut buzzer).unwrap();
        p.update(5, &mut buzzer).unwrap();
        assert_eq!(buzzer.calls, [None]);

        p.play(&BEEP, 10);
        p.update(10, &mut buzzer).unwrap();
        p.update(50, &mut buzzer).unwrap();
        p.update(115, &mut buzzer).unwrap();
        p.update(170, &mut buzzer).unwrap();
        p.update(300, &mut buzzer).unwrap();
        assert_eq!(buzzer.calls, [None, Some(440), None, Some(880), None]);
    }
}
