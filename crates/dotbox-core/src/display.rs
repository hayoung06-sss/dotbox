//! 화면 좌표계와 프레임버퍼.
//!
//! 논리 좌표계는 모듈의 합체·분리 여부와 관계없이 고정이다. 분리 상태를 모르는 게임도 그대로 동작한다.
//!
//! - [`ScreenLayout::Wide`] (기본): 32×16 화면 하나. x 0~15는 왼쪽 패널, 16~31은 오른쪽 패널에 나온다.
//! - [`ScreenLayout::Mirrored`]: 16×16 화면 하나를 두 패널에 똑같이 보여 준다.

use crate::hal::Rgb;
use crate::{Error, Result};

/// 패널 하나(모듈 하나)의 폭.
pub const PANEL_WIDTH: u16 = 16;
/// 패널 하나의 높이.
pub const PANEL_HEIGHT: u16 = 16;
/// 패널 하나의 픽셀 수.
pub const PANEL_PIXELS: usize = PANEL_WIDTH as usize * PANEL_HEIGHT as usize;
/// 두 패널을 이어 붙인 전체 화면의 폭.
pub const SCREEN_WIDTH: u16 = PANEL_WIDTH * 2;
/// 전체 화면의 높이.
pub const SCREEN_HEIGHT: u16 = PANEL_HEIGHT;

const BUFFER_PIXELS: usize = SCREEN_WIDTH as usize * SCREEN_HEIGHT as usize;

/// 모듈 위치.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// 왼쪽 모듈(마스터).
    Left,
    /// 오른쪽 모듈(슬레이브).
    Right,
}

/// 게임이 그리는 논리 화면과 두 패널의 대응 방식.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScreenLayout {
    /// 32×16 화면 하나를 두 패널에 나눠 보여 준다.
    #[default]
    Wide,
    /// 16×16 화면 하나를 두 패널에 똑같이 보여 준다.
    Mirrored,
}

impl ScreenLayout {
    /// 논리 화면의 폭.
    pub const fn width(self) -> u16 {
        match self {
            Self::Wide => SCREEN_WIDTH,
            Self::Mirrored => PANEL_WIDTH,
        }
    }

    /// 논리 화면의 높이.
    pub const fn height(self) -> u16 {
        SCREEN_HEIGHT
    }

    /// `side` 패널이 보여 줄 논리 화면의 시작 x.
    const fn panel_origin_x(self, side: Side) -> u16 {
        match (self, side) {
            (Self::Wide, Side::Right) => PANEL_WIDTH,
            _ => 0,
        }
    }
}

/// 게임이 그리는 프레임. 크기가 고정이라 힙 할당이 없다.
#[derive(Debug, Clone)]
pub struct FrameBuffer {
    layout: ScreenLayout,
    pixels: [Rgb; BUFFER_PIXELS],
}

impl FrameBuffer {
    pub fn new(layout: ScreenLayout) -> Self {
        Self {
            layout,
            pixels: [Rgb::BLACK; BUFFER_PIXELS],
        }
    }

    pub fn layout(&self) -> ScreenLayout {
        self.layout
    }

    /// 레이아웃을 바꾸고 화면을 비운다.
    pub fn set_layout(&mut self, layout: ScreenLayout) {
        self.layout = layout;
        self.clear();
    }

    pub fn width(&self) -> u16 {
        self.layout.width()
    }

    pub fn height(&self) -> u16 {
        self.layout.height()
    }

    pub fn clear(&mut self) {
        self.fill(Rgb::BLACK);
    }

    pub fn fill(&mut self, color: Rgb) {
        self.pixels.fill(color);
    }

    /// 픽셀을 칠한다. 화면 밖 좌표(음수 포함)는 무시한다.
    pub fn set_pixel(&mut self, x: i32, y: i32, color: Rgb) {
        if let Some(index) = self.index(x, y) {
            self.pixels[index] = color;
        }
    }

    /// 픽셀 색을 돌려준다. 화면 밖이면 `None`.
    pub fn pixel(&self, x: i32, y: i32) -> Option<Rgb> {
        self.index(x, y).map(|index| self.pixels[index])
    }

