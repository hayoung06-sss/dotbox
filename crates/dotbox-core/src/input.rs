//! 입력 처리와 게임에 넘기는 입력 타입(`docs/game-api.md` 5장).
//!
//! 흐름: 조이스틱 원시 값([`RawInput`]) → [`InputProcessor`](각 모듈) → [`ControlState`](모듈 간 전송 단위)
//! → [`EventTracker`](마스터, 틱마다) → [`PlayerInput`](게임).

use crate::hal::RawInput;
use crate::state::{StateReader, StateWriter};
use crate::{Error, Result};

/// ADC 원시 값의 최댓값(ESP32 12비트).
pub const ADC_MAX: u16 = 4095;

/// 4방향.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    /// x, y 변화량(오른쪽·아래가 +).
    pub const fn delta(self) -> (i32, i32) {
        match self {
            Self::Up => (0, -1),
            Self::Down => (0, 1),
            Self::Left => (-1, 0),
            Self::Right => (1, 0),
        }
    }

    pub const fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    fn to_wire(direction: Option<Self>) -> u8 {
        match direction {
            None => 0,
            Some(Self::Up) => 1,
            Some(Self::Down) => 2,
            Some(Self::Left) => 3,
            Some(Self::Right) => 4,
        }
    }

    fn from_wire(value: u8) -> Result<Option<Self>> {
        Ok(match value {
            0 => None,
            1 => Some(Self::Up),
            2 => Some(Self::Down),
            3 => Some(Self::Left),
            4 => Some(Self::Right),
            _ => return Err(Error::InvalidArgument),
        })
    }
}

/// 버튼 상태.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ButtonState {
    pub held: bool,
    /// 이번 틱에 눌림.
    pub pressed: bool,
    /// 이번 틱에 뗌.
    pub released: bool,
}

/// 플레이어 한 명의 이번 틱 입력.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerInput {
    /// 데드존과 히스테리시스를 거친 4방향. 중립이면 `None`.
    pub direction: Option<Direction>,
    /// 이번 틱에 새로 들어간 방향.
    pub direction_pressed: Option<Direction>,
    /// -100..=100. 오른쪽이 +.
    pub axis_x: i8,
    /// -100..=100. 아래가 +.
    pub axis_y: i8,
    pub button: ButtonState,
    /// 원격 입력이 끊겨 중립으로 바꾼 상태인지.
    pub stale: bool,
}

/// 모듈 합체 상태(`docs/modes.md` 4장).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DockState {
    Docked,
    Undocked,
    /// 홀센서가 없거나 판정할 수 없다.
    #[default]
    Unknown,
}

/// 게임이 한 틱에 받는 입력.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Input {
    /// `[P1(왼쪽 모듈), P2(오른쪽 모듈)]`.
    pub players: [PlayerInput; 2],
    pub dock: DockState,
}

/// 입력 처리 설정. 기본값은 `docs/game-api.md` 5.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputConfig {
    /// 중립 보정에 쓰는 샘플 수.
    pub calibration_samples: u8,
    /// 방향이 들어가는 기준이자 아날로그 데드존(%).
    pub deadzone_percent: u8,
    /// 방향이 풀리는 기준(%). `deadzone_percent`보다 작아야 한다.
    pub release_percent: u8,
    pub debounce_ms: u32,
    /// 축 방향 뒤집기(조이스틱 장착 방향 보정).
    pub invert_x: bool,
    pub invert_y: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            calibration_samples: 8,
            deadzone_percent: 20,
            release_percent: 15,
            debounce_ms: 10,
            invert_x: false,
            invert_y: false,
        }
    }
}

/// 처리를 마친 조이스틱 상태. 슬레이브가 마스터로 보내는 단위다(`docs/game-api.md` 7.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControlState {
    pub direction: Option<Direction>,
    pub axis_x: i8,
    pub axis_y: i8,
    pub button_held: bool,
    /// 버튼 누름 누적 횟수(순환).
    pub press_count: u8,
    /// 버튼 뗌 누적 횟수(순환).
    pub release_count: u8,
}

impl ControlState {
    /// 전송 크기.
    pub const WIRE_LEN: usize = 7;

