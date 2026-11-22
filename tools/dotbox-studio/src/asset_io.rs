use dotbox_core::Note;
use dotbox_core::hal::Rgb;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::Command;

// 4.4 규칙: 에셋 탐색기에서 관리하는 에셋 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Sprite, // assets/sprites/<name>.png
    Sound,  // assets/sounds/<name>.sound.toml
    Font,   // assets/fonts/<name>.font.toml
}

impl AssetKind {
    // UI에 표시할 에셋 종류 이름
    pub fn label(self) -> &'static str {
        match self {
            Self::Sprite => "스프라이트 (.png)",
            Self::Sound => "효과음 (.sound.toml)",
            Self::Font => "글꼴 (.font.toml)",
        }
    }

    // assets_dir 기준 해당 에셋의 실제 파일 경로 계산
    pub fn asset_path(self, assets_dir: &Path, name: &str) -> PathBuf {
        match self {
            Self::Sprite => assets_dir.join("sprites").join(format!("{name}.png")),
            Self::Sound => assets_dir.join("sounds").join(format!("{name}.sound.toml")),
            Self::Font => assets_dir.join("fonts").join(format!("{name}.font.toml")),
        }
    }
}

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

// 새 에셋을 기본 템플릿으로 생성 (이미 같은 이름이 있으면 오류 반환)
pub fn create_default_asset(
    assets_dir: &Path,
    kind: AssetKind,
    name: &str,
) -> Result<PathBuf, String> {
    validate_asset_name(name)?;
    let path = kind.asset_path(assets_dir, name);
    if path.exists() {
        return Err(format!("이미 존재하는 에셋입니다: {name}"));
    }

    match kind {
        AssetKind::Sprite => {
            // 기본 16×16 빈(검은색) 스프라이트 PNG 생성
            let pixels = vec![Rgb::BLACK; 16 * 16];
            save_sprite_png(&path, 16, 16, &pixels)?;
        }
        AssetKind::Sound => {
            // 기본 2음표(도-솔) 효과음 TOML 생성
            let notes = vec![Note::new(523, 120), Note::new(784, 120)];
            save_sound_toml(&path, &notes)?;
        }
        AssetKind::Font => {
            // 기본 글꼴 변환 설정 TOML 생성
            let data = FontAssetFile {
                source: r"C:\Windows\Fonts\malgun.ttf".to_string(),
                pixel_size: 12.0,
                threshold: 128,
                charset: "DotBox 0123456789".to_string(),
                license: "OFL-1.1".to_string(),
            };
            save_font_toml(&path, &data)?;
        }
    }
    Ok(path)
}

// 기존 에셋의 이름 변경 (식별자 규칙 검사 및 중복 방지 포함)
pub fn rename_asset(
    assets_dir: &Path,
    kind: AssetKind,
    old_name: &str,
    new_name: &str,
) -> Result<PathBuf, String> {
    validate_asset_name(new_name)?;
    if old_name == new_name {
        return Err("새 이름이 기존 이름과 같습니다.".to_string());
    }
    let old_path = kind.asset_path(assets_dir, old_name);
    if !old_path.is_file() {
        return Err(format!(
            "원본 에셋 파일을 찾을 수 없습니다: {}",
            old_path.display()
        ));
    }
    let new_path = kind.asset_path(assets_dir, new_name);
    if new_path.exists() {
        return Err(format!("이미 존재하는 에셋 이름입니다: {new_name}"));
    }
    fs::rename(&old_path, &new_path).map_err(|e| {
        format!(
            "에셋 이름 변경 실패 ({} -> {}): {e}",
            old_path.display(),
            new_path.display()
        )
    })?;
    Ok(new_path)
}

// 기존 에셋을 새 이름으로 복제 (식별자 규칙 검사 및 중복 방지 포함)
pub fn duplicate_asset(
    assets_dir: &Path,
    kind: AssetKind,
    src_name: &str,
    dst_name: &str,
) -> Result<PathBuf, String> {
    validate_asset_name(dst_name)?;
    let src_path = kind.asset_path(assets_dir, src_name);
    if !src_path.is_file() {
        return Err(format!(
            "원본 에셋 파일을 찾을 수 없습니다: {}",
            src_path.display()
        ));
    }
    let dst_path = kind.asset_path(assets_dir, dst_name);
    if dst_path.exists() {
        return Err(format!("이미 존재하는 에셋 이름입니다: {dst_name}"));
    }
    fs::copy(&src_path, &dst_path).map_err(|e| {
        format!(
            "에셋 복제 실패 ({} -> {}): {e}",
            src_path.display(),
            dst_path.display()
        )
    })?;
    Ok(dst_path)
}

