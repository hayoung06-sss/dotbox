use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

// 최근 프로젝트 목록 최대 보관 개수
const MAX_RECENT_PROJECTS: usize = 10;

// 사용자 전역 설정 (OS 설정 디렉토리의 dotbox/studio.toml에 저장)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSettings {
    pub recent_projects: Vec<PathBuf>, // 최근 연 프로젝트 경로 목록
    pub window_width: f32,             // 마지막 창 가로 크기
    pub window_height: f32,            // 마지막 창 세로 크기
    pub led_preview: bool,             // 실기기 밝기 미리보기 켜짐 여부
    pub brightness_percent: u8,        // 실기기 밝기 비율 (15~25%)
}

impl Default for UserSettings {
    // 설정 파일이 없을 때 쓸 기본값
    fn default() -> Self {
        Self {
            recent_projects: Vec::new(), // 최근 목록 비어 있음
            window_width: 960.0,         // 기본 창 너비
            window_height: 640.0,        // 기본 창 높이
            led_preview: false,          // 밝기 미리보기 기본 꺼짐
            brightness_percent: 20,      // 기본 밝기 20%
        }
    }
}

impl UserSettings {
    // OS별 사용자 설정 파일 경로 반환 (Windows: %APPDATA%\dotbox\studio.toml)
    pub fn config_path() -> Option<PathBuf> {
        #[cfg(windows)]
        {
            // Windows 환경 변수 APPDATA 아래 dotbox/studio.toml 경로 생성
            std::env::var_os("APPDATA")
                .map(|dir| PathBuf::from(dir).join("dotbox").join("studio.toml"))
        }
        #[cfg(target_os = "macos")]
        {
            // macOS 표준 Application Support 아래 dotbox/studio.toml 경로 생성
            std::env::var_os("HOME").map(|home| {
                PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
                    .join("dotbox")
                    .join("studio.toml")
            })
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            // Linux: XDG_CONFIG_HOME 우선, 없으면 ~/.config 사용
            let base = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
            Some(base.join("dotbox").join("studio.toml"))
        }
    }

    // 지정한 경로에서 설정 파일 읽기 (없거나 손상되면 기본값 반환, 패닉 금지)
    pub fn load_from(path: &Path) -> Self {
        // 파일 읽기 실패 시 기본값 반환
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        // TOML 파싱 실패 시 기본값 반환
        let mut settings: Self = toml::from_str(&text).unwrap_or_default();
        // 밝기 값은 항상 15~25% 범위 안으로 안전하게 보정
        settings.brightness_percent = settings.brightness_percent.clamp(15, 25);
        settings
    }

    // 지정한 경로에 설정 파일 저장 (상위 폴더가 없으면 자동 생성)
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        // 상위 디렉터리가 없으면 먼저 생성
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("설정 폴더 생성 실패 ({}): {e}", parent.display()))?;
        }
        // 구조체를 읽기 좋은 TOML 문자열로 직렬화
        let text = toml::to_string_pretty(self).map_err(|e| format!("설정 직렬화 실패: {e}"))?;
        // 파일에 쓰기
        fs::write(path, text).map_err(|e| format!("설정 파일 쓰기 실패 ({}): {e}", path.display()))
    }

    // 기본 OS 설정 경로에서 불러오기
    pub fn load() -> Self {
        // 경로를 찾으면 load_from 호출, 못 찾으면 기본값 반환
        Self::config_path()
            .map(|path| Self::load_from(&path))
            .unwrap_or_default()
    }

    // 기본 OS 설정 경로에 저장하기
    pub fn save(&self) -> Result<(), String> {
        // OS 설정 경로가 없으면 에러 반환
        let Some(path) = Self::config_path() else {
            return Err("OS 설정 경로를 찾을 수 없습니다.".to_string());
        };
        self.save_to(&path) // 해당 경로에 저장
    }

    // 최근 프로젝트 목록 맨 앞에 경로 추가 (중복 제거 및 최대 개수 유지)
    pub fn push_recent(&mut self, path: PathBuf) {
        self.recent_projects.retain(|existing| existing != &path); // 기존 중복 경로 제거
        self.recent_projects.insert(0, path); // 맨 앞(최신)에 삽입
        self.recent_projects.truncate(MAX_RECENT_PROJECTS); // 최대 10개까지만 유지
    }

    // 최근 프로젝트 목록에서 특정 경로 삭제
    pub fn remove_recent(&mut self, path: &Path) {
        self.recent_projects.retain(|existing| existing != path); // 일치하는 항목만 제외
    }

    // 마지막 창 크기를 최소 크기 이상으로 보정해 설정에 반영 (최소화·비정상 크기 방지)
    pub fn update_window_size(&mut self, width: f32, height: f32) {
        // NaN·무한대 같은 비정상 실수 값은 무시
        if width.is_finite() && height.is_finite() {
            self.window_width = width.max(640.0); // 최소 너비 640px 보장
            self.window_height = height.max(480.0); // 최소 높이 480px 보장
        }
    }
}

