use crate::font::FontConverter;
use dotbox_core::display::{FrameBuffer, ScreenLayout};
use dotbox_core::hal::Rgb;

// 장면에 배치할 수 있는 요소 종류 (스프라이트 또는 텍스트)
#[derive(Clone, Debug, PartialEq)]
pub enum SceneItemKind {
    Sprite {
        name: String,            // 표시용 스프라이트 이름
        width: usize,            // 스프라이트 가로 크기 (px)
        height: usize,           // 스프라이트 세로 크기 (px)
        pixels: Vec<Rgb>,        // width * height 크기의 RGB 픽셀 배열
        transparent_black: bool, // true면 검은색(0,0,0) 픽셀을 투명으로 건너뜀
    },
    Text {
        text: String,    // 화면에 표시할 문구
        color: Rgb,      // 글자 색상
        pixel_size: f32, // 글꼴 변환기 사용 시 픽셀 크기
        threshold: u8,   // 글꼴 변환기 사용 시 이진화 임계값
    },
}

// 32×16 장면 미리보기 위에 배치되는 단일 레이어 항목
#[derive(Clone, Debug, PartialEq)]
pub struct SceneItem {
    pub x: i32,              // 좌상단 X 좌표 (화면 밖 음수·양수 허용)
    pub y: i32,              // 좌상단 Y 좌표
    pub visible: bool,       // 화면 표시 여부
    pub kind: SceneItemKind, // 스프라이트 또는 텍스트 데이터
}

impl SceneItem {
    // UI 목록에 표시할 짧은 요약 이름 반환
    pub fn display_label(&self) -> String {
        match &self.kind {
            SceneItemKind::Sprite {
                name,
                width,
                height,
                ..
            } => format!("[스프라이트] {name} ({width}×{height})"),
            SceneItemKind::Text { text, .. } => format!("[텍스트] \"{text}\""),
        }
    }
}

// 파일로 저장하지 않는 32×16 임시 장면(화면) 미리보기 상태
#[derive(Clone, Debug)]
pub struct ScenePreview {
    pub background: Rgb,       // 배경 채우기 색상 (기본 검은색)
    pub items: Vec<SceneItem>, // 아래(0번)부터 위(마지막) 순서로 그릴 레이어 목록
}

impl Default for ScenePreview {
    fn default() -> Self {
        Self::new()
    }
}

impl ScenePreview {
    // 빈 검은색 32×16 장면 생성
    pub fn new() -> Self {
        Self {
            background: Rgb::BLACK,
            items: Vec::new(),
        }
    }

    // 스프라이트 항목 추가 후 새 인덱스 반환
    pub fn add_sprite(
        &mut self,
        name: String,
        width: usize,
        height: usize,
        pixels: Vec<Rgb>,
        x: i32,
        y: i32,
    ) -> usize {
        self.items.push(SceneItem {
            x,
            y,
            visible: true,
            kind: SceneItemKind::Sprite {
                name,
                width,
                height,
                pixels,
                transparent_black: true,
            },
        });
        self.items.len() - 1
    }

    // 텍스트 항목 추가 후 새 인덱스 반환
    pub fn add_text(&mut self, text: String, x: i32, y: i32, color: Rgb) -> usize {
        self.items.push(SceneItem {
            x,
            y,
            visible: true,
            kind: SceneItemKind::Text {
                text,
                color,
                pixel_size: 8.0,
                threshold: 128,
            },
        });
        self.items.len() - 1
    }

    // 지정한 인덱스의 항목 삭제
    pub fn remove_item(&mut self, index: usize) -> bool {
        if index < self.items.len() {
            self.items.remove(index);
            true
        } else {
            false
        }
    }

    // 항목을 한 칸 앞(아래 레이어)으로 이동
    pub fn move_up(&mut self, index: usize) -> bool {
        if index > 0 && index < self.items.len() {
            self.items.swap(index, index - 1);
            true
        } else {
            false
        }
    }

