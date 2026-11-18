//! 키보드 → 조이스틱 원시 값.

use dotbox_core::hal::RawInput;
use dotbox_core::input::ADC_MAX;
use minifb::Key;

const CENTER: u16 = 2048;

/// 한 모듈에 대응하는 키 상태.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyState {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub button: bool,
}

/// 키 상태를 조이스틱 원시 값으로 바꾼다. 반대 방향을 함께 누르면 그 축은 중앙이다.
pub fn raw_input(keys: KeyState) -> RawInput {
    let axis = |negative: bool, positive: bool| match (negative, positive) {
        (true, false) => 0,
        (false, true) => ADC_MAX,
        _ => CENTER,
    };
    RawInput {
        x: axis(keys.left, keys.right),
        y: axis(keys.up, keys.down),
        button_down: keys.button,
    }
}

/// 왼쪽 모듈(P1): W A S D + Space.
pub(crate) fn left_keys(is_down: impl Fn(Key) -> bool) -> KeyState {
    KeyState {
        up: is_down(Key::W),
        down: is_down(Key::S),
        left: is_down(Key::A),
        right: is_down(Key::D),
        button: is_down(Key::Space),
    }
}

/// 오른쪽 모듈(P2): 방향키 + Enter.
pub(crate) fn right_keys(is_down: impl Fn(Key) -> bool) -> KeyState {
    KeyState {
        up: is_down(Key::Up),
        down: is_down(Key::Down),
        left: is_down(Key::Left),
        right: is_down(Key::Right),
        button: is_down(Key::Enter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_is_center() {
        assert_eq!(
            raw_input(KeyState::default()),
            RawInput {
                x: CENTER,
                y: CENTER,
                button_down: false
            }
        );
    }

    #[test]
    fn directions_map_to_extremes() {
        let up_right = raw_input(KeyState {
            up: true,
            right: true,
            ..KeyState::default()
        });
        assert_eq!((up_right.x, up_right.y), (ADC_MAX, 0));
        let down_left = raw_input(KeyState {
            down: true,
            left: true,
            button: true,
            ..KeyState::default()
        });
        assert_eq!(
            (down_left.x, down_left.y, down_left.button_down),
            (0, ADC_MAX, true)
        );
    }

    #[test]
    fn opposite_keys_cancel() {
        let raw = raw_input(KeyState {
            left: true,
            right: true,
            ..KeyState::default()
        });
        assert_eq!(raw.x, CENTER);
    }

    #[test]
    fn key_layout() {
        let p1 = left_keys(|k| matches!(k, Key::W | Key::Space));
        assert!(p1.up && p1.button && !p1.down);
        let p2 = right_keys(|k| matches!(k, Key::Right | Key::Enter));
        assert!(p2.right && p2.button && !p2.left);
        // P1 키는 P2에 영향이 없다.
        assert_eq!(right_keys(|k| k == Key::W), KeyState::default());
    }
}
