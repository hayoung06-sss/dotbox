//! 두 패널(16×16씩)을 창 버퍼로 그린다.

use dotbox_core::display::{PANEL_HEIGHT, PANEL_PIXELS, PANEL_WIDTH};
use dotbox_core::hal::Rgb;

/// LED 한 칸의 크기(px).
const CELL: usize = 24;
/// LED 사이 틈(px). LED를 둥글게 보이지는 않지만 칸이 구분된다.
const LED_GAP: usize = 3;
/// 두 패널 사이 간격(px). 두 모듈이 붙은 이음매를 나타낸다.
const PANEL_GAP: usize = 16;
const MARGIN: usize = 16;

const PANEL_PX_W: usize = PANEL_WIDTH as usize * CELL;
const PANEL_PX_H: usize = PANEL_HEIGHT as usize * CELL;

pub const WINDOW_WIDTH: usize = MARGIN * 2 + PANEL_PX_W * 2 + PANEL_GAP;
pub const WINDOW_HEIGHT: usize = MARGIN * 2 + PANEL_PX_H;

const BACKGROUND: u32 = 0x0010_1014;
/// 꺼진 LED. 실제 패널처럼 어둡게 보이게 한다.
const LED_OFF: u32 = 0x001C_1C22;

fn to_u32(color: Rgb) -> u32 {
    (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)
}

/// 왼쪽·오른쪽 패널을 `out`(크기 `WINDOW_WIDTH * WINDOW_HEIGHT`, 0RGB)에 그린다.
pub fn compose(left: &[Rgb; PANEL_PIXELS], right: &[Rgb; PANEL_PIXELS], out: &mut [u32]) {
    assert_eq!(out.len(), WINDOW_WIDTH * WINDOW_HEIGHT, "창 버퍼 크기");
    out.fill(BACKGROUND);
    for (panel_index, panel) in [left, right].into_iter().enumerate() {
        let origin_x = MARGIN + panel_index * (PANEL_PX_W + PANEL_GAP);
        for (i, &pixel) in panel.iter().enumerate() {
            let led_x = i % PANEL_WIDTH as usize;
            let led_y = i / PANEL_WIDTH as usize;
            let color = if pixel == Rgb::BLACK {
                LED_OFF
            } else {
                to_u32(pixel)
            };
            let x0 = origin_x + led_x * CELL + LED_GAP;
            let y0 = MARGIN + led_y * CELL + LED_GAP;
            for y in y0..y0 + CELL - 2 * LED_GAP {
                out[y * WINDOW_WIDTH + x0..y * WINDOW_WIDTH + x0 + CELL - 2 * LED_GAP].fill(color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn center_of(panel: usize, led_x: usize, led_y: usize) -> usize {
        let x = MARGIN + panel * (PANEL_PX_W + PANEL_GAP) + led_x * CELL + CELL / 2;
        let y = MARGIN + led_y * CELL + CELL / 2;
        y * WINDOW_WIDTH + x
    }

    #[test]
    fn draws_each_panel_in_place() {
        let mut left = [Rgb::BLACK; PANEL_PIXELS];
        let mut right = [Rgb::BLACK; PANEL_PIXELS];
        left[0] = Rgb::new(255, 0, 0);
        right[PANEL_PIXELS - 1] = Rgb::new(0, 0, 255);
        let mut out = vec![0; WINDOW_WIDTH * WINDOW_HEIGHT];
        compose(&left, &right, &mut out);

        assert_eq!(out[center_of(0, 0, 0)], 0x00FF_0000);
        assert_eq!(out[center_of(1, 15, 15)], 0x0000_00FF);
        assert_eq!(out[center_of(0, 1, 0)], LED_OFF);
        assert_eq!(out[center_of(1, 0, 0)], LED_OFF);
        assert_eq!(out[0], BACKGROUND);
    }

    #[test]
    fn panels_are_separated_by_gap() {
        let left = [Rgb::WHITE; PANEL_PIXELS];
        let right = [Rgb::WHITE; PANEL_PIXELS];
        let mut out = vec![0; WINDOW_WIDTH * WINDOW_HEIGHT];
        compose(&left, &right, &mut out);
        let gap_x = MARGIN + PANEL_PX_W + PANEL_GAP / 2;
        let y = WINDOW_HEIGHT / 2;
        assert_eq!(out[y * WINDOW_WIDTH + gap_x], BACKGROUND);
    }
}
