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

// 기본 프레임 재생 시간 (ms)
const DEFAULT_FRAME_MS: u16 = 100;

// 스프라이트 편집기 그리기 도구 종류
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DrawTool {
    #[default]
    Pencil, // 연필 (픽셀 찍기)
    Eraser,     // 지우개 (검은색/꺼짐으로 지우기)
    Fill,       // 채우기 (Flood Fill)
    Eyedropper, // 스포이드 (클릭한 픽셀 색상 추출)
    Line,       // 직선 (드래그 시작점 -> 끝점)
    Rect,       // 사각형 테두리 (드래그 시작점 -> 끝점)
}

impl DrawTool {
    // 도구 버튼에 표시할 이름 반환
    pub fn label(self) -> &'static str {
        match self {
            Self::Pencil => "연필",
            Self::Eraser => "지우개",
            Self::Fill => "채우기",
            Self::Eyedropper => "스포이드",
            Self::Line => "직선",
            Self::Rect => "사각형",
        }
    }
}

// 애니메이션의 단일 프레임 데이터 (픽셀 배열 + 재생 시간)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpriteFrame {
    pub pixels: Vec<Rgb>, // width * height 크기의 RGB 픽셀
    pub duration_ms: u16, // 이 프레임이 표시되는 시간 (ms)
}

// Undo/Redo를 위해 스프라이트 전체 상태(크기·모든 프레임·선택 인덱스)를 보관하는 스냅샷
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpriteSnapshot {
    pub width: usize,
    pub height: usize,
    pub frames: Vec<SpriteFrame>,
    pub current_frame: usize,
}

// GUI 없이 독립적으로 테스트할 수 있는 캔버스·애니메이션 편집기 상태
pub struct CanvasEditor {
    framebuffer: FrameBuffer, // 현재 활성 프레임의 엔진 프레임버퍼 (미리보기 일치 보장)
    layout: ScreenLayout,     // 현재 내부 레이아웃 (Wide 32x16 / Mirrored 16x16)
    canvas_width: usize,      // 실제 편집 중인 가로 크기 (1~32)
    canvas_height: usize,     // 실제 편집 중인 세로 크기 (1~16)
    frames: Vec<SpriteFrame>, // 애니메이션 프레임 목록 (항상 1개 이상 유지)
    current_frame: usize,     // 현재 편집 중인 프레임 인덱스
    selected_color: Rgb,      // 현재 선택된 브러시 색상
    led_preview: bool,        // 실기기 LED 밝기 미리보기 켜기/끄기
    brightness_percent: u8,   // 실기기 밝기 제한 시뮬레이션 (15~25%)
}

impl Default for CanvasEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl CanvasEditor {
    // 기본값으로 편집기 생성 (32x16, 1프레임, 빨간색 브러시, 밝기 20%)
    pub fn new() -> Self {
        Self {
            framebuffer: FrameBuffer::new(ScreenLayout::Wide),
            layout: ScreenLayout::Wide,
            canvas_width: 32,
            canvas_height: 16,
            frames: vec![SpriteFrame {
                pixels: vec![Rgb::BLACK; 32 * 16],
                duration_ms: DEFAULT_FRAME_MS,
            }],
            current_frame: 0,
            selected_color: PALETTE[1],
            led_preview: false,
            brightness_percent: 20,
        }
    }

    // 현재 캔버스 가로 픽셀 수 반환 (1~32)
    pub fn width(&self) -> usize {
        self.canvas_width
    }