    pub fn encode(&self, out: &mut StateWriter<'_>) -> Result<()> {
        out.u8(Direction::to_wire(self.direction))?;
        out.i8(self.axis_x)?;
        out.i8(self.axis_y)?;
        out.bool(self.button_held)?;
        out.u8(self.press_count)?;
        out.u8(self.release_count)?;
        out.u8(0) // 예약
    }

    pub fn decode(input: &mut StateReader<'_>) -> Result<Self> {
        let direction = Direction::from_wire(input.u8()?)?;
        let axis_x = input.i8()?;
        let axis_y = input.i8()?;
        let button_held = input.bool()?;
        let press_count = input.u8()?;
        let release_count = input.u8()?;
        let _reserved = input.u8()?;
        if !(-100..=100).contains(&axis_x) || !(-100..=100).contains(&axis_y) {
            return Err(Error::InvalidArgument);
        }
        Ok(Self {
            direction,
            axis_x,
            axis_y,
            button_held,
            press_count,
            release_count,
        })
    }
}

/// 조이스틱 원시 값을 [`ControlState`]로 바꾼다. 조이스틱이 달린 모듈에서 돈다.
#[derive(Debug, Clone)]
pub struct InputProcessor {
    config: InputConfig,
    calibration_sum: (u32, u32),
    calibration_count: u8,
    center: Option<(u16, u16)>,
    state: ControlState,
    /// 디바운스 중인 원시 버튼 값과 그 값이 처음 보인 시각.
    pending_button: Option<(bool, u32)>,
}

impl InputProcessor {
    pub fn new(config: InputConfig) -> Self {
        Self {
            config,
            calibration_sum: (0, 0),
            calibration_count: 0,
            center: None,
            state: ControlState::default(),
            pending_button: None,
        }
    }

    /// 중립 보정이 끝났는지.
    pub fn is_calibrated(&self) -> bool {
        self.center.is_some()
    }

    pub fn state(&self) -> ControlState {
        self.state
    }

    /// 샘플 하나를 반영한다.
    pub fn sample(&mut self, raw: RawInput, now_ms: u32) {
        self.update_button(raw.button_down, now_ms);

        let Some((center_x, center_y)) = self.center else {
            self.calibrate(raw);
            return;
        };
        let mut x = normalize(raw.x, center_x);
        let mut y = normalize(raw.y, center_y);
        if self.config.invert_x {
            x = -x;
        }
        if self.config.invert_y {
            y = -y;
        }
        self.state.direction = self.next_direction(x, y);
        self.state.axis_x = apply_deadzone(x, self.config.deadzone_percent);
        self.state.axis_y = apply_deadzone(y, self.config.deadzone_percent);
    }

    fn calibrate(&mut self, raw: RawInput) {
        self.calibration_sum.0 += u32::from(raw.x);
        self.calibration_sum.1 += u32::from(raw.y);
        self.calibration_count += 1;
        let count = self.config.calibration_samples.max(1);
        if self.calibration_count >= count {
            let n = u32::from(count);
            // 합이 u16 범위 값 n개의 합이므로 나누면 u16 범위다.
            let to_u16 = |sum: u32| u16::try_from(sum / n).unwrap_or(ADC_MAX / 2);
            self.center = Some((
                to_u16(self.calibration_sum.0),
                to_u16(self.calibration_sum.1),
            ));
        }
    }

    fn next_direction(&self, x: i8, y: i8) -> Option<Direction> {
        let current = self.state.direction;
        let threshold = if current.is_some() {
            self.config.release_percent
        } else {
            self.config.deadzone_percent
        };
        let (ax, ay) = (x.unsigned_abs(), y.unsigned_abs());
        if ax.max(ay) < threshold {
            return None;
        }
        let horizontal = if x >= 0 {
            Direction::Right
        } else {
            Direction::Left
        };
        let vertical = if y >= 0 {
            Direction::Down
        } else {
            Direction::Up
        };
        match ax.cmp(&ay) {
            std::cmp::Ordering::Greater => Some(horizontal),
            std::cmp::Ordering::Less => Some(vertical),
            // 같으면 이전 방향이 둘 중 하나일 때 유지한다.
            std::cmp::Ordering::Equal => match current {
                Some(d) if d == horizontal || d == vertical => Some(d),
                _ => Some(horizontal),
            },
        }
    }

