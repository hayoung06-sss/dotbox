use dotbox_tools::Error as ToolsError;
use dotbox_tools::doctor::{self, DoctorOptions, Report};
use dotbox_tools::local::LocalConfig;
use dotbox_tools::manifest::{MANIFEST_FILE, Manifest, PathsSection, Project, validate_name};
use dotbox_tools::paths::{PROJECT_PATH_MAX_CHARS, path_issues};
use dotbox_tools::project::{self, EngineSource, NewOptions, OFFICIAL_ENGINE_GIT};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

// dotbox-tools 오류를 메시지와 힌트 문자열로 정리하는 헬퍼
pub fn format_tools_error(err: &ToolsError) -> String {
    match err.hint() {
        Some(hint) => format!("{}\n힌트: {hint}", err.message()), // 힌트가 있으면 함께 표시
        None => err.message().to_string(),                        // 힌트가 없으면 메시지만 반환
    }
}

// 새 프로젝트 생성 시 선택할 엔진 소스 방식
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EngineSourceKind {
    #[default]
    Git, // 공식/커스텀 Git 저장소
    LocalPath, // 로컬 엔진 저장소 폴더
}

// 시작 화면의 [새 프로젝트] 입력 폼 상태
#[derive(Debug, Clone)]
pub struct NewProjectDraft {
    pub name: String,                  // 프로젝트 이름 (ASCII 소문자, 숫자, '-')
    pub parent_dir: String,            // 프로젝트를 만들 상위 폴더 경로
    pub engine_kind: EngineSourceKind, // 엔진 소스 종류 (Git / 로컬 경로)
    pub git_url: String,               // Git 저장소 URL
    pub git_rev: String,               // Git 브랜치/태그/커밋 (비우면 기본 브랜치)
    pub local_engine_path: String,     // 로컬 엔진 저장소 루트 경로
    pub firmware: bool,                // firmware/ 포함 여부
    pub git_init: bool,                // git init 실행 여부
}

impl Default for NewProjectDraft {
    // Windows 기준 안전하고 짧은 기본 경로로 초기화
    fn default() -> Self {
        // Windows는 경로 길이 제한 때문에 C:\dev, 그 외 OS는 현재 폴더 사용
        let default_parent = if cfg!(windows) {
            r"C:\dev".to_string()
        } else {
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".to_string())
        };

        Self {
            name: "my-game".to_string(),              // 기본 프로젝트 이름
            parent_dir: default_parent,               // 기본 상위 폴더
            engine_kind: EngineSourceKind::Git,       // 기본 엔진 소스: Git
            git_url: OFFICIAL_ENGINE_GIT.to_string(), // 공식 엔진 저장소 URL
            git_rev: String::new(),                   // 기본 브랜치 사용
            local_engine_path: String::new(),         // 로컬 경로 비워 둠
            firmware: true,                           // 펌웨어 기본 포함
            git_init: true,                           // git init 기본 활성화
        }
    }
}

impl NewProjectDraft {
    // 최종 생성될 프로젝트 루트 경로 계산 (상위 폴더 + 이름)
    pub fn target_path(&self) -> PathBuf {
        let parent = PathBuf::from(self.parent_dir.trim()); // 공백 제거한 상위 폴더
        let name = self.name.trim(); // 공백 제거한 이름
        if name.is_empty() {
            parent // 이름이 비어 있으면 상위 폴더만 반환
        } else {
            parent.join(name) // 상위 폴더 아래에 프로젝트 이름 결합
        }
    }

    // 입력 중인 프로젝트 이름을 dotbox-tools 규칙으로 실시간 검사
    pub fn name_validation_error(&self) -> Option<String> {
        validate_name(self.name.trim()) // dotbox-tools 이름 규칙 검사
            .err()
            .map(|err| format_tools_error(&err)) // 오류 시 문자열로 변환
    }

    // 만들기 전에 경로 문제(한글·공백·OneDrive·길이)를 doctor 기준으로 사전 검사
    pub fn path_warnings(&self) -> Vec<String> {
        // 펌웨어를 포함하지 않으면 ESP-IDF 경로 제약이 없으므로 경고 없음
        if !self.firmware {
            return Vec::new();
        }
        let target = self.target_path(); // 생성 예정 경로 계산
        // 가능하면 절대 경로로 바꿔 정확히 검사
        let resolved = std::path::absolute(&target).unwrap_or(target);
        // 발견된 모든 경로 문제를 메시지 문자열 목록으로 변환
        path_issues(&resolved, PROJECT_PATH_MAX_CHARS)
            .into_iter()
            .map(|issue| issue.message())
            .collect()
    }