    // 현재 캔버스 세로 픽셀 수 반환 (1~16)
    pub fn height(&self) -> usize {
        self.canvas_height
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

    // 실기기 밝기 미리보기 켜기/끄기 설정
    pub fn set_led_preview(&mut self, enabled: bool) {
        self.led_preview = enabled;
    }

    // 밝기 값을 실기기 범위(15~25%) 안으로 제한해 저장
    pub fn set_brightness_percent(&mut self, percent: u8) {
        self.brightness_percent = percent.clamp(15, 25);
    }

    // 현재 프레임버퍼 내용을 frames[current_frame]에 동기화
    fn sync_to_current_frame(&mut self) {
        let mut pixels = Vec::with_capacity(self.canvas_width * self.canvas_height);
        for y in 0..self.canvas_height {
            for x in 0..self.canvas_width {
                pixels.push(self.pixel(x, y));
            }
        }
        if let Some(frame) = self.frames.get_mut(self.current_frame) {
            frame.pixels = pixels;
        }
    }

    // frames[current_frame]의 픽셀을 엔진 프레임버퍼로 불러오기
    fn load_current_frame_to_fb(&mut self) {
        self.framebuffer.clear();
        let Some(frame) = self.frames.get(self.current_frame) else {
            return;
        };
        for y in 0..self.canvas_height {
            for x in 0..self.canvas_width {
                let idx = y * self.canvas_width + x;
                if let Some(&color) = frame.pixels.get(idx)
                    && let (Ok(ix), Ok(iy)) = (i32::try_from(x), i32::try_from(y))
                {
                    self.framebuffer.set_pixel(ix, iy, color);
                }
            }
        }
    }

    // 전체 프레임 개수 반환
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    // 현재 선택된 프레임 인덱스 반환 (0부터 시작)
    pub fn current_frame_index(&self) -> usize {
        self.current_frame
    }

    // 지정한 프레임의 재생 시간(ms) 조회
    pub fn frame_duration_ms(&self, index: usize) -> u16 {
        self.frames
            .get(index)
            .map(|f| f.duration_ms)
            .unwrap_or(DEFAULT_FRAME_MS)
    }

    // 지정한 프레임의 재생 시간(ms) 변경 (20~5000ms 범위 제한)
    pub fn set_frame_duration_ms(&mut self, index: usize, ms: u16) {
        if let Some(frame) = self.frames.get_mut(index) {
            frame.duration_ms = ms.clamp(20, 5000);
        }
    }

    // 지정한 인덱스의 프레임으로 전환
    pub fn select_frame(&mut self, index: usize) {
        if index < self.frames.len() && index != self.current_frame {
            self.sync_to_current_frame();
            self.current_frame = index;
            self.load_current_frame_to_fb();
        }
    }

    // 현재 프레임 뒤에 빈 프레임 1개를 추가하고 선택
    pub fn add_frame(&mut self) {
        self.sync_to_current_frame();
        let insert_at = self.current_frame + 1;
        self.frames.insert(
            insert_at,
            SpriteFrame {
                pixels: vec![Rgb::BLACK; self.canvas_width * self.canvas_height],
                duration_ms: DEFAULT_FRAME_MS,
            },
        );
        self.current_frame = insert_at;
        self.load_current_frame_to_fb();
    }

    // 현재 프레임을 복제해 바로 뒤에 삽입하고 선택
    pub fn duplicate_frame(&mut self) {
        self.sync_to_current_frame();
        let copy = self.frames[self.current_frame].clone();
        let insert_at = self.current_frame + 1;
        self.frames.insert(insert_at, copy);
        self.current_frame = insert_at;
        self.load_current_frame_to_fb();
    }

    // 지정한 프레임 삭제 (최소 1프레임은 유지)
    pub fn delete_frame(&mut self, index: usize) -> bool {
        if self.frames.len() <= 1 || index >= self.frames.len() {
            return false;
        }
        self.sync_to_current_frame();
        self.frames.remove(index);
        if self.current_frame >= self.frames.len() {
            self.current_frame = self.frames.len() - 1;
        } else if index < self.current_frame {
            self.current_frame -= 1;
        }
        self.load_current_frame_to_fb();
        true
    }

    // 지정한 프레임을 왼쪽(앞)으로 한 칸 이동
    pub fn move_frame_left(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.frames.len() {
            return false;
        }
        self.sync_to_current_frame();
        self.frames.swap(index, index - 1);
        if self.current_frame == index {
            self.current_frame = index - 1;
        } else if self.current_frame == index - 1 {
            self.current_frame = index;
        }
        self.load_current_frame_to_fb();
        true
    }

    // 지정한 프레임을 오른쪽(뒤)으로 한 칸 이동
    pub fn move_frame_right(&mut self, index: usize) -> bool {
        if index + 1 >= self.frames.len() {
            return false;
        }
        self.sync_to_current_frame();
        self.frames.swap(index, index + 1);
        if self.current_frame == index {
            self.current_frame = index + 1;
        } else if self.current_frame == index + 1 {
            self.current_frame = index;
        }
        self.load_current_frame_to_fb();
        true
    }

    // Undo/Redo용 전체 스프라이트 상태 스냅샷 추출
    pub fn snapshot_state(&mut self) -> SpriteSnapshot {
        self.sync_to_current_frame();
        SpriteSnapshot {
            width: self.canvas_width,
            height: self.canvas_height,
            frames: self.frames.clone(),
            current_frame: self.current_frame,
        }
    }

    // Undo/Redo용 전체 스프라이트 상태 스냅샷 복원
    pub fn restore_state(&mut self, state: &SpriteSnapshot) {
        let w = state.width.clamp(1, 32);
        let h = state.height.clamp(1, 16);
        self.layout = if w <= 16 {
            ScreenLayout::Mirrored
        } else {
            ScreenLayout::Wide
        };
        self.framebuffer = FrameBuffer::new(self.layout);
        self.canvas_width = w;
        self.canvas_height = h;
        self.frames = if state.frames.is_empty() {
            vec![SpriteFrame {
                pixels: vec![Rgb::BLACK; w * h],
                duration_ms: DEFAULT_FRAME_MS,
            }]
        } else {
            state.frames.clone()
        };
        self.current_frame = state.current_frame.min(self.frames.len() - 1);
        self.load_current_frame_to_fb();
    }

    // 단일 PNG 이미지를 불러와 1프레임 상태로 초기화
    pub fn load_single_frame(&mut self, width: usize, height: usize, pixels: &[Rgb]) {
        let w = width.clamp(1, 32);
        let h = height.clamp(1, 16);
        self.layout = if w <= 16 {
            ScreenLayout::Mirrored
        } else {
            ScreenLayout::Wide
        };
        self.framebuffer = FrameBuffer::new(self.layout);
        self.canvas_width = w;
        self.canvas_height = h;

        let mut frame_pixels = vec![Rgb::BLACK; w * h];
        for (dst, &src) in frame_pixels.iter_mut().zip(pixels.iter()) {
            *dst = src;
        }
        self.frames = vec![SpriteFrame {
            pixels: frame_pixels,
            duration_ms: DEFAULT_FRAME_MS,
        }];
        self.current_frame = 0;
        self.load_current_frame_to_fb();
    }

    // 화면 모드 변경 시 새 레이아웃 크기의 빈 1프레임으로 초기화
    pub fn set_layout(&mut self, layout: ScreenLayout) {
        self.layout = layout;
        self.framebuffer = FrameBuffer::new(layout);
        self.canvas_width = usize::from(self.framebuffer.width());
        self.canvas_height = usize::from(self.framebuffer.height());
        self.frames = vec![SpriteFrame {
            pixels: vec![Rgb::BLACK; self.canvas_width * self.canvas_height],
            duration_ms: DEFAULT_FRAME_MS,
        }];
        self.current_frame = 0;
    }

    // 캔버스 크기를 1×1 ~ 32×16 범위로 변경 (모든 프레임의 겹치는 픽셀 유지)
    pub fn resize(&mut self, width: usize, height: usize) {
        self.sync_to_current_frame();
        let new_w = width.clamp(1, 32);
        let new_h = height.clamp(1, 16);
        let old_w = self.canvas_width;
        let old_h = self.canvas_height;

        // 모든 프레임을 새 크기에 맞게 조정
        for frame in &mut self.frames {
            let mut resized = vec![Rgb::BLACK; new_w * new_h];
            for y in 0..old_h.min(new_h) {
                for x in 0..old_w.min(new_w) {
                    resized[y * new_w + x] = frame.pixels[y * old_w + x];
                }
            }
            frame.pixels = resized;
        }

        self.layout = if new_w <= 16 {
            ScreenLayout::Mirrored
        } else {
            ScreenLayout::Wide
        };
        self.framebuffer = FrameBuffer::new(self.layout);
        self.canvas_width = new_w;
        self.canvas_height = new_h;
        self.load_current_frame_to_fb();
    }

    // 지정한 좌표(x, y)에 현재 선택된 색으로 픽셀 찍기
    pub fn paint_pixel(&mut self, x: usize, y: usize) {
        if x >= self.canvas_width || y >= self.canvas_height {
            return;
        }
        if let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) {
            self.framebuffer.set_pixel(x, y, self.selected_color);
        }
    }