    fn update_button(&mut self, raw_down: bool, now_ms: u32) {
        if raw_down == self.state.button_held {
            self.pending_button = None;
            return;
        }
        match self.pending_button {
            Some((value, since)) if value == raw_down => {
                if now_ms.wrapping_sub(since) >= self.config.debounce_ms {
                    self.state.button_held = raw_down;
                    if raw_down {
                        self.state.press_count = self.state.press_count.wrapping_add(1);
                    } else {
                        self.state.release_count = self.state.release_count.wrapping_add(1);
                    }
                    self.pending_button = None;
                }
            }
            _ => {
                self.pending_button = Some((raw_down, now_ms));
                if self.config.debounce_ms == 0 {
                    self.update_button(raw_down, now_ms);
                }
            }
        }
    }
}

/// ADC 값을 중앙 기준 -100..=100으로 바꾼다.
fn normalize(raw: u16, center: u16) -> i8 {
    let raw = i32::from(raw.min(ADC_MAX));
    let center = i32::from(center);
    let span = if raw >= center {
        i32::from(ADC_MAX) - center
    } else {
        center
    };
    if span <= 0 {
        return 0;
    }
    let value = (raw - center) * 100 / span;
    // -100..=100으로 잘랐으므로 i8 범위다.
    i8::try_from(value.clamp(-100, 100)).unwrap_or(0)
}

/// 데드존 안은 0, 밖은 0..=100으로 다시 늘린다.
fn apply_deadzone(value: i8, deadzone_percent: u8) -> i8 {
    let deadzone = i32::from(deadzone_percent.min(99));
    let magnitude = i32::from(value.unsigned_abs());
    if magnitude < deadzone {
        return 0;
    }
    let scaled = (magnitude - deadzone) * 100 / (100 - deadzone);
    let signed = if value < 0 { -scaled } else { scaled };
    i8::try_from(signed.clamp(-100, 100)).unwrap_or(0)
}

/// [`ControlState`]의 변화로 틱 이벤트를 만든다(`docs/game-api.md` 5.3). 플레이어마다 하나씩 둔다.
#[derive(Debug, Clone, Default)]
pub struct EventTracker {
    last: Option<ControlState>,
}

impl EventTracker {
    /// 기준을 현재 상태로 맞춘다. 게임 시작 직후 이미 누르고 있던 버튼으로 이벤트를 만들지 않게 한다.
    pub fn reset_baseline(&mut self, current: ControlState) {
        self.last = Some(current);
    }