    // 항목을 한 칸 뒤(위 레이어)로 이동
    pub fn move_down(&mut self, index: usize) -> bool {
        if index + 1 < self.items.len() {
            self.items.swap(index, index + 1);
            true
        } else {
            false
        }
    }

    // 모든 배치 항목 초기화
    pub fn clear(&mut self) {
        self.items.clear();
    }

    // 현재 장면의 모든 보이는 레이어를 dotbox-core FrameBuffer(32×16)로 합성 렌더링
    // font가 Some이면 해당 TTF/OTF 변환기로, None이면 내장 3×5 미니 픽셀 폰트로 텍스트를 그림
    pub fn render(&self, font: Option<&FontConverter>) -> FrameBuffer {
        let mut fb = FrameBuffer::new(ScreenLayout::Wide);
        let max_w = i32::from(fb.width());
        let max_h = i32::from(fb.height());

        // 1) 배경색 채우기
        if self.background != Rgb::BLACK {
            for y in 0..max_h {
                for x in 0..max_w {
                    fb.set_pixel(x, y, self.background);
                }
            }
        }

        // 2) 레이어 순서대로 스프라이트·텍스트 합성
        for item in &self.items {
            if !item.visible {
                continue;
            }
            match &item.kind {
                SceneItemKind::Sprite {
                    width,
                    height,
                    pixels,
                    transparent_black,
                    ..
                } => {
                    for sy in 0..*height {
                        for sx in 0..*width {
                            let idx = sy * (*width) + sx;
                            let Some(&px) = pixels.get(idx) else {
                                continue;
                            };
                            if *transparent_black && px == Rgb::BLACK {
                                continue;
                            }
                            let dx = item.x + sx as i32;
                            let dy = item.y + sy as i32;
                            if dx >= 0 && dx < max_w && dy >= 0 && dy < max_h {
                                fb.set_pixel(dx, dy, px);
                            }
                        }
                    }
                }
                SceneItemKind::Text {
                    text,
                    color,
                    pixel_size,
                    threshold,
                } => {
                    if let Some(converter) = font {
                        let style = TextRenderStyle {
                            pixel_size: *pixel_size,
                            threshold: *threshold,
                            color: *color,
                        };
                        draw_text_with_converter(&mut fb, converter, text, (item.x, item.y), style);
                    } else {
                        draw_text_builtin_3x5(&mut fb, text, item.x, item.y, *color);
                    }
                }
            }
        }

        fb
    }
}

// TTF/OTF 텍스트 렌더링 옵션 묶음
struct TextRenderStyle {
    pixel_size: f32,
    threshold: u8,
    color: Rgb,
}

// 불러온 TTF/OTF FontConverter를 이용해 지정 좌표(x, y)에 텍스트 그리기
fn draw_text_with_converter(
    fb: &mut FrameBuffer,
    converter: &FontConverter,
    text: &str,
    origin: (i32, i32),
    style: TextRenderStyle,
) {
    let max_w = i32::from(fb.width());
    let max_h = i32::from(fb.height());
    let (mut cursor_x, start_y) = origin;
    let baseline_y = start_y + style.pixel_size.round() as i32;

    for ch in text.chars() {
        let glyph = converter.rasterize_char(ch, style.pixel_size, style.threshold);
        let offset_y = baseline_y - (glyph.y_offset + glyph.height as i32);

        for gy in 0..glyph.height {
            for gx in 0..glyph.width {
                if glyph.pixel(gx, gy) {
                    let dx = cursor_x + gx as i32;
                    let dy = offset_y + gy as i32;
                    if dx >= 0 && dx < max_w && dy >= 0 && dy < max_h {
                        fb.set_pixel(dx, dy, style.color);
                    }
                }
            }
        }
        cursor_x += glyph.advance_x as i32 + 1;
    }
}

