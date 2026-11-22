use dotbox_core::display::{FrameBuffer, ScreenLayout};
use dotbox_core::hal::Rgb;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

// 글꼴 변환 설정값 (픽셀 크기, 이진화 임계값, 포함할 문자 집합)
#[derive(Clone, Debug)]
pub struct FontConfig {
    pub pixel_size: f32, // 글자 높이(px), 32x16 화면 기준 보통 8~16px
    pub threshold: u8,   // 이진화 임계값 (이 값 이상이면 픽셀 켜짐)
    pub charset: String, // 변환할 문자 집합 (한글은 필요한 음절만 입력)
}

impl Default for FontConfig {
    // 기본 글꼴 설정값 반환
    fn default() -> Self {
        Self {
            pixel_size: 12.0, // 기본 글자 크기 12px
            threshold: 128,   // 중간 밝기(128)를 기준으로 흑백 분리
            charset: "DotBox 0123456789 안녕".to_string(),
        }
    }
}

// 1비트(켜짐/꺼짐)로 변환된 단일 글자 비트맵
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitmapGlyph {
    pub ch: char,         // 해당 문자
    pub width: usize,     // 비트맵 가로 크기(px)
    pub height: usize,    // 비트맵 세로 크기(px)
    pub advance_x: usize, // 다음 글자까지 이동할 가로 간격(px)
    pub y_offset: i32,    // 기준선(baseline)으로부터의 Y 오프셋 (위로 갈수록 양수)
    pub bits: Vec<bool>,  // 1비트 픽셀 배열 (길이 = width * height)
}

impl BitmapGlyph {
    // (x, y) 좌표의 픽셀이 켜져 있는지 조회
    pub fn pixel(&self, x: usize, y: usize) -> bool {
        // 비트맵 범위를 벗어나면 안전하게 꺼짐(false) 반환
        if x >= self.width || y >= self.height {
            return false;
        }
        // 2차원 (x, y) 좌표를 1차원 배열 인덱스로 변환
        self.bits[y * self.width + x]
    }
}

// 불러온 TTF/OTF 폰트 데이터를 보관하고 1비트 변환을 수행하는 구조체
pub struct FontConverter {
    font: fontdue::Font, // 파싱 완료된 폰트 객체
}

impl FontConverter {
    // 파일 경로에서 TTF/OTF 폰트 불러오기 (실패 시 패닉 대신 Result 반환)
    pub fn from_file(path: &Path) -> Result<Self, String> {
        // 폰트 파일을 바이트 배열로 읽기
        let bytes = fs::read(path)
            .map_err(|e| format!("폰트 파일을 읽을 수 없습니다 ({}): {e}", path.display()))?;
        // 읽은 바이트를 폰트 객체로 파싱
        Self::from_bytes(&bytes)
    }