    /// 이번 틱의 입력을 만든다.
    pub fn next(&mut self, current: ControlState, stale: bool) -> PlayerInput {
        let last = self.last.unwrap_or(current);
        self.last = Some(current);
        let direction_pressed = match current.direction {
            Some(d) if last.direction != Some(d) => Some(d),
            _ => None,
        };
        PlayerInput {
            direction: current.direction,
            direction_pressed,
            axis_x: current.axis_x,
            axis_y: current.axis_y,
            button: ButtonState {
                held: current.button_held,
                pressed: current.press_count != last.press_count,
                released: current.release_count != last.release_count,
            },
            stale,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CENTER: u16 = 2048;

    fn raw(x: u16, y: u16, button_down: bool) -> RawInput {
        RawInput { x, y, button_down }
    }

    fn calibrated() -> InputProcessor {
        let mut p = InputProcessor::new(InputConfig::default());
        for i in 0..8 {
            p.sample(raw(CENTER, CENTER, false), i);
        }
        assert!(p.is_calibrated());
        p
    }

    /// 중앙 기준 퍼센트를 ADC 값으로.
    fn adc(percent: i32) -> u16 {
        let center = i32::from(CENTER);
        let span = if percent >= 0 {
            i32::from(ADC_MAX) - center
        } else {
            center
        };
        u16::try_from(center + percent * span / 100).unwrap()
    }

    #[test]
    fn neutral_until_calibrated() {
        let mut p = InputProcessor::new(InputConfig::default());
        for i in 0..7 {
            p.sample(raw(ADC_MAX, ADC_MAX, false), i);
            assert_eq!(p.state().direction, None);
            assert_eq!(p.state().axis_x, 0);
        }
        assert!(!p.is_calibrated());
        p.sample(raw(ADC_MAX, ADC_MAX, false), 8);
        assert!(p.is_calibrated());
    }

    #[test]
    fn calibration_uses_average_center() {
        let mut p = InputProcessor::new(InputConfig::default());
        for i in 0..8 {
            p.sample(raw(1900, 2200, false), i);
        }
        p.sample(raw(1900, 2200, false), 10);
        assert_eq!(p.state().axis_x, 0);
        assert_eq!(p.state().axis_y, 0);
        assert_eq!(p.state().direction, None);
    }

    #[test]
    fn normalize_reaches_extremes() {
        assert_eq!(normalize(ADC_MAX, CENTER), 100);
        assert_eq!(normalize(0, CENTER), -100);
        assert_eq!(normalize(CENTER, CENTER), 0);
        assert_eq!(normalize(ADC_MAX, ADC_MAX), 0);
        assert_eq!(normalize(9999, CENTER), 100);
    }

    #[test]
    fn deadzone_rescales() {
        assert_eq!(apply_deadzone(19, 20), 0);
        assert_eq!(apply_deadzone(20, 20), 0);
        assert_eq!(apply_deadzone(60, 20), 50);
        assert_eq!(apply_deadzone(100, 20), 100);
        assert_eq!(apply_deadzone(-100, 20), -100);
        assert_eq!(apply_deadzone(-60, 20), -50);
    }

    #[test]
    fn small_wobble_is_not_a_direction() {
        let mut p = calibrated();
        for offset in [-15, -10, 0, 10, 15, 19] {
            p.sample(raw(adc(offset), adc(-offset), false), 100);
            assert_eq!(p.state().direction, None, "{offset}");
        }
    }

    #[test]
    fn picks_dominant_axis() {
        let mut p = calibrated();
        p.sample(raw(adc(80), adc(30), false), 100);
        assert_eq!(p.state().direction, Some(Direction::Right));
        p.sample(raw(CENTER, CENTER, false), 101);
        p.sample(raw(adc(-30), adc(-90), false), 102);
        assert_eq!(p.state().direction, Some(Direction::Up));
        p.sample(raw(CENTER, CENTER, false), 103);
        p.sample(raw(adc(-90), CENTER, false), 104);
        assert_eq!(p.state().direction, Some(Direction::Left));
        p.sample(raw(CENTER, CENTER, false), 105);
        p.sample(raw(CENTER, adc(90), false), 106);
        assert_eq!(p.state().direction, Some(Direction::Down));
    }

    #[test]
    fn hysteresis_keeps_direction_until_release_threshold() {
        let mut p = calibrated();
        p.sample(raw(adc(18), CENTER, false), 100);
        assert_eq!(p.state().direction, None, "진입 기준(20%) 아래");
        p.sample(raw(adc(25), CENTER, false), 101);
        assert_eq!(p.state().direction, Some(Direction::Right));
        p.sample(raw(adc(17), CENTER, false), 102);
        assert_eq!(
            p.state().direction,
            Some(Direction::Right),
            "해제 기준(15%) 위"
        );
        p.sample(raw(adc(10), CENTER, false), 103);
        assert_eq!(p.state().direction, None);
    }

    #[test]
    fn diagonal_tie_keeps_previous_direction() {
        let mut p = calibrated();
        p.sample(raw(CENTER, adc(60), false), 100);
        assert_eq!(p.state().direction, Some(Direction::Down));
        p.sample(raw(adc(60), adc(60), false), 101);
        assert_eq!(p.state().direction, Some(Direction::Down));
    }

    #[test]
    fn inverted_axes() {
        let mut p = InputProcessor::new(InputConfig {
            invert_y: true,
            ..InputConfig::default()
        });
        for i in 0..8 {
            p.sample(raw(CENTER, CENTER, false), i);
        }
        p.sample(raw(CENTER, ADC_MAX, false), 10);
        assert_eq!(p.state().direction, Some(Direction::Up));
        assert_eq!(p.state().axis_y, -100);
    }

    #[test]
    fn button_debounce() {
        let mut p = calibrated();
        p.sample(raw(CENTER, CENTER, true), 100);
        p.sample(raw(CENTER, CENTER, true), 105);
        assert!(!p.state().button_held, "10ms 전");
        p.sample(raw(CENTER, CENTER, true), 110);
        assert!(p.state().button_held);
        assert_eq!(p.state().press_count, 1);

        // 짧은 튐은 무시한다.
        p.sample(raw(CENTER, CENTER, false), 120);
        p.sample(raw(CENTER, CENTER, true), 125);
        p.sample(raw(CENTER, CENTER, false), 128);
        assert!(p.state().button_held);
        assert_eq!(p.state().release_count, 0);

        p.sample(raw(CENTER, CENTER, false), 140);
        assert!(!p.state().button_held);
        assert_eq!(p.state().release_count, 1);
    }

    #[test]
    fn button_works_during_calibration() {
        let mut p = InputProcessor::new(InputConfig::default());
        p.sample(raw(CENTER, CENTER, true), 0);
        p.sample(raw(CENTER, CENTER, true), 10);
        assert!(p.state().button_held);
    }

    #[test]
    fn control_state_round_trip_and_size() {
        let state = ControlState {
            direction: Some(Direction::Left),
            axis_x: -100,
            axis_y: 42,
            button_held: true,
            press_count: 255,
            release_count: 3,
        };
        let mut buf = [0; ControlState::WIRE_LEN];
        let mut w = StateWriter::new(&mut buf);
        state.encode(&mut w).unwrap();
        assert_eq!(w.len(), ControlState::WIRE_LEN);
        let decoded = ControlState::decode(&mut StateReader::new(&buf)).unwrap();
        assert_eq!(decoded, state);
    }

    #[test]
    fn control_state_rejects_bad_values() {
        assert!(ControlState::decode(&mut StateReader::new(&[9, 0, 0, 0, 0, 0, 0])).is_err());
        assert!(ControlState::decode(&mut StateReader::new(&[0, 101, 0, 0, 0, 0, 0])).is_err());
        assert!(ControlState::decode(&mut StateReader::new(&[0, 0, 0])).is_err());
    }

    #[test]
    fn events_fire_once_per_change() {
        let mut t = EventTracker::default();
        let mut s = ControlState::default();
        assert_eq!(t.next(s, false), PlayerInput::default());

        s.direction = Some(Direction::Up);
        s.button_held = true;
        s.press_count = 1;
        let first = t.next(s, false);
        assert_eq!(first.direction_pressed, Some(Direction::Up));
        assert!(first.button.pressed && first.button.held);

        // 같은 상태가 이어지면 이벤트 없음(따라잡기 틱 포함).
        let second = t.next(s, false);
        assert_eq!(second.direction_pressed, None);
        assert!(!second.button.pressed && second.button.held);

        s.direction = Some(Direction::Left);
        assert_eq!(t.next(s, false).direction_pressed, Some(Direction::Left));
    }

    #[test]
    fn press_and_release_in_same_tick() {
        let mut t = EventTracker::default();
        t.next(ControlState::default(), false);
        let s = ControlState {
            press_count: 1,
            release_count: 1,
            ..ControlState::default()
        };
        let input = t.next(s, false);
        assert!(input.button.pressed && input.button.released && !input.button.held);
    }

    #[test]
    fn counters_survive_lost_packets_and_wrap() {
        let mut t = EventTracker::default();
        t.reset_baseline(ControlState {
            press_count: 254,
            ..ControlState::default()
        });
        // 중간 패킷이 빠져 두 번 누른 것이 한 번에 보여도 pressed는 참이다.
        let input = t.next(
            ControlState {
                press_count: 0,
                ..ControlState::default()
            },
            false,
        );
        assert!(input.button.pressed);
    }

    #[test]
    fn baseline_suppresses_held_button_at_start() {
        let mut t = EventTracker::default();
        let held = ControlState {
            button_held: true,
            press_count: 5,
            direction: Some(Direction::Right),
            ..ControlState::default()
        };
        t.reset_baseline(held);
        let input = t.next(held, false);
        assert!(input.button.held && !input.button.pressed);
        assert_eq!(input.direction_pressed, None);
    }

    #[test]
    fn direction_helpers() {
        assert_eq!(Direction::Up.delta(), (0, -1));
        assert_eq!(Direction::Right.opposite(), Direction::Left);
        for d in [
            None,
            Some(Direction::Up),
            Some(Direction::Down),
            Some(Direction::Left),
            Some(Direction::Right),
        ] {
            assert_eq!(Direction::from_wire(Direction::to_wire(d)).unwrap(), d);
        }
    }
}