// TTF 폰트를 불러오지 않았을 때도 숫자·영문을 즉시 배치할 수 있는 내장 3×5 미니 픽셀 폰트
fn draw_text_builtin_3x5(fb: &mut FrameBuffer, text: &str, start_x: i32, start_y: i32, color: Rgb) {
    let max_w = i32::from(fb.width());
    let max_h = i32::from(fb.height());
    let mut cursor_x = start_x;

    for ch in text.chars() {
        let rows = builtin_glyph_3x5(ch.to_ascii_uppercase());
        for (ry, &bits) in rows.iter().enumerate() {
            for rx in 0..3 {
                if (bits & (1 << (2 - rx))) != 0 {
                    let dx = cursor_x + rx;
                    let dy = start_y + ry as i32;
                    if dx >= 0 && dx < max_w && dy >= 0 && dy < max_h {
                        fb.set_pixel(dx, dy, color);
                    }
                }
            }
        }
        cursor_x += 4; // 3px 글자 + 1px 간격
    }
}

// 3×5 비트맵 글리프 테이블 (각 u8의 하위 3비트가 한 행의 3픽셀)
fn builtin_glyph_3x5(ch: char) -> [u8; 5] {
    match ch {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b111, 0b100, 0b100, 0b100, 0b111],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b111, 0b100, 0b101, 0b101, 0b111],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b111],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b111, 0b101, 0b101, 0b101, 0b101],
        'O' => [0b111, 0b101, 0b101, 0b101, 0b111],
        'P' => [0b111, 0b101, 0b111, 0b100, 0b100],
        'Q' => [0b111, 0b101, 0b101, 0b111, 0b001],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b111, 0b100, 0b111, 0b001, 0b111],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        ':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '!' => [0b010, 0b010, 0b010, 0b000, 0b010],
        ' ' => [0b000, 0b000, 0b000, 0b000, 0b000],
        _ => [0b111, 0b101, 0b101, 0b101, 0b111],
    }
}

// GUI 없이 장면 합성·투명색·레이어 순서·클리핑을 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;

    // 스프라이트 합성 시 검은색 투명 처리와 레이어 덮어쓰기 검증
    #[test]
    fn renders_sprites_with_transparency_and_layer_order() {
        let mut scene = ScenePreview::new();
        let bg = Rgb::new(10, 20, 30);
        let red = Rgb::new(255, 0, 0);
        let green = Rgb::new(0, 255, 0);
        scene.background = bg;

        // 0번 레이어: (2, 2)에 2×1 [빨강, 검정] 배치
        scene.add_sprite("hero".to_string(), 2, 1, vec![red, Rgb::BLACK], 2, 2);
        // 1번 레이어: (2, 2)에 1×1 [초록] 겹쳐 배치
        scene.add_sprite("hat".to_string(), 1, 1, vec![green], 2, 2);

        let fb = scene.render(None);
        assert_eq!(fb.pixel(2, 2), Some(green)); // 위 레이어(초록)가 덮음
        assert_eq!(fb.pixel(3, 2), Some(bg)); // 검정 픽셀은 투명 처리되어 배경색 유지

        // 레이어 순서를 바꾸면 빨강이 위로 올라와야 함
        assert!(scene.move_up(1));
        let fb2 = scene.render(None);
        assert_eq!(fb2.pixel(2, 2), Some(red));
    }

    // 내장 3×5 폰트 렌더링 및 화면 밖 좌표 안전 클리핑 검증
    #[test]
    fn renders_builtin_text_and_clips_out_of_bounds() {
        let mut scene = ScenePreview::new();
        let white = Rgb::WHITE;
        scene.add_text("1".to_string(), 0, 0, white);
        // 화면 경계 밖(-1, -1)에 배치해도 패닉 없이 잘려서 그려져야 함
        scene.add_sprite("edge".to_string(), 2, 2, vec![white; 4], -1, -1);

        let fb = scene.render(None);
        assert_eq!(fb.pixel(1, 0), Some(white)); // '1'의 상단 중앙 픽셀
        assert_eq!(fb.pixel(0, 0), Some(white)); // (-1,-1) 2×2의 우하단 픽셀이 (0,0)에 표시
    }
}
