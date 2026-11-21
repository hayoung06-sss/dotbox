use dotbox_core::display::{FrameBuffer, ScreenLayout};
use dotbox_core::hal::Rgb;

// 기본으로 제공하는 8가지 색상 팔레트
pub const PALETTE: [Rgb; 8] = [
    Rgb::WHITE,             // 흰색
    Rgb::new(255, 60, 60),  // 빨강
    Rgb::new(255, 165, 0),  // 주황
    Rgb::new(255, 230, 50), // 노랑
    Rgb::new(60, 220, 90),  // 초록
    Rgb::new(60, 210, 255), // 하늘
    Rgb::new(70, 110, 255), // 파랑
    Rgb::new(210, 80, 255), // 보라
];

// GUI 없이 독립적으로 테스트할 수 있는 캔버스 편집기 상태
pub struct CanvasEditor {
    framebuffer: FrameBuffer, // 엔진 프레임버퍼 (미리보기 일치 보장)
    layout: ScreenLayout,     // 현재 화면 모드 (Wide 32x16 / Mirrored 16x16)
    selected_color: Rgb,      // 현재 선택된 브러시 색상
    led_preview: bool,        // 실기기 LED 밝기 미리보기 켜기/끄기
    brightness_percent: u8,   // 실기기 밝기 제한 시뮬레이션 (15~25%)
}

impl Default for CanvasEditor {
    // 기본값 생성 시 new() 호출
    fn default() -> Self {
        Self::new()
    }
}

impl CanvasEditor {
    // 기본값으로 편집기 생성 (32x16, 빨간색 브러시, 밝기 20%)
    pub fn new() -> Self {
        Self {
            framebuffer: FrameBuffer::new(ScreenLayout::Wide),
            layout: ScreenLayout::Wide,
            selected_color: PALETTE[1],
            led_preview: false,
            brightness_percent: 20,
        }
    }

    // 현재 캔버스 가로 픽셀 수 반환 (u16 → usize 변환)
    pub fn width(&self) -> usize {
        usize::from(self.framebuffer.width())
    }

    // 현재 캔버스 세로 픽셀 수 반환 (u16 → usize 변환)
    pub fn height(&self) -> usize {
        usize::from(self.framebuffer.height())
    }

    // 현재 화면 레이아웃 모드 반환 (Wide / Mirrored)
    pub fn layout(&self) -> ScreenLayout {
        self.layout
    }

    // 현재 선택된 브러시 색상 반환
    pub fn selected_color(&self) -> Rgb {
        self.selected_color
    }

    // 브러시 색상 변경
    pub fn set_selected_color(&mut self, color: Rgb) {
        self.selected_color = color;
    }

    // 실기기 밝기 미리보기 활성화 여부 반환
    pub fn led_preview(&self) -> bool {
        self.led_preview
    }

    // 실기기 밝기 미리보기 켜기/끄기 설정
    pub fn set_led_preview(&mut self, enabled: bool) {
        self.led_preview = enabled;
    }

    // 현재 설정된 실기기 밝기 비율(%) 반환
    pub fn brightness_percent(&self) -> u8 {
        self.brightness_percent
    }

    // 밝기 값을 실기기 범위(15~25%) 안으로 제한해 저장
    pub fn set_brightness_percent(&mut self, percent: u8) {
        self.brightness_percent = percent.clamp(15, 25);
    }

    // 화면 모드 변경 시 새 레이아웃 크기로 프레임버퍼 초기화
    pub fn set_layout(&mut self, layout: ScreenLayout) {
        self.layout = layout;
        self.framebuffer = FrameBuffer::new(layout);
    }