    // 지정한 좌표(x, y)의 픽셀을 검은색(LED 꺼짐)으로 지우기
    pub fn erase_pixel(&mut self, x: usize, y: usize) {
        if x >= self.canvas_width || y >= self.canvas_height {
            return;
        }
        if let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) {
            self.framebuffer.set_pixel(x, y, Rgb::BLACK);
        }
    }

    // 스포이드: 지정한 좌표의 색을 읽어 현재 브러시 색으로 설정
    pub fn pick_color(&mut self, x: usize, y: usize) -> Rgb {
        let color = self.pixel(x, y);
        self.selected_color = color;
        color
    }

    // 채우기(Flood Fill): 시작점과 같은 색으로 연결된 인접 영역을 현재 색으로 채움
    pub fn flood_fill(&mut self, start_x: usize, start_y: usize) {
        if start_x >= self.canvas_width || start_y >= self.canvas_height {
            return;
        }
        let target = self.pixel(start_x, start_y);
        let replacement = self.selected_color;
        if target == replacement {
            return; // 이미 같은 색이면 무한 루프 방지를 위해 즉시 종료
        }

        let mut stack = vec![(start_x, start_y)];
        while let Some((x, y)) = stack.pop() {
            if self.pixel(x, y) != target {
                continue;
            }
            self.paint_pixel(x, y);
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < self.canvas_width {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < self.canvas_height {
                stack.push((x, y + 1));
            }
        }
    }

    // 직선 그리기 (브레젠험 알고리즘)
    pub fn draw_line(&mut self, x0: usize, y0: usize, x1: usize, y1: usize) {
        let mut x = x0 as isize;
        let mut y = y0 as isize;
        let tx = x1 as isize;
        let ty = y1 as isize;

        let dx = (tx - x).abs();
        let dy = -(ty - y).abs();
        let sx = if x < tx { 1 } else { -1 };
        let sy = if y < ty { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            if x >= 0 && y >= 0 {
                self.paint_pixel(x as usize, y as usize);
            }
            if x == tx && y == ty {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    // 사각형 테두리 그리기
    pub fn draw_rect(&mut self, x0: usize, y0: usize, x1: usize, y1: usize) {
        let min_x = x0.min(x1);
        let max_x = x0.max(x1);
        let min_y = y0.min(y1);
        let max_y = y0.max(y1);

        for x in min_x..=max_x {
            self.paint_pixel(x, min_y);
            self.paint_pixel(x, max_y);
        }
        for y in min_y..=max_y {
            self.paint_pixel(min_x, y);
            self.paint_pixel(max_x, y);
        }
    }

    // 현재 프레임 전체를 검은색으로 초기화
    pub fn clear(&mut self) {
        self.framebuffer.clear();
    }

    // 특정 좌표의 원본 색상 읽기
    pub fn pixel(&self, x: usize, y: usize) -> Rgb {
        if x >= self.canvas_width || y >= self.canvas_height {
            return Rgb::BLACK;
        }
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

// GUI 창 없이 편집·애니메이션 프레임 로직을 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;
    use dotbox_core::display::{PANEL_HEIGHT, PANEL_WIDTH, SCREEN_HEIGHT, SCREEN_WIDTH};

    #[test]
    fn paint_and_erase_pixel() {
        let mut editor = CanvasEditor::new();
        let red = Rgb::new(255, 0, 0);
        editor.set_selected_color(red);

        editor.paint_pixel(5, 3);
        assert_eq!(editor.pixel(5, 3), red);

        editor.erase_pixel(5, 3);
        assert_eq!(editor.pixel(5, 3), Rgb::BLACK);
    }

    #[test]
    fn changing_layout_resizes_and_clears_canvas() {
        let mut editor = CanvasEditor::new();
        assert_eq!(
            (editor.width(), editor.height()),
            (usize::from(SCREEN_WIDTH), usize::from(SCREEN_HEIGHT))
        );

        editor.paint_pixel(1, 1);
        editor.set_layout(ScreenLayout::Mirrored);

        assert_eq!(
            (editor.width(), editor.height()),
            (usize::from(PANEL_WIDTH), usize::from(PANEL_HEIGHT))
        );
        assert_eq!(editor.pixel(1, 1), Rgb::BLACK);
    }

    #[test]
    fn resize_clamps_bounds_and_preserves_pixels() {
        let mut editor = CanvasEditor::new();
        let green = Rgb::new(0, 255, 0);
        editor.set_selected_color(green);
        editor.paint_pixel(2, 3);

        editor.resize(8, 8);
        assert_eq!((editor.width(), editor.height()), (8, 8));
        assert_eq!(editor.pixel(2, 3), green);

        editor.resize(0, 99);
        assert_eq!((editor.width(), editor.height()), (1, 16));
    }

    #[test]
    fn flood_fill_fills_connected_region_only() {
        let mut editor = CanvasEditor::new();
        editor.resize(5, 5);
        let white = Rgb::WHITE;
        let blue = Rgb::new(0, 0, 255);

        editor.set_selected_color(white);
        editor.draw_line(2, 0, 2, 4);

        editor.set_selected_color(blue);
        editor.flood_fill(0, 0);

        assert_eq!(editor.pixel(1, 2), blue);
        assert_eq!(editor.pixel(2, 2), white);
        assert_eq!(editor.pixel(3, 2), Rgb::BLACK);
    }

    #[test]
    fn draw_rect_and_eyedropper_work() {
        let mut editor = CanvasEditor::new();
        editor.resize(6, 6);
        let yellow = Rgb::new(255, 255, 0);
        editor.set_selected_color(yellow);
        editor.draw_rect(1, 1, 4, 4);

        assert_eq!(editor.pixel(1, 1), yellow);
        assert_eq!(editor.pixel(2, 2), Rgb::BLACK);

        editor.set_selected_color(Rgb::WHITE);
        let picked = editor.pick_color(4, 4);
        assert_eq!(picked, yellow);
        assert_eq!(editor.selected_color(), yellow);
    }

    // 애니메이션 프레임 추가·복제·전환·순서 이동·삭제·시간 설정 검증
    #[test]
    fn animation_frames_add_switch_reorder_and_delete() {
        let mut editor = CanvasEditor::new();
        editor.resize(4, 4);
        let red = Rgb::new(255, 0, 0);
        let blue = Rgb::new(0, 0, 255);

        // 0번 프레임에 빨간 점 찍기
        editor.set_selected_color(red);
        editor.paint_pixel(0, 0);
        editor.set_frame_duration_ms(0, 150);

        // 1번 빈 프레임 추가 후 파란 점 찍기
        editor.add_frame();
        assert_eq!(editor.frame_count(), 2);
        assert_eq!(editor.current_frame_index(), 1);
        assert_eq!(editor.pixel(0, 0), Rgb::BLACK);
        editor.set_selected_color(blue);
        editor.paint_pixel(1, 1);

        // 0번 프레임으로 돌아오면 빨간 점과 시간(150ms)이 유지되어야 함
        editor.select_frame(0);
        assert_eq!(editor.pixel(0, 0), red);
        assert_eq!(editor.frame_duration_ms(0), 150);

        // 0번 프레임을 오른쪽(1번)으로 이동
        assert!(editor.move_frame_right(0));
        assert_eq!(editor.current_frame_index(), 1);
        assert_eq!(editor.pixel(0, 0), red);

        // 0번(파란 점) 삭제 후 1프레임만 남으면 추가 삭제는 거부되어야 함
        assert!(editor.delete_frame(0));
        assert_eq!(editor.frame_count(), 1);
        assert!(!editor.delete_frame(0));
    }

    #[test]
    fn led_preview_scales_brightness() {
        let dimmed = scale_brightness(Rgb::new(200, 100, 50), 20);
        assert_eq!(dimmed, Rgb::new(40, 20, 10));
    }
}
