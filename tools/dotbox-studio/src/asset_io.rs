use dotbox_core::Note;
use dotbox_core::hal::Rgb;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::Path;

// 4.4 규칙: 에셋 이름은 ASCII 소문자로 시작하고 소문자·숫자·'_'만 허용
pub fn validate_asset_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("에셋 이름을 입력하세요.".to_string());
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("에셋 이름을 입력하세요.".to_string());
    };
    if !first.is_ascii_lowercase() {
        return Err("에셋 이름은 영문 소문자로 시작해야 합니다. (예: player_run)".to_string());
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err("에셋 이름은 영문 소문자, 숫자, '_'만 사용할 수 있습니다.".to_string());
    }
    Ok(())
}

// 스프라이트 픽셀 배열을 RGB8 PNG 파일로 저장 (상위 폴더 자동 생성)
pub fn save_sprite_png(
    path: &Path,
    width: usize,
    height: usize,
    pixels: &[Rgb],
) -> Result<(), String> {
    if pixels.len() != width * height {
        return Err("픽셀 데이터 크기가 캔버스 크기와 일치하지 않습니다.".to_string());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("스프라이트 폴더 생성 실패 ({}): {e}", parent.display()))?;
    }

    let file =
        File::create(path).map_err(|e| format!("PNG 파일 생성 실패 ({}): {e}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgb); // 24비트 RGB
    encoder.set_depth(png::BitDepth::Eight); // 채널당 8비트

    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("PNG 헤더 쓰기 실패: {e}"))?;

    // Rgb 배열을 [r, g, b, ...] 바이트 버퍼로 직렬화
    let mut raw = Vec::with_capacity(pixels.len() * 3);
    for px in pixels {
        raw.push(px.r);
        raw.push(px.g);
        raw.push(px.b);
    }
    writer
        .write_image_data(&raw)
        .map_err(|e| format!("PNG 데이터 쓰기 실패: {e}"))
}

// PNG 파일을 읽어 (가로, 세로, RGB 픽셀 배열) 반환 (RGB8·RGBA8 지원)
pub fn load_sprite_png(path: &Path) -> Result<(usize, usize, Vec<Rgb>), String> {
    let file =
        File::open(path).map_err(|e| format!("PNG 파일 열기 실패 ({}): {e}", path.display()))?;
    let decoder = png::Decoder::new(BufReader::new(file));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("PNG 헤더 읽기 실패: {e}"))?;

    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG 프레임 디코딩 실패: {e}"))?;

    if info.bit_depth != png::BitDepth::Eight {
        return Err("8비트 PNG 파일만 불러올 수 있습니다.".to_string());
    }

    let bytes = &buf[..info.buffer_size()];
    let pixels = match info.color_type {
        png::ColorType::Rgb => bytes
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| Rgb::new(c[0], c[1], c[2]))
            .collect(),
        png::ColorType::Rgba => bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| {
                // 완전 투명 픽셀은 LED 꺼짐(검은색)으로 처리
                if c[3] == 0 {
                    Rgb::BLACK
                } else {
                    Rgb::new(c[0], c[1], c[2])
                }
            })
            .collect(),
        other => {
            return Err(format!("지원하지 않는 PNG 색상 형식입니다: {other:?}"));
        }
    };

    Ok((info.width as usize, info.height as usize, pixels))
}

// .sound.toml 직렬화용 음표 레코드 (5장 합의 시 이 구조체만 맞추면 됨)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NoteRecord {
    freq_hz: u16,     // 주파수 (Hz, 0이면 쉼표)
    duration_ms: u16, // 재생 길이 (ms)
}

// .sound.toml 루트 구조
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SoundFile {
    notes: Vec<NoteRecord>, // 음표 시퀀스
}

// 멜로디 음표 목록을 .sound.toml 파일로 저장
pub fn save_sound_toml(path: &Path, notes: &[Note]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("효과음 폴더 생성 실패 ({}): {e}", parent.display()))?;
    }

    let data = SoundFile {
        notes: notes
            .iter()
            .map(|n| NoteRecord {
                freq_hz: n.freq_hz,
                duration_ms: n.duration_ms,
            })
            .collect(),
    };
    let text =
        toml::to_string_pretty(&data).map_err(|e| format!("효과음 TOML 직렬화 실패: {e}"))?;
    fs::write(path, text).map_err(|e| format!("효과음 파일 저장 실패 ({}): {e}", path.display()))
}

// .sound.toml 파일을 읽어 음표 목록 반환
pub fn load_sound_toml(path: &Path) -> Result<Vec<Note>, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("효과음 파일 읽기 실패 ({}): {e}", path.display()))?;
    let data: SoundFile =
        toml::from_str(&text).map_err(|e| format!("효과음 TOML 파싱 실패: {e}"))?;
    Ok(data
        .notes
        .into_iter()
        .map(|r| Note::new(r.freq_hz, r.duration_ms))
        .collect())
}