    // 현재 입력값으로 dotbox-tools::project::create 실행
    pub fn create_project(&self) -> Result<(PathBuf, Vec<String>), String> {
        let name = self.name.trim();
        // 생성 전 이름 규칙 먼저 검증
        validate_name(name).map_err(|err| format_tools_error(&err))?;

        // 선택한 엔진 소스 방식에 따라 EngineSource 구성
        let engine = match self.engine_kind {
            EngineSourceKind::Git => {
                let url = self.git_url.trim();
                if url.is_empty() {
                    return Err("Git 저장소 URL을 입력하세요.".to_string());
                }
                let rev = self.git_rev.trim();
                EngineSource::Git {
                    url: url.to_string(),
                    rev: (!rev.is_empty()).then(|| rev.to_string()), // 비어 있으면 None
                }
            }
            EngineSourceKind::LocalPath => {
                let path = self.local_engine_path.trim();
                if path.is_empty() {
                    return Err("로컬 엔진 저장소 경로를 입력하세요.".to_string());
                }
                EngineSource::Path(PathBuf::from(path))
            }
        };

        // dotbox new와 동일한 옵션 구조체 생성
        let options = NewOptions {
            path: self.target_path(),
            name: Some(name.to_string()),
            engine,
            firmware: self.firmware,
            git_init: self.git_init,
        };
        // 실제 프로젝트 파일 생성 실행
        let created = project::create(&options).map_err(|err| format_tools_error(&err))?;
        Ok((created.root, created.warnings)) // 생성된 루트 경로와 경고 목록 반환
    }
}

// Studio에서 현재 열려 있는 게임 프로젝트 상태
#[derive(Debug, Clone)]
pub struct OpenedProject {
    pub project: Project,          // dotbox.toml에서 읽은 프로젝트 정보
    pub local_config: LocalConfig, // .dotbox/local.toml 개인 설정 (포트, 플래시 기록)
}

impl OpenedProject {
    // 지정한 경로(또는 하위 폴더)에서 프로젝트를 찾아 열기
    pub fn open(dir: &Path) -> Result<Self, String> {
        // 상위 폴더로 올라가며 dotbox.toml 탐색 및 로드
        let project = Project::discover(dir).map_err(|err| format_tools_error(&err))?;
        // .dotbox/local.toml 개인 설정 로드 (없으면 기본값)
        let local_config = LocalConfig::load(&project).map_err(|err| format_tools_error(&err))?;
        Ok(Self {
            project,
            local_config,
        })
    }

    // dotbox.toml의 [paths] 수정본을 Manifest::parse로 검증한 뒤 저장
    pub fn save_paths(
        &mut self,
        game: &str,
        sim: &str,
        firmware: &str,
        assets: &str,
    ) -> Result<(), String> {
        // 모든 경로 입력값의 앞뒤 공백 제거
        let game = game.trim();
        let sim = sim.trim();
        let firmware = firmware.trim();
        let assets = assets.trim();

        // 비어 있는 경로가 있으면 저장 차단
        if game.is_empty() || sim.is_empty() || firmware.is_empty() || assets.is_empty() {
            return Err("모든 [paths] 항목은 비워 둘 수 없습니다.".to_string());
        }

        // 기존 프로젝트 이름은 유지하고 [paths]만 새 값으로 구성
        let updated = Manifest {
            project: self.project.manifest().project.clone(),
            paths: PathsSection {
                game: PathBuf::from(game),
                sim: PathBuf::from(sim),
                firmware: PathBuf::from(firmware),
                assets: PathBuf::from(assets),
            },
        };

        // TOML 문자열로 직렬화한 뒤 저장 전 Manifest::parse로 사전 검사
        let toml_text = updated.to_toml();
        Manifest::parse(&toml_text).map_err(|err| format_tools_error(&err))?;

        // 검증 통과 시 dotbox.toml 파일에 쓰기
        let manifest_path = self.project.root().join(MANIFEST_FILE);
        fs::write(&manifest_path, toml_text)
            .map_err(|err| format!("dotbox.toml 저장 실패 ({}): {err}", manifest_path.display()))?;

        // 디스크에서 프로젝트를 다시 읽어 메모리 상태 동기화
        self.project =
            Project::load(self.project.root()).map_err(|err| format_tools_error(&err))?;
        Ok(())
    }