    // 지정한 좌표(x, y)에 현재 선택된 색으로 픽셀 찍기
    pub fn paint_pixel(&mut self, x: usize, y: usize) {
        if let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) {
            self.framebuffer.set_pixel(x, y, self.selected_color);
        }
    }

    // 지정한 좌표(x, y)의 픽셀을 검은색(LED 꺼짐)으로 지우기
    pub fn erase_pixel(&mut self, x: usize, y: usize) {
        if let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) {
            self.framebuffer.set_pixel(x, y, Rgb::BLACK);
        }
    }

    // 캔버스 전체를 검은색으로 초기화
    pub fn clear(&mut self) {
        self.framebuffer.clear();
    }

    // 특정 좌표의 원본 색상 읽기
    pub fn pixel(&self, x: usize, y: usize) -> Rgb {
        let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) else {
            return Rgb::BLACK;
        };
        self.framebuffer.pixel(x, y).unwrap_or(Rgb::BLACK)
    }

    // 화면에 실제로 표시할 색상 계산 (실기기 미리보기 모드면 밝기 비율 적용)
    pub fn display_color(&self, x: usize, y: usize) -> Rgb {
        let raw = self.pixel(x, y);
        if !self.led_preview {
            return raw;
        }
        scale_brightness(raw, self.brightness_percent)
    }
}

// RGB 각 채널에 밝기 비율(15~25%)을 곱해 실기기 LED 느낌을 흉내내는 순수 함수
pub fn scale_brightness(color: Rgb, percent: u8) -> Rgb {
    let scale = u16::from(percent.clamp(15, 25));
    Rgb::new(
        ((u16::from(color.r) * scale) / 100) as u8,
        ((u16::from(color.g) * scale) / 100) as u8,
        ((u16::from(color.b) * scale) / 100) as u8,
    )
}

// GUI 창 없이 편집 로직만 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;
    // 엔진에 정의된 화면·패널 표준 크기 상수(32, 16) 불러오기
    use dotbox_core::display::{PANEL_HEIGHT, PANEL_WIDTH, SCREEN_HEIGHT, SCREEN_WIDTH};

    // 픽셀 찍기와 지우기가 정상 동작하는지 검증
    #[test]
    fn paint_and_erase_pixel() {
        let mut editor = CanvasEditor::new(); // 기본 편집기 생성
        let red = Rgb::new(255, 0, 0); // 테스트용 빨간색 정의
        editor.set_selected_color(red); // 브러시 색을 빨간색으로 변경

        editor.paint_pixel(5, 3); // (5, 3) 좌표에 빨간 픽셀 찍기
        assert_eq!(editor.pixel(5, 3), red); // 실제로 빨간색이 저장됐는지 확인

        editor.erase_pixel(5, 3); // (5, 3) 좌표 픽셀 지우기
        assert_eq!(editor.pixel(5, 3), Rgb::BLACK); // 검은색(꺼짐)으로 돌아왔는지 확인
    }

    // 화면 모드(Wide → Mirrored) 전환 시 크기 변경과 초기화를 검증
    #[test]
    fn changing_layout_resizes_and_clears_canvas() {
        let mut editor = CanvasEditor::new();
        // 초기 모드는 Wide(32x16)인지 확인
        assert_eq!(
            (editor.width(), editor.height()),
            (usize::from(SCREEN_WIDTH), usize::from(SCREEN_HEIGHT))
        );

        editor.paint_pixel(1, 1); // 임시로 (1, 1)에 점 하나 찍기
        editor.set_layout(ScreenLayout::Mirrored); // Mirrored(16x16) 모드로 전환

        // 크기가 16x16으로 줄고, 기존에 찍었던 픽셀이 초기화됐는지 확인
        assert_eq!(
            (editor.width(), editor.height()),
            (usize::from(PANEL_WIDTH), usize::from(PANEL_HEIGHT))
        );
        assert_eq!(editor.pixel(1, 1), Rgb::BLACK);
    }

    // 실기기 밝기 제한(15~25%) 계산이 정확한지 검증
    #[test]
    fn led_preview_scales_brightness() {
        // RGB(200, 100, 50)에 20% 밝기를 적용하면 (40, 20, 10)이 되어야 함
        let dimmed = scale_brightness(Rgb::new(200, 100, 50), 20);
        assert_eq!(dimmed, Rgb::new(40, 20, 10));
    }
}