// 지정한 에셋 파일 삭제
pub fn delete_asset(assets_dir: &Path, kind: AssetKind, name: &str) -> Result<(), String> {
    let path = kind.asset_path(assets_dir, name);
    if !path.is_file() {
        return Err(format!(
            "삭제할 에셋 파일을 찾을 수 없습니다: {}",
            path.display()
        ));
    }
    fs::remove_file(&path).map_err(|e| format!("에셋 삭제 실패 ({}): {e}", path.display()))
}

// assets/ 내에서 알 수 없는 종류의 파일 목록을 수집 ("기타" 표시용)
pub fn list_other_files(assets_dir: &Path) -> Vec<String> {
    let mut others = Vec::new();

    // 1) assets/ 루트의 알 수 없는 파일 (icon.png 제외)
    if let Ok(entries) = fs::read_dir(assets_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
                && name != "icon.png"
            {
                others.push(name.to_string());
            }
        }
    }

    // 2) 하위 폴더별 예상 외 확장자 파일 수집
    type KnownAssetCheck = fn(&str) -> bool;
    let subdirs: [(&str, KnownAssetCheck); 3] = [
        ("sprites", |n| {
            n.ends_with(".png") || n.ends_with(".sprite.toml")
        }),
        ("sounds", |n| n.ends_with(".sound.toml")),
        ("fonts", |n| {
            n.ends_with(".font.toml") || n.ends_with(".ttf") || n.ends_with(".otf")
        }),
    ];

    for (sub, is_known) in subdirs {
        let dir = assets_dir.join(sub);
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && !is_known(name)
                {
                    others.push(format!("{sub}/{name}"));
                }
            }
        }
    }

    others.sort();
    others
}

// OS 기본 파일 탐색기로 지정한 폴더 열기 (폴더가 없으면 자동 생성 후 열기)
pub fn open_in_file_explorer(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        fs::create_dir_all(dir).map_err(|e| format!("폴더 생성 실패 ({}): {e}", dir.display()))?;
    }

    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("explorer");
        c.arg(dir);
        c
    };

    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(dir);
        c
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(dir);
        c
    };

    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("탐색기 실행 실패 ({}): {e}", dir.display()))
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

// GUI 없이 이름 검사·PNG 왕복·TOML 왕복·탐색기 CRUD를 검증하는 단위 테스트
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

    // 에셋 생성·중복 방지·이름 변경·복제·삭제 전체 흐름 검증
    #[test]
    fn create_rename_duplicate_delete_asset_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let assets = dir.path();

        // 1) 기본 스프라이트 생성 및 중복 생성 거부 확인
        create_default_asset(assets, AssetKind::Sprite, "hero").unwrap();
        assert!(create_default_asset(assets, AssetKind::Sprite, "hero").is_err());
        assert_eq!(list_sprite_names(assets), vec!["hero"]);

        // 2) 복제 확인
        duplicate_asset(assets, AssetKind::Sprite, "hero", "hero_copy").unwrap();
        assert_eq!(list_sprite_names(assets), vec!["hero", "hero_copy"]);

        // 3) 이름 변경 확인
        rename_asset(assets, AssetKind::Sprite, "hero_copy", "enemy").unwrap();
        assert_eq!(list_sprite_names(assets), vec!["enemy", "hero"]);

        // 4) 삭제 확인
        delete_asset(assets, AssetKind::Sprite, "enemy").unwrap();
        assert_eq!(list_sprite_names(assets), vec!["hero"]);

        // 5) 효과음·글꼴 기본 생성 검증
        create_default_asset(assets, AssetKind::Sound, "beep").unwrap();
        create_default_asset(assets, AssetKind::Font, "digits").unwrap();
        assert_eq!(list_sound_names(assets), vec!["beep"]);
        assert_eq!(list_font_names(assets), vec!["digits"]);
    }

    // 알 수 없는 파일만 "기타" 목록에 수집되는지 검증
    #[test]
    fn list_other_files_finds_unknown_assets() {
        let dir = tempfile::tempdir().unwrap();
        let assets = dir.path();
        create_default_asset(assets, AssetKind::Sprite, "hero").unwrap();
        fs::write(assets.join("icon.png"), b"").unwrap();
        fs::write(assets.join("notes.txt"), b"memo").unwrap();
        fs::write(assets.join("sprites").join("readme.md"), b"doc").unwrap();

        let others = list_other_files(assets);
        assert_eq!(others, vec!["notes.txt", "sprites/readme.md"]);
    }
}