    // .dotbox/local.toml의 왼쪽/오른쪽 포트 설정 갱신 후 저장
    pub fn save_ports(&mut self, left: &str, right: &str) -> Result<(), String> {
        let left = left.trim(); // 왼쪽 포트 문자열 정리
        let right = right.trim(); // 오른쪽 포트 문자열 정리

        // 빈 문자열이면 None(미지정), 값이 있으면 Some으로 저장
        self.local_config.ports.left = (!left.is_empty()).then(|| left.to_string());
        self.local_config.ports.right = (!right.is_empty()).then(|| right.to_string());

        // .dotbox/local.toml 파일에 저장
        self.local_config
            .save(&self.project)
            .map_err(|err| format_tools_error(&err))
    }
}

// dotbox_tools::doctor::run을 백그라운드 스레드에서 실행하고 결과 수신 채널 반환
pub fn run_doctor_async(project: Project, host_only: bool) -> Receiver<Report> {
    let (tx, rx) = mpsc::channel(); // 점검 결과를 돌려받을 채널 생성

    // 외부 도구(rustc, rustup 등) 조사 중 GUI가 멈추지 않도록 별도 스레드 실행
    std::thread::spawn(move || {
        let report = doctor::run(DoctorOptions {
            project: Some(&project), // 열린 프로젝트 기준으로 점검
            host_only,               // 호스트(시뮬레이터) 전용 점검 여부
        });
        let _ = tx.send(report); // 완료된 Report를 메인 GUI 스레드로 전송
    });

    rx
}

// GUI 없이 프로젝트 이름 검사·경로 경고·설정 저장 흐름을 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;

    // 실시간 이름 검사가 유효/무효 이름을 정확히 판별하는지 검증
    #[test]
    fn draft_validates_project_name() {
        let mut draft = NewProjectDraft {
            name: "snake-2p".to_string(), // 소문자·숫자·하이픈 조합
            ..Default::default()
        };
        assert!(draft.name_validation_error().is_none()); // 오류 없어야 통과

        draft.name = "SnakeGame".to_string(); // 대문자 포함 (규칙 위반)
        assert!(draft.name_validation_error().is_some()); // 오류 발생해야 통과
    }

    // 펌웨어 포함 시 공백·한글·OneDrive 경로 경고가 만들기 전에 나오는지 검증
    #[test]
    fn draft_warns_bad_path_only_with_firmware() {
        let mut draft = NewProjectDraft {
            parent_dir: "C:/Users/홍길동/OneDrive/my games".to_string(),
            name: "snake".to_string(),
            firmware: true, // 펌웨어 포함 (경로 검사 활성화)
            ..Default::default()
        };

        let warnings = draft.path_warnings(); // 사전 경로 경고 목록 수집
        assert!(warnings.iter().any(|w| w.contains("ASCII"))); // 한글 경고 확인
        assert!(warnings.iter().any(|w| w.contains("공백"))); // 공백 경고 확인
        assert!(warnings.iter().any(|w| w.contains("OneDrive"))); // OneDrive 경고 확인

        // 펌웨어 미포함으로 바꾸면 경로 경고가 비어 있어야 함
        draft.firmware = false;
        assert!(draft.path_warnings().is_empty());
    }

    // 새 프로젝트 생성 후 열기·경로 수정·포트 저장이 왕복 동작하는지 검증
    #[test]
    fn create_open_and_save_settings_round_trip() {
        let dir = tempfile::tempdir().unwrap(); // 임시 폴더 생성
        let draft = NewProjectDraft {
            name: "pong".to_string(),
            parent_dir: dir.path().display().to_string(),
            engine_kind: EngineSourceKind::Git,
            git_url: OFFICIAL_ENGINE_GIT.to_string(),
            git_rev: String::new(),
            local_engine_path: String::new(),
            firmware: false, // 테스트 속도 위해 펌웨어 제외
            git_init: false, // 테스트 속도 위해 git init 제외
        };

        let (root, _warnings) = draft.create_project().unwrap(); // 프로젝트 생성
        let mut opened = OpenedProject::open(&root).unwrap(); // 생성된 폴더 열기
        assert_eq!(opened.project.name(), "pong");

        // [paths] 수정 후 저장 및 재로딩 확인
        opened
            .save_paths("game", "desktop", "firmware", "assets")
            .unwrap();
        assert_eq!(
            opened.project.manifest().paths.sim,
            PathBuf::from("desktop")
        );

        // 빈 경로는 저장이 거부되어야 함
        assert!(opened.save_paths("", "sim", "firmware", "assets").is_err());

        // 좌/우 포트 저장 후 다시 열었을 때 유지되는지 확인
        opened.save_ports("COM5", "COM6").unwrap();
        let reopened = OpenedProject::open(&root).unwrap();
        assert_eq!(reopened.local_config.ports.left.as_deref(), Some("COM5"));
        assert_eq!(reopened.local_config.ports.right.as_deref(), Some("COM6"));
    }
}