    /// `side` 패널에 보낼 16×16 픽셀을 `out`에 행 우선 순서로 복사한다.
    ///
    /// `out`의 길이는 [`PANEL_PIXELS`]여야 한다.
    pub fn copy_panel(&self, side: Side, out: &mut [Rgb]) -> Result<()> {
        if out.len() != PANEL_PIXELS {
            return Err(Error::InvalidArgument);
        }
        let origin_x = usize::from(self.layout.panel_origin_x(side));
        let panel_width = usize::from(PANEL_WIDTH);
        for (y, row) in out.chunks_exact_mut(panel_width).enumerate() {
            let start = y * usize::from(SCREEN_WIDTH) + origin_x;
            row.copy_from_slice(&self.pixels[start..start + panel_width]);
        }
        Ok(())
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let x = u16::try_from(x).ok().filter(|&x| x < self.width())?;
        let y = u16::try_from(y).ok().filter(|&y| y < self.height())?;
        Some(usize::from(y) * usize::from(SCREEN_WIDTH) + usize::from(x))
    }
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new(ScreenLayout::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgb = Rgb::new(255, 0, 0);

    fn panel(frame: &FrameBuffer, side: Side) -> [Rgb; PANEL_PIXELS] {
        let mut out = [Rgb::BLACK; PANEL_PIXELS];
        frame.copy_panel(side, &mut out).unwrap();
        out
    }

    #[test]
    fn wide_layout_is_32_by_16() {
        let frame = FrameBuffer::new(ScreenLayout::Wide);
        assert_eq!((frame.width(), frame.height()), (32, 16));
    }

    #[test]
    fn mirrored_layout_is_16_by_16() {
        let frame = FrameBuffer::new(ScreenLayout::Mirrored);
        assert_eq!((frame.width(), frame.height()), (16, 16));
    }

    #[test]
    fn out_of_range_pixels_are_ignored() {
        let mut frame = FrameBuffer::default();
        for (x, y) in [(-1, 0), (0, -1), (32, 0), (0, 16), (i32::MIN, i32::MAX)] {
            frame.set_pixel(x, y, RED);
            assert_eq!(frame.pixel(x, y), None);
        }
        assert!(panel(&frame, Side::Left).iter().all(|&p| p == Rgb::BLACK));
        assert!(panel(&frame, Side::Right).iter().all(|&p| p == Rgb::BLACK));
    }

    #[test]
    fn wide_layout_splits_at_x_16() {
        let mut frame = FrameBuffer::new(ScreenLayout::Wide);
        frame.set_pixel(0, 0, RED);
        frame.set_pixel(15, 15, RED);
        frame.set_pixel(16, 0, RED);
        frame.set_pixel(31, 15, RED);

        let left = panel(&frame, Side::Left);
        let right = panel(&frame, Side::Right);
        assert_eq!(left[0], RED);
        assert_eq!(left[PANEL_PIXELS - 1], RED);
        assert_eq!(right[0], RED);
        assert_eq!(right[PANEL_PIXELS - 1], RED);
        assert_eq!(left.iter().filter(|&&p| p == RED).count(), 2);
        assert_eq!(right.iter().filter(|&&p| p == RED).count(), 2);
    }

    #[test]
    fn mirrored_layout_shows_same_image_on_both_panels() {
        let mut frame = FrameBuffer::new(ScreenLayout::Mirrored);
        frame.set_pixel(3, 7, RED);
        frame.set_pixel(16, 0, RED); // 16×16 밖이라 무시된다.

        let left = panel(&frame, Side::Left);
        assert_eq!(left, panel(&frame, Side::Right));
        assert_eq!(left[7 * 16 + 3], RED);
        assert_eq!(left.iter().filter(|&&p| p == RED).count(), 1);
    }

    #[test]
    fn set_layout_clears_frame() {
        let mut frame = FrameBuffer::new(ScreenLayout::Wide);
        frame.set_pixel(20, 5, RED);
        frame.set_layout(ScreenLayout::Mirrored);
        frame.set_layout(ScreenLayout::Wide);
        assert_eq!(frame.pixel(20, 5), Some(Rgb::BLACK));
    }

    #[test]
    fn copy_panel_rejects_wrong_length() {
        let frame = FrameBuffer::default();
        let mut out = [Rgb::BLACK; 10];
        assert_eq!(
            frame.copy_panel(Side::Left, &mut out),
            Err(Error::InvalidArgument)
        );
    }
}