// GUI 없이 사용자 설정 저장·불러오기 및 최근 목록 로직을 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;

    // 설정 파일이 없을 때 기본값이 안전하게 반환되는지 검증
    #[test]
    fn missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap(); // 임시 폴더 생성
        let path = dir.path().join("missing.toml"); // 존재하지 않는 파일 경로
        assert_eq!(UserSettings::load_from(&path), UserSettings::default()); // 기본값 반환 확인
    }

    // 최근 프로젝트 추가 시 중복 제거·최신순 정렬·삭제가 동작하는지 검증
    #[test]
    fn recent_projects_deduplicate_and_remove() {
        let mut settings = UserSettings::default();
        let p1 = PathBuf::from("C:/dev/snake");
        let p2 = PathBuf::from("C:/dev/pong");

        settings.push_recent(p1.clone()); // [p1]
        settings.push_recent(p2.clone()); // [p2, p1]
        settings.push_recent(p1.clone()); // p1을 다시 열면 [p1, p2]로 맨 앞 이동
        assert_eq!(settings.recent_projects, vec![p1.clone(), p2.clone()]);

        settings.remove_recent(&p1); // p1 제거 -> [p2]
        assert_eq!(settings.recent_projects, vec![p2]);
    }

    // 설정 파일 저장 후 다시 읽었을 때 값이 동일한지 왕복 검증
    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap(); // 임시 폴더 생성
        let path = dir.path().join("dotbox").join("studio.toml"); // 테스트용 설정 파일 경로

        let mut settings = UserSettings::default();
        settings.push_recent(PathBuf::from("C:/dev/snake")); // 최근 경로 추가
        settings.led_preview = true; // 밝기 미리보기 켜기
        settings.brightness_percent = 22; // 밝기 22% 설정

        settings.save_to(&path).unwrap(); // 파일로 저장
        let loaded = UserSettings::load_from(&path); // 다시 읽기
        assert_eq!(loaded, settings); // 저장 전후 값 일치 확인
    }

    // 창 크기 갱신 시 최소 크기 보정 및 비정상 값 무시가 동작하는지 검증
    #[test]
    fn update_window_size_clamps_minimum_and_ignores_nan() {
        let mut settings = UserSettings::default();

        // 너무 작은 창 크기는 최소 640×480으로 보정되어야 함
        settings.update_window_size(200.0, 100.0);
        assert_eq!(settings.window_width, 640.0);
        assert_eq!(settings.window_height, 480.0);

        // 정상 크기는 그대로 반영되어야 함
        settings.update_window_size(1280.0, 800.0);
        assert_eq!(settings.window_width, 1280.0);
        assert_eq!(settings.window_height, 800.0);

        // NaN 값은 기존 값을 덮어쓰지 않고 무시해야 함
        settings.update_window_size(f32::NAN, 900.0);
        assert_eq!(settings.window_width, 1280.0);
        assert_eq!(settings.window_height, 800.0);
    }
}