// assets/sprites/ 폴더 안의 *.png 파일 이름(확장자 제외) 목록을 정렬해 반환
pub fn list_sprite_names(assets_dir: &Path) -> Vec<String> {
    let dir = assets_dir.join("sprites");
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new(); // 폴더가 아직 없으면 빈 목록
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("png") {
                path.file_stem()?.to_str().map(ToString::to_string)
            } else {
                None
            }
        })
        .collect();
    names.sort(); // 가나다/알파벳 순 정렬
    names
}

// assets/sounds/ 폴더 안의 *.sound.toml 파일 이름(확장자 제외) 목록을 정렬해 반환
pub fn list_sound_names(assets_dir: &Path) -> Vec<String> {
    let dir = assets_dir.join("sounds");
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new(); // 폴더가 아직 없으면 빈 목록
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let s = name.to_str()?;
            s.strip_suffix(".sound.toml").map(ToString::to_string)
        })
        .collect();
    names.sort(); // 가나다/알파벳 순 정렬
    names
}

// .font.toml 직렬화용 글꼴 설정 구조체 (5장 합의 시 이 구조체만 맞추면 됨)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontAssetFile {
    pub source: String,  // 원본 TTF/OTF 파일 경로
    pub pixel_size: f32, // 픽셀 크기 (px)
    pub threshold: u8,   // 이진화 임계값 (1~255)
    pub charset: String, // 포함할 문자 집합
    #[serde(default)]
    pub license: String, // 글꼴 라이선스 메모
}

// 글꼴 설정을 assets/fonts/<이름>.font.toml 파일로 저장
pub fn save_font_toml(path: &Path, data: &FontAssetFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("글꼴 폴더 생성 실패 ({}): {e}", parent.display()))?;
    }
    let text = toml::to_string_pretty(data).map_err(|e| format!("글꼴 TOML 직렬화 실패: {e}"))?;
    fs::write(path, text).map_err(|e| format!("글꼴 파일 저장 실패 ({}): {e}", path.display()))
}

// .font.toml 파일을 읽어 글꼴 설정 반환
pub fn load_font_toml(path: &Path) -> Result<FontAssetFile, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("글꼴 파일 읽기 실패 ({}): {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("글꼴 TOML 파싱 실패: {e}"))
}

// assets/fonts/ 폴더 안의 *.font.toml 파일 이름(확장자 제외) 목록을 정렬해 반환
pub fn list_font_names(assets_dir: &Path) -> Vec<String> {
    let dir = assets_dir.join("fonts");
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let s = name.to_str()?;
            s.strip_suffix(".font.toml").map(ToString::to_string)
        })
        .collect();
    names.sort();
    names
}

// assets/icon.png 파일 존재 여부 확인
pub fn has_icon_png(assets_dir: &Path) -> bool {
    assets_dir.join("icon.png").is_file()
}

// GUI 없이 이름 검사·PNG 왕복·TOML 왕복을 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;

    // 에셋 이름 규칙(소문자 시작, 소문자·숫자·'_') 검증
    #[test]
    fn validate_asset_name_checks_rules() {
        assert!(validate_asset_name("player_run_1").is_ok());
        assert!(validate_asset_name("").is_err());
        assert!(validate_asset_name("1player").is_err());
        assert!(validate_asset_name("Player").is_err());
        assert!(validate_asset_name("my-sprite").is_err());
    }

    // PNG 저장 후 다시 읽었을 때 크기와 픽셀이 동일한지 검증
    #[test]
    fn sprite_png_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sprites").join("hero.png");
        let pixels = vec![
            Rgb::new(255, 0, 0),
            Rgb::new(0, 255, 0),
            Rgb::new(0, 0, 255),
            Rgb::BLACK,
        ];

        save_sprite_png(&path, 2, 2, &pixels).unwrap();
        let (w, h, loaded) = load_sprite_png(&path).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!(loaded, pixels);
    }

    // .sound.toml 저장 후 다시 읽었을 때 음표 목록이 동일한지 검증
    #[test]
    fn sound_toml_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sounds").join("jump.sound.toml");
        let notes = vec![Note::new(523, 100), Note::new(0, 50), Note::new(784, 150)];

        save_sound_toml(&path, &notes).unwrap();
        let loaded = load_sound_toml(&path).unwrap();
        assert_eq!(loaded, notes);
    }

    // .font.toml 저장 후 다시 읽었을 때 설정값이 동일한지 검증
    #[test]
    fn font_toml_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fonts").join("main.font.toml");
        let font = FontAssetFile {
            source: r"C:\Windows\Fonts\malgun.ttf".to_string(),
            pixel_size: 12.0,
            threshold: 128,
            charset: "DotBox 0123456789".to_string(),
            license: "MIT".to_string(),
        };

        save_font_toml(&path, &font).unwrap();
        let loaded = load_font_toml(&path).unwrap();
        assert_eq!(loaded, font);
    }
}