    // 바이트 배열에서 TTF/OTF 폰트 파싱
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        // fontdue 기본 설정으로 폰트 데이터 해석
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| format!("폰트 파싱 실패: {e}"))?;
        Ok(Self { font })
    }

    // 단일 문자를 지정된 크기와 임계값으로 1비트 비트맵 변환
    pub fn rasterize_char(&self, ch: char, pixel_size: f32, threshold: u8) -> BitmapGlyph {
        // 글자를 지정 픽셀 크기의 회색조(0~255) 비트맵으로 래스터화
        let (metrics, coverage) = self.font.rasterize(ch, pixel_size);
        // 회색조 배열을 임계값 기준 1비트(bool)로 변환
        let bits = binarize_coverage(&coverage, threshold);

        // 다음 글자 간격 계산 (공백 문자 등도 최소 1px 확보)
        let advance_x = (metrics.advance_width.round() as usize)
            .max(metrics.width)
            .max(1);

        // 완성된 1비트 글리프 반환
        BitmapGlyph {
            ch,
            width: metrics.width,
            height: metrics.height,
            advance_x,
            y_offset: metrics.ymin,
            bits,
        }
    }

    // 문자 집합의 중복을 제거하고 유니코드 순서대로 정렬해 결정적으로 변환
    pub fn rasterize_charset(&self, config: &FontConfig) -> Vec<BitmapGlyph> {
        // 중복·줄바꿈 제거 및 정렬
        let unique_chars = normalize_charset(&config.charset);
        // 정렬된 각 문자를 순서대로 1비트 비트맵으로 변환
        unique_chars
            .into_iter()
            .map(|ch| self.rasterize_char(ch, config.pixel_size, config.threshold))
            .collect()
    }

    // 미리보기 문자열을 엔진 FrameBuffer(32×16)에 기준선 또는 중앙 정렬로 렌더링
    pub fn render_preview(
        &self,
        text: &str,
        pixel_size: f32,
        threshold: u8,
        color: Rgb,
        align_baseline: bool,
    ) -> FrameBuffer {
        // 32×16 엔진 프레임버퍼 생성
        let mut fb = FrameBuffer::new(ScreenLayout::Wide);
        let max_w = usize::from(fb.width()); // 화면 가로(32)
        let max_h = usize::from(fb.height()); // 화면 세로(16)
        let mut cursor_x: usize = 0; // 현재 글자를 찍을 X 시작 위치

        // 16px 높이 화면에서 기준선(baseline)을 y=12(하단 4px 여백)로 배치
        let baseline_y: i32 = (max_h as i32 * 3) / 4;

        // 문자열의 각 글자를 왼쪽부터 순서대로 그리기
        for ch in text.chars() {
            // 32px 화면 오른쪽 끝을 넘어가면 중단
            if cursor_x >= max_w {
                break;
            }

            // 현재 문자를 1비트 비트맵으로 변환
            let glyph = self.rasterize_char(ch, pixel_size, threshold);
            let offset_y = if align_baseline {
                // 기준선 정렬: 글자 상단 Y = baseline_y - (ymin + height)
                baseline_y - (glyph.y_offset + glyph.height as i32)
            } else {
                // 단순 세로 중앙 정렬
                (max_h.saturating_sub(glyph.height) / 2) as i32
            };

            // 글리프의 모든 픽셀을 순회하며 켜진 점만 프레임버퍼에 찍기
            for y in 0..glyph.height {
                for x in 0..glyph.width {
                    if glyph.pixel(x, y) {
                        let px = cursor_x + x; // 실제 화면 X 좌표
                        let py = offset_y + y as i32; // 실제 화면 Y 좌표
                        if let Ok(ix) = i32::try_from(px)
                            && py >= 0
                            && py < max_h as i32
                        {
                            fb.set_pixel(ix, py, color);
                        }
                    }
                }
            }

            // 다음 글자 위치로 커서 이동 (글자 간 1px 여백 포함)
            cursor_x += glyph.advance_x + 1;
        }

        fb // 완성된 미리보기 프레임버퍼 반환
    }
}

// 안티앨리어싱 밝기(0~255) 배열을 임계값 기준 1비트(bool)로 변환하는 순수 함수
pub fn binarize_coverage(coverage: &[u8], threshold: u8) -> Vec<bool> {
    // 각 픽셀 밝기가 threshold 이상이면 true(켜짐), 미만이면 false(꺼짐)
    coverage.iter().map(|&alpha| alpha >= threshold).collect()
}

// 결정적 출력을 위해 문자 집합에서 중복·줄바꿈을 제거하고 정렬하는 순수 함수
pub fn normalize_charset(charset: &str) -> Vec<char> {
    charset
        .chars()
        .filter(|&c| c != '\n' && c != '\r') // 줄바꿈 문자 제외
        .collect::<BTreeSet<char>>() // BTreeSet으로 중복 제거 + 자동 정렬
        .into_iter()
        .collect()
}

// GUI 없이 글꼴 변환 로직만 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;

    // 이진화 임계값 기준으로 픽셀 켜짐/꺼짐이 정확히 나뉘는지 검증
    #[test]
    fn binarize_applies_threshold_correctly() {
        let coverage = [0, 127, 128, 255]; // 경계값 테스트 배열
        // 임계값 128 기준: 127까지는 꺼짐, 128부터 켜짐
        assert_eq!(
            binarize_coverage(&coverage, 128),
            vec![false, false, true, true]
        );
    }

    // 문자 집합 중복 제거 및 정렬이 결정적으로 동작하는지 검증
    #[test]
    fn normalize_charset_deduplicates_and_sorts() {
        let chars = normalize_charset("baba\n가나"); // 중복과 줄바꿈 포함 입력
        // 중복과 줄바꿈이 사라지고 유니코드 순서대로 정렬되어야 함
        assert_eq!(chars, vec!['a', 'b', '가', '나']);
    }

    // 비트맵 글리프의 범위 밖 좌표 조회 시 안전하게 false를 반환하는지 검증
    #[test]
    fn glyph_pixel_out_of_bounds_returns_false() {
        // 2×2 대각선 모양의 테스트용 글리프 생성
        let glyph = BitmapGlyph {
            ch: 'A',
            width: 2,
            height: 2,
            advance_x: 2,
            y_offset: 0,
            bits: vec![true, false, false, true],
        };
        assert!(glyph.pixel(0, 0)); // 켜진 픽셀 확인
        assert!(!glyph.pixel(1, 0)); // 꺼진 픽셀 확인
        assert!(!glyph.pixel(5, 5)); // 범위 밖 좌표는 패닉 없이 false 반환
    }
}
