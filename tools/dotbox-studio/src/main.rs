mod asset_io; // 스프라이트(.png) · 효과음(.sound.toml) · 글꼴(.font.toml) · 아이콘 파일 입출력 모듈
mod command; // GUI 독립 편집 명령 객체 및 실행 취소/다시 실행 모듈
mod device;
mod diagnostics; // cargo JSON 컴파일 진단 파싱 모듈 // 4순위: 환경 점검·포트 감지·플래시 로직 모듈
mod editor; // 1순위: 스프라이트·화면 편집 로직 모듈
mod font; // 2순위: 글꼴 변환·미리보기 로직 모듈
mod scene; // 32×16 장면(화면) 임시 미리보기 모듈
mod serial_log; // 왼쪽/오른쪽 모듈 시리얼 로그 수신·검색·저장 모듈
mod settings; // 사용자 전역 설정 (최근 프로젝트, 창 크기, 밝기)
mod sound; // 3순위: 효과음·멜로디 편집 및 재생 로직 모듈
mod tabs; // 파일 단위 탭 및 더티(●) 상태 관리 모듈
mod workspace; // 프로젝트 열기·생성·설정 저장·doctor 실행 모듈

// 명령 객체, 이력 관리자, 캔버스 스냅샷 헬퍼 불러오기
use asset_io::{
    AssetKind, FontAssetFile, create_default_asset, delete_asset, duplicate_asset, has_icon_png,
    list_font_names, list_other_files, list_sound_names, list_sprite_names, load_font_toml,
    load_sound_toml, load_sprite_png, open_in_file_explorer, rename_asset, save_font_toml,
    save_sound_toml, save_sprite_png, validate_asset_name,
};
use command::{CommandHistory, ReplaceCanvasCommand, ReplaceMelodyCommand, snapshot_canvas};
use device::{
    DetectedPort, FlashTarget, RunningCommand, build_flash_args, build_run_args, format_command,
    scan_serial_ports, spawn_command_streaming,
};
use diagnostics::{CargoDiagnostic, DiagnosticLevel, parse_cargo_json_line};
use dotbox_core::display::{FrameBuffer, PANEL_WIDTH, ScreenLayout};
use dotbox_core::hal::Rgb;
use dotbox_tools::doctor::{Level, Report};
use editor::{CanvasEditor, DrawTool, PALETTE, SpriteSnapshot, scale_brightness};
use eframe::egui;
use font::{FontConfig, FontConverter};
use scene::{SceneItemKind, ScenePreview};
use serial_log::SerialMonitor;
use settings::UserSettings;
use sound::{
    BUZZER_MAX_FREQ_HZ, BUZZER_MIN_FREQ_HZ, MelodyEditor, NOTE_PRESETS, is_valid_buzzer_freq,
    play_melody_async,
};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use tabs::{TabKind, TabManager};
use workspace::{EngineSourceKind, NewProjectDraft, OpenedProject, run_doctor_async};

// 캔버스 한 칸(도트 1개)의 화면 픽셀 크기
const CELL_SIZE: f32 = 24.0;

// 하단 패널 탭 종류
#[derive(Default, PartialEq, Eq)]
enum BottomTab {
    #[default]
    Output, // CLI 실행 출력 로그
    Problems, // doctor 점검 및 cargo 빌드 문제 목록
    Serial,   // 왼쪽·오른쪽 모듈 시리얼 로그 뷰어
}

// 시리얼 로그 뷰어 배치 방식 (나란히 / 왼쪽만 / 오른쪽만)
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum SerialViewMode {
    #[default]
    Split, // 왼쪽·오른쪽 나란히 표시
    Left,  // 왼쪽 모듈만 크게 표시
    Right, // 오른쪽 모듈만 크게 표시
}

// 저장하지 않은 변경 사항이 있을 때 사용자 확인을 기다리는 대기 동작
#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingConfirm {
    Tab(TabKind), // 미저장 탭 하나 닫기 확인
    Project,      // 미저장 탭이 있는 상태에서 프로젝트 닫기 확인
    App,          // 미저장 탭이 있는 상태에서 앱 창 종료 확인
}

// 에셋 탐색기 관리 팝업(새로 만들기 · 이름 바꾸기 · 복제 · 삭제 확인) 상태
#[derive(Clone, PartialEq, Eq)]
enum AssetModal {
    Create {
        kind: AssetKind,
        name: String,
        error: String,
    },
    Rename {
        kind: AssetKind,
        old_name: String,
        new_name: String,
        error: String,
    },
    Duplicate {
        kind: AssetKind,
        src_name: String,
        dst_name: String,
        error: String,
    },
    Delete {
        kind: AssetKind,
        name: String,
        error: String,
    },
}

// 프로젝트 설정 탭 전용 편집 폼 상태
#[derive(Default)]
struct ProjectSettingsForm {
    game_path: String,      // [paths] game 상대 경로
    sim_path: String,       // [paths] sim 상대 경로
    firmware_path: String,  // [paths] firmware 상대 경로
    assets_path: String,    // [paths] assets 상대 경로
    left_port: String,      // .dotbox/local.toml 왼쪽 포트
    right_port: String,     // .dotbox/local.toml 오른쪽 포트
    status_message: String, // 저장 성공 안내 메시지
    error_message: String,  // 검증·저장 실패 오류 메시지
}

impl ProjectSettingsForm {
    // 열린 프로젝트의 현재 설정값으로 폼 입력칸 동기화
    fn sync_from(&mut self, opened: &OpenedProject) {
        let paths = &opened.project.manifest().paths;
        self.game_path = paths.game.display().to_string();
        self.sim_path = paths.sim.display().to_string();
        self.firmware_path = paths.firmware.display().to_string();
        self.assets_path = paths.assets.display().to_string();
        self.left_port = opened.local_config.ports.left.clone().unwrap_or_default();
        self.right_port = opened.local_config.ports.right.clone().unwrap_or_default();
        self.status_message.clear(); // 이전 안내 초기화
        self.error_message.clear(); // 이전 오류 초기화
    }
}

// 글꼴 탭 전용 UI 상태
struct FontTabState {
    font_path: String,                // 불러올 TTF/OTF 파일 경로
    config: FontConfig,               // 픽셀 크기, 임계값, 문자 집합 설정
    license: String,                  // 글꼴 라이선스 정보
    preview_text: String,             // 32×16 화면에 미리 볼 문구
    align_baseline: bool,             // true면 기준선(baseline) 정렬, false면 세로 중앙 정렬
    converter: Option<FontConverter>, // 불러온 폰트 변환기 (불러오기 전엔 None)
    status_message: String,           // 폰트 불러오기 안내 메시지
    asset_name: String,               // 저장·불러올 글꼴 이름 (fonts/<이름>.font.toml)
    io_status: String,                // .font.toml 저장/불러오기 결과 안내
}

impl Default for FontTabState {
    fn default() -> Self {
        Self {
            font_path: r"C:\Windows\Fonts\malgun.ttf".to_string(),
            config: FontConfig::default(),
            license: String::new(),
            preview_text: "DotBox".to_string(),
            align_baseline: true,
            converter: None,
            status_message: "TTF/OTF 경로를 입력하고 [폰트 불러오기]를 누르세요.".to_string(),
            asset_name: "main".to_string(),
            io_status: String::new(),
        }
    }
}

// 효과음·멜로디 탭 전용 UI 상태 (독립된 실행 취소/다시 실행 이력 포함)
struct SoundTabState {
    melody: MelodyEditor,                  // 음표 시퀀스 편집기
    history: CommandHistory<MelodyEditor>, // 파일별 독립 Undo/Redo 이력
    new_freq_hz: u16,                      // 새로 추가할 음표의 주파수 (Hz)
    new_duration_ms: u16,                  // 새로 추가할 음표의 길이 (ms)
    asset_name: String,                    // 저장·불러올 효과음 이름 (sounds/<이름>.sound.toml)
    status_message: String,                // 저장/불러오기 결과 안내
}

impl Default for SoundTabState {
    // 기본 음표 입력값(도 523Hz, 150ms)과 빈 이력으로 초기화
    fn default() -> Self {
        Self {
            melody: MelodyEditor::default(),
            history: CommandHistory::new(),
            new_freq_hz: 523,
            new_duration_ms: 150,
            asset_name: "jump".to_string(),
            status_message: String::new(),
        }
    }
}

impl SoundTabState {
    // 현재 음표 목록을 (주파수, 길이) 스냅샷 벡터로 추출
    fn snapshot_notes(&self) -> Vec<(u16, u16)> {
        self.melody
            .notes()
            .iter()
            .map(|n| (n.freq_hz, n.duration_ms))
            .collect()
    }

    // 음표 목록 변경을 명령 객체로 감싸 실행 및 Undo 스택에 기록
    fn commit_notes_change(&mut self, after: Vec<(u16, u16)>) {
        let before = self.snapshot_notes(); // 변경 전 스냅샷
        if before == after {
            return; // 변화가 없으면 불필요한 이력을 남기지 않음
        }
        let cmd = Box::new(ReplaceMelodyCommand::new(before, after));
        self.history.execute(&mut self.melody, cmd); // 명령 실행 및 이력 추가
    }
}

// 기기 패널 탭 전용 UI 상태 (프로젝트 경로는 열린 프로젝트 루트를 자동 사용)
struct DeviceTabState {
    dotbox_bin: String,                  // 실행할 dotbox CLI 경로 또는 명령 이름
    ports: Vec<DetectedPort>,            // 마지막으로 스캔한 시리얼 포트 목록
    port_status: String,                 // 포트 스캔 상태 메시지
    selected_target: FlashTarget,        // 플래시 대상 (좌측 / 우측 / 양쪽)
    selected_port: String,               // 플래시 시 지정할 시리얼 포트 (비우면 자동)
    running_cmd: Option<RunningCommand>, // 백그라운드 실행 중인 CLI 프로세스 핸들
    output_log: String,                  // 줄 단위로 누적되는 CLI 실행 출력 로그
}

impl Default for DeviceTabState {
    // 기본 CLI 명령 이름과 안내 문구로 초기화
    fn default() -> Self {
        Self {
            dotbox_bin: "dotbox".to_string(),
            ports: Vec::new(),
            port_status: "[시리얼 포트 새로고침]을 눌러 연결된 기기를 확인하세요.".to_string(),
            selected_target: FlashTarget::Left,
            selected_port: String::new(),
            running_cmd: None,
            output_log: "실행 로그가 여기에 표시됩니다.".to_string(),
        }
    }
}

// 장면(화면) 미리보기 탭 전용 UI 상태 (파일로 저장하지 않는 임시 배치기)
struct SceneTabState {
    scene: ScenePreview,                       // 32×16 장면 레이어 데이터
    selected_idx: Option<usize>,               // 현재 선택된 항목 인덱스 (캔버스 드래그 이동 대상)
    selected_sprite_name: String,              // 추가할 프로젝트 스프라이트 이름
    new_text: String,                          // 새로 추가할 텍스트 문구
    new_text_color: Rgb,                       // 새로 추가할 텍스트 색상
    use_loaded_font: bool,                     // true면 글꼴 탭에서 불러온 TTF/OTF 사용
    status_message: String,                    // 스프라이트 불러오기 오류·안내 메시지
    drag_anchor: Option<(i32, i32, i32, i32)>, // (시작 포인터 X, Y, 항목 원래 X, Y)
}

impl Default for SceneTabState {
    fn default() -> Self {
        Self {
            scene: ScenePreview::new(),
            selected_idx: None,
            selected_sprite_name: String::new(),
            new_text: "SCORE 00".to_string(),
            new_text_color: Rgb::WHITE,
            use_loaded_font: true,
            status_message: String::new(),
            drag_anchor: None,
        }
    }
}

// 시작 화면 전용 UI 상태 (폴더 열기 · 새 프로젝트 만들기 · 오류 표시)
#[derive(Default)]
struct StartScreenState {
    open_dir_input: String,         // [폴더 열기] 경로 입력칸
    show_new_form: bool,            // [새 프로젝트] 폼 펼침 여부
    new_draft: NewProjectDraft,     // 새 프로젝트 입력값
    error_message: Option<String>,  // 열기·생성 실패 시 오류 메시지
    notice_message: Option<String>, // 생성 시 경고 등 안내 메시지
}

// GUI 앱 전체 상태
struct StudioApp {
    settings: UserSettings,         // 사용자 전역 설정 (최근 목록, 창 크기, 밝기)
    opened: Option<OpenedProject>,  // 현재 열린 프로젝트 (없으면 시작 화면 표시)
    start_screen: StartScreenState, // 시작 화면 UI 상태
    settings_form: ProjectSettingsForm, // 프로젝트 설정 탭 입력 폼 상태
    doctor_report: Option<Report>,
    build_diagnostics: Vec<CargoDiagnostic>, // cargo 컴파일 에러/경고 목록  // 마지막 doctor::run 점검 결과
    doctor_rx: Option<Receiver<Report>>,     // 백그라운드 doctor 실행 수신기
    doctor_host_only: bool,                  // 시뮬레이터 전용 점검 옵션 여부
    tabs: TabManager,                        // 파일 단위 중앙 탭 관리자
    pending_confirm: Option<PendingConfirm>, // 미저장 탭/프로젝트/앱 닫기 확인 팝업 상태
    allow_app_close: bool,                   // 확인 팝업에서 앱 종료를 승인했는지 여부
    bottom_tab: BottomTab,                   // 현재 열려 있는 하단 패널 탭
    show_bottom_panel: bool,                 // 하단 패널 펼침 여부
    serial_left: SerialMonitor,              // 왼쪽 모듈 시리얼 모니터
    serial_right: SerialMonitor,             // 오른쪽 모듈 시리얼 모니터
    serial_view_mode: SerialViewMode,        // 시리얼 로그 배치 방식
    serial_search: String,                   // 시리얼 로그 검색어
    serial_save_status: String,              // 시리얼 로그 파일 저장 결과 안내
    editor: CanvasEditor,                    // 스프라이트·화면 편집기 상태
    canvas_history: CommandHistory<CanvasEditor>, // 스프라이트 탭 독립 Undo/Redo 이력
    canvas_drag_before: Option<SpriteSnapshot>, // 마우스 드래그 시작 시점의 스프라이트 스냅샷
    draw_tool: DrawTool,                     // 현재 선택된 스프라이트 그리기 도구
    shape_start: Option<(usize, usize)>,     // 직선·사각형 드래그 시작 좌표
    anim_playing: bool,                      // 애니메이션 재생 미리보기 실행 여부
    anim_elapsed_ms: f32,                    // 현재 프레임 경과 시간 (ms)
    font_tab: FontTabState,                  // 글꼴 변환기 상태
    sound_tab: SoundTabState,                // 효과음·멜로디 편집기 상태 (Undo/Redo 포함)
    device_tab: DeviceTabState,              // 기기 패널 상태
    scene_tab: SceneTabState,                // 장면(화면) 미리보기 상태
    sprite_asset_name: String,               // 저장·불러올 스프라이트 이름 (sprites/<이름>.png)
    sprite_io_status: String,                // 저장/불러오기 결과 안내
    sprite_files: Vec<String>,               // 탐색기에 표시할 assets/sprites/*.png 목록
    sound_files: Vec<String>,                // 탐색기에 표시할 assets/sounds/*.sound.toml 목록
    font_files: Vec<String>,                 // 탐색기에 표시할 assets/fonts/*.font.toml 목록
    other_files: Vec<String>,                // 탐색기에 "기타"로 표시할 알 수 없는 파일 목록
    has_icon: bool,                          // assets/icon.png 파일 존재 여부
    editing_icon: bool, // true면 스프라이트 편집기가 assets/icon.png(16×16) 편집 모드
    asset_modal: Option<AssetModal>, // 에셋 생성·이름변경·복제·삭제 모달 상태
    explorer_status: String, // 에셋 관리 작업 결과 안내
}

impl StudioApp {
    // 실행 인자(DIR)와 불러온 사용자 설정으로 앱 초기화
    fn new(initial_dir: Option<PathBuf>, settings: UserSettings) -> Self {
        // 사용자 설정에 저장된 전역 밝기 미리보기 값을 편집기에 반영
        let mut editor = CanvasEditor::new();
        editor.set_led_preview(settings.led_preview);
        editor.set_brightness_percent(settings.brightness_percent);

        let mut app = Self {
            settings,
            opened: None,
            start_screen: StartScreenState::default(),
            settings_form: ProjectSettingsForm::default(),
            doctor_report: None,
            build_diagnostics: Vec::new(),
            doctor_rx: None,
            doctor_host_only: false,
            tabs: TabManager::new(), // [개요] 탭 기본 열림
            pending_confirm: None,
            allow_app_close: false, // 기본값: 미저장 탭이 있으면 종료 차단
            bottom_tab: BottomTab::Output,
            show_bottom_panel: true, // 하단 패널 기본 표시
            serial_left: SerialMonitor::new(),
            serial_right: SerialMonitor::new(),
            serial_view_mode: SerialViewMode::Split,
            serial_search: String::new(),
            serial_save_status: String::new(),
            editor,
            canvas_history: CommandHistory::new(), // 깨끗한 이력으로 초기화
            canvas_drag_before: None,              // 드래그 중 아님
            draw_tool: DrawTool::Pencil,
            shape_start: None,
            anim_playing: false,
            anim_elapsed_ms: 0.0,
            font_tab: FontTabState::default(),
            sound_tab: SoundTabState::default(),
            device_tab: DeviceTabState::default(),
            scene_tab: SceneTabState::default(),
            sprite_asset_name: "player".to_string(),
            sprite_io_status: String::new(),
            sprite_files: Vec::new(),
            sound_files: Vec::new(),
            font_files: Vec::new(),
            other_files: Vec::new(),
            has_icon: false,
            editing_icon: false,
            asset_modal: None,
            explorer_status: String::new(),
        };

        // 실행 인자로 경로가 들어오면 시작하자마자 프로젝트 열기 시도
        if let Some(dir) = initial_dir {
            app.open_project_path(&dir);
        }

        app
    }

    // 지정한 경로에서 프로젝트를 열고 최근 목록·설정 폼·doctor 점검을 자동 시작
    fn open_project_path(&mut self, dir: &Path) {
        match OpenedProject::open(dir) {
            Ok(opened) => {
                let root = opened.project.root().to_path_buf(); // 프로젝트 루트 절대 경로
                // 이전 프로젝트에서 실행 중이던 CLI가 있으면 중지
                if let Some(mut cmd) = self.device_tab.running_cmd.take() {
                    cmd.stop();
                }
                // 기기 패널 및 시리얼 모니터 포트 동기화 (기존 연결은 해제)
                self.serial_left.disconnect();
                self.serial_right.disconnect();
                self.serial_left.port_name =
                    opened.local_config.ports.left.clone().unwrap_or_default();
                self.serial_right.port_name =
                    opened.local_config.ports.right.clone().unwrap_or_default();
                self.device_tab.selected_port = self.serial_left.port_name.clone();
                // 프로젝트 설정 폼 동기화
                self.settings_form.sync_from(&opened);
                // 최근 프로젝트 목록 맨 앞에 추가 후 사용자 설정 저장
                self.settings.push_recent(root);
                let _ = self.settings.save();
                // 새 프로젝트를 열 때 탭·편집 이력 초기화 후 [개요] 탭 활성화
                self.opened = Some(opened);
                self.refresh_asset_lists();
                self.tabs = TabManager::new();
                self.sound_tab.history.reset();
                self.canvas_history.reset(); // 스프라이트 편집 이력 초기화
                self.canvas_drag_before = None;
                self.shape_start = None;
                self.editing_icon = false;
                self.anim_playing = false;
                self.anim_elapsed_ms = 0.0;
                self.scene_tab = SceneTabState::default();
                self.asset_modal = None;
                self.explorer_status.clear();
                self.pending_confirm = None;
                self.start_screen.error_message = None;
                // 프로젝트 열기 직후 환경 점검(doctor)을 백그라운드로 자동 시작
                self.start_doctor_check();
            }
            Err(err) => {
                // 열기 실패 시 시작 화면으로 유지하고 오류 메시지 표시
                self.opened = None;
                self.start_screen.error_message = Some(err);
            }
        }
    }

    // 프로젝트 닫기 요청: 저장하지 않은 탭이 있으면 확인 팝업을 먼저 표시
    fn request_close_project(&mut self) {
        if self.tabs.has_any_dirty(|kind| self.is_tab_dirty(kind)) {
            self.pending_confirm = Some(PendingConfirm::Project); // 확인 팝업 띄우기
        } else {
            self.force_close_project(); // 미저장 변경이 없으면 바로 닫기
        }
    }

    // 확인 없이 즉시 프로젝트를 닫고 시작 화면으로 복귀
    fn force_close_project(&mut self) {
        // 프로젝트 닫기 시 실행 중인 시뮬레이터·빌드 프로세스 및 시리얼 연결 정리
        if let Some(mut cmd) = self.device_tab.running_cmd.take() {
            cmd.stop();
        }
        self.serial_left.disconnect();
        self.serial_right.disconnect();
        self.opened = None; // 열린 프로젝트 비우기
        self.doctor_report = None; // doctor 결과 정리
        self.build_diagnostics.clear();
        self.doctor_rx = None; // doctor 수신기 정리
        self.tabs = TabManager::new(); // 탭 목록 초기화
        self.sound_tab.history.reset(); // 효과음 편집 이력 초기화
        self.canvas_history.reset(); // 스프라이트 편집 이력 초기화
        self.canvas_drag_before = None;
        self.shape_start = None;
        self.editing_icon = false;
        self.anim_playing = false;
        self.anim_elapsed_ms = 0.0;
        self.sprite_files.clear();
        self.sound_files.clear();
        self.font_files.clear();
        self.other_files.clear();
        self.has_icon = false;
        self.scene_tab = SceneTabState::default();
        self.asset_modal = None;
        self.explorer_status.clear();
        self.pending_confirm = None; // 확인 팝업 닫기
    }

    // 특정 탭 닫기 요청: 저장하지 않은 변경이 있으면 확인 팝업 표시
    fn request_close_tab(&mut self, kind: TabKind) {
        if !kind.is_closable() {
            return; // 개요 탭은 닫기 불가
        }
        if self.is_tab_dirty(kind) {
            self.pending_confirm = Some(PendingConfirm::Tab(kind)); // 확인 팝업 띄우기
        } else {
            self.tabs.close(kind); // 변경 없으면 바로 닫기
        }
    }

    // 특정 탭에 저장되지 않은 변경 사항(●)이 있는지 조회
    fn is_tab_dirty(&self, kind: TabKind) -> bool {
        match kind {
            TabKind::SpriteEditor => self.canvas_history.is_dirty(), // 스프라이트 이력 기준
            TabKind::SoundTool => self.sound_tab.history.is_dirty(), // 효과음 이력 기준
            _ => false,                                              // 그 외 탭은 읽기/즉시 저장
        }
    }

    // 현재 OS 창 크기를 읽어 사용자 설정(studio.toml)에 저장
    fn save_window_size(&mut self, ctx: &egui::Context) {
        if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
            self.settings
                .update_window_size(rect.width(), rect.height()); // 최소 크기 보정 후 반영
            let _ = self.settings.save(); // studio.toml 파일로 저장
        }
    }

    // OS 창 닫기(X) 버튼 클릭 시 미저장 탭 확인·실행 중 CLI 정리·창 크기 저장 처리
    fn handle_window_close(&mut self, ctx: &egui::Context) {
        // 창 닫기 요청이 없는 일반 프레임이면 즉시 반환
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }

        // 확인 팝업에서 이미 종료를 승인한 경우 CLI 정리·창 크기 저장 후 종료 진행
        if self.allow_app_close {
            self.stop_cli_command();
            self.serial_left.disconnect();
            self.serial_right.disconnect();
            self.save_window_size(ctx);
            return;
        }

        // 열린 프로젝트에 저장하지 않은 탭이 하나라도 있는지 검사
        let has_dirty =
            self.opened.is_some() && self.tabs.has_any_dirty(|kind| self.is_tab_dirty(kind));
        if has_dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose); // 즉시 종료 취소
            self.pending_confirm = Some(PendingConfirm::App); // 종료 확인 팝업 표시
        } else {
            self.stop_cli_command(); // 실행 중인 자식 프로세스 정리
            self.serial_left.disconnect();
            self.serial_right.disconnect();
            self.save_window_size(ctx); // 미저장 변경이 없으면 창 크기 저장 후 바로 종료
        }
    }

    // 열린 프로젝트의 에셋 파일 목록(sprites, sounds, fonts, 기타, icon.png)을 다시 스캔
    fn refresh_asset_lists(&mut self) {
        let Some(assets_dir) = self.opened.as_ref().map(|o| o.project.assets_dir()) else {
            self.sprite_files.clear();
            self.sound_files.clear();
            self.font_files.clear();
            self.other_files.clear();
            self.has_icon = false;
            return;
        };
        self.sprite_files = list_sprite_names(&assets_dir);
        self.sound_files = list_sound_names(&assets_dir);
        self.font_files = list_font_names(&assets_dir);
        self.other_files = list_other_files(&assets_dir);
        self.has_icon = has_icon_png(&assets_dir);
    }

    // 지정한 종류와 이름의 에셋을 불러와 해당 편집기 탭 열기
    fn open_asset_by_kind(&mut self, kind: AssetKind, name: String) {
        match kind {
            AssetKind::Sprite => {
                self.editing_icon = false;
                self.anim_playing = false;
                self.anim_elapsed_ms = 0.0;
                self.sprite_asset_name = name;
                self.load_current_sprite();
                self.tabs.open(TabKind::SpriteEditor);
            }
            AssetKind::Sound => {
                self.sound_tab.asset_name = name;
                self.load_current_sound();
                self.tabs.open(TabKind::SoundTool);
            }
            AssetKind::Font => {
                self.font_tab.asset_name = name;
                self.load_current_font();
                self.tabs.open(TabKind::FontTool);
            }
        }
    }

    // 현재 스프라이트(또는 아이콘 모드면 assets/icon.png)를 PNG로 저장
    fn save_current_sprite(&mut self) {
        let Some(assets_dir) = self.opened.as_ref().map(|o| o.project.assets_dir()) else {
            return;
        };

        let path = if self.editing_icon {
            assets_dir.join("icon.png") // 16×16 아이콘 고정 경로
        } else {
            let name = self.sprite_asset_name.trim().to_string();
            if let Err(err) = validate_asset_name(&name) {
                self.sprite_io_status = format!("오류: {err}");
                return;
            }
            assets_dir.join("sprites").join(format!("{name}.png"))
        };

        let width = self.editor.width();
        let height = self.editor.height();
        let pixels = snapshot_canvas(&self.editor);

        match save_sprite_png(&path, width, height, &pixels) {
            Ok(()) => {
                self.canvas_history.mark_saved(); // ● 더티 표시 해제
                self.refresh_asset_lists();
                self.sprite_io_status = format!("저장 완료: {}", path.display());
            }
            Err(err) => self.sprite_io_status = format!("오류: {err}"),
        }
    }

    // assets/icon.png(16×16 고정)를 열거나, 아직 없으면 16×16 빈 캔버스로 아이콘 편집 시작
    fn open_icon_png(&mut self) {
        let Some(assets_dir) = self.opened.as_ref().map(|o| o.project.assets_dir()) else {
            return;
        };
        self.editing_icon = true;
        let path = assets_dir.join("icon.png");

        if path.is_file() {
            match load_sprite_png(&path) {
                Ok((16, 16, pixels)) => {
                    self.editor.load_single_frame(16, 16, &pixels);
                    self.canvas_history.reset();
                    self.canvas_drag_before = None;
                    self.shape_start = None;
                    self.anim_playing = false;
                    self.anim_elapsed_ms = 0.0;
                    self.sprite_io_status = format!("아이콘 불러오기 완료: {}", path.display());
                }
                Ok((w, h, _)) => {
                    self.sprite_io_status =
                        format!("오류: icon.png는 16×16 크기여야 합니다 ({w}×{h}).");
                }
                Err(err) => self.sprite_io_status = format!("오류: {err}"),
            }
        } else {
            // 파일이 아직 없으면 16×16 빈 캔버스로 새로 생성 준비
            self.editor.set_layout(ScreenLayout::Mirrored);
            self.canvas_history.reset();
            self.canvas_drag_before = None;
            self.shape_start = None;
            self.anim_playing = false;
            self.anim_elapsed_ms = 0.0;
            self.sprite_io_status =
                "새 icon.png (16×16) 편집 중 — Ctrl+S로 저장하세요.".to_string();
        }
        self.tabs.open(TabKind::SpriteEditor);
    }

    // 현재 글꼴 설정을 <프로젝트>/assets/fonts/<이름>.font.toml 로 저장
    fn save_current_font(&mut self) {
        let Some(assets_dir) = self.opened.as_ref().map(|o| o.project.assets_dir()) else {
            return;
        };
        let name = self.font_tab.asset_name.trim().to_string();
        if let Err(err) = validate_asset_name(&name) {
            self.font_tab.io_status = format!("오류: {err}");
            return;
        }

        let path = assets_dir.join("fonts").join(format!("{name}.font.toml"));
        let data = FontAssetFile {
            source: self.font_tab.font_path.clone(),
            pixel_size: self.font_tab.config.pixel_size,
            threshold: self.font_tab.config.threshold,
            charset: self.font_tab.config.charset.clone(),
            license: self.font_tab.license.clone(),
        };

        match save_font_toml(&path, &data) {
            Ok(()) => {
                self.refresh_asset_lists();
                self.font_tab.io_status = format!("저장 완료: {}", path.display());
            }
            Err(err) => self.font_tab.io_status = format!("오류: {err}"),
        }
    }

    // <프로젝트>/assets/fonts/<이름>.font.toml 을 불러오고 원본 TTF/OTF도 자동 로드
    fn load_current_font(&mut self) {
        let Some(assets_dir) = self.opened.as_ref().map(|o| o.project.assets_dir()) else {
            return;
        };
        let name = self.font_tab.asset_name.trim().to_string();
        if let Err(err) = validate_asset_name(&name) {
            self.font_tab.io_status = format!("오류: {err}");
            return;
        }

        let path = assets_dir.join("fonts").join(format!("{name}.font.toml"));
        match load_font_toml(&path) {
            Ok(data) => {
                self.font_tab.font_path = data.source;
                self.font_tab.config.pixel_size = data.pixel_size;
                self.font_tab.config.threshold = data.threshold;
                self.font_tab.config.charset = data.charset;
                self.font_tab.license = data.license;
                self.font_tab.io_status = format!("불러오기 완료: {}", path.display());

                // 설정에 적힌 원본 TTF/OTF 파일도 바로 불러와 미리보기 갱신
                match FontConverter::from_file(Path::new(&self.font_tab.font_path)) {
                    Ok(converter) => {
                        self.font_tab.converter = Some(converter);
                        self.font_tab.status_message = "폰트 불러오기 완료!".to_string();
                    }
                    Err(err) => {
                        self.font_tab.converter = None;
                        self.font_tab.status_message = err;
                    }
                }
            }
            Err(err) => self.font_tab.io_status = format!("오류: {err}"),
        }
    }

    // <프로젝트>/assets/sprites/<이름>.png 를 불러와 캔버스에 반영 (1×1 ~ 32×16 지원)
    fn load_current_sprite(&mut self) {
        let Some(assets_dir) = self.opened.as_ref().map(|o| o.project.assets_dir()) else {
            return;
        };
        let name = self.sprite_asset_name.trim().to_string();
        if let Err(err) = validate_asset_name(&name) {
            self.sprite_io_status = format!("오류: {err}");
            return;
        }

        let path = assets_dir.join("sprites").join(format!("{name}.png"));
        match load_sprite_png(&path) {
            Ok((w, h, pixels)) if (1..=32).contains(&w) && (1..=16).contains(&h) => {
                self.editing_icon = false;
                self.editor.load_single_frame(w, h, &pixels);
                self.canvas_history.reset();
                self.canvas_drag_before = None;
                self.shape_start = None;
                self.anim_playing = false;
                self.anim_elapsed_ms = 0.0;
                self.sprite_io_status = format!("불러오기 완료: {} ({w}×{h})", path.display());
            }
            Ok((w, h, _)) => {
                self.sprite_io_status =
                    format!("오류: 지원 크기(1×1 ~ 32×16)를 벗어났습니다 ({w}×{h}).");
            }
            Err(err) => self.sprite_io_status = format!("오류: {err}"),
        }
    }

    // 현재 효과음을 <프로젝트>/assets/sounds/<이름>.sound.toml 로 저장
    fn save_current_sound(&mut self) {
        let Some(assets_dir) = self.opened.as_ref().map(|o| o.project.assets_dir()) else {
            return;
        };
        let name = self.sound_tab.asset_name.trim().to_string();
        if let Err(err) = validate_asset_name(&name) {
            self.sound_tab.status_message = format!("오류: {err}");
            return;
        }

        let path = assets_dir.join("sounds").join(format!("{name}.sound.toml"));
        match save_sound_toml(&path, self.sound_tab.melody.notes()) {
            Ok(()) => {
                self.sound_tab.history.mark_saved(); // ● 더티 표시 해제
                self.refresh_asset_lists();
                self.sound_tab.status_message = format!("저장 완료: {}", path.display());
            }
            Err(err) => self.sound_tab.status_message = format!("오류: {err}"),
        }
    }

    // <프로젝트>/assets/sounds/<이름>.sound.toml 을 불러와 편집기에 반영
    fn load_current_sound(&mut self) {
        let Some(assets_dir) = self.opened.as_ref().map(|o| o.project.assets_dir()) else {
            return;
        };
        let name = self.sound_tab.asset_name.trim().to_string();
        if let Err(err) = validate_asset_name(&name) {
            self.sound_tab.status_message = format!("오류: {err}");
            return;
        }

        let path = assets_dir.join("sounds").join(format!("{name}.sound.toml"));
        match load_sound_toml(&path) {
            Ok(notes) => {
                self.sound_tab.melody.clear();
                for note in notes {
                    self.sound_tab
                        .melody
                        .add_note(note.freq_hz, note.duration_ms);
                }
                self.sound_tab.history.reset();
                self.sound_tab.status_message = format!("불러오기 완료: {}", path.display());
            }
            Err(err) => self.sound_tab.status_message = format!("오류: {err}"),
        }
    }

    // 전역 키보드 단축키(Ctrl+S 저장, Ctrl+Z 실행 취소, Ctrl+Y 다시 실행, Ctrl+W 현재 탭 닫기) 처리
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Ctrl+S: 텍스트 입력칸 포커스 여부와 관계없이 현재 탭 즉시 저장
        let want_save =
            ctx.input(|i| i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::S));
        if want_save {
            match self.tabs.active() {
                TabKind::SpriteEditor => self.save_current_sprite(),
                TabKind::SoundTool => self.save_current_sound(),
                TabKind::FontTool => self.save_current_font(), // 글꼴 설정 저장
                _ => {}
            }
            return;
        }

        // 텍스트 입력칸에 포커스가 있을 때는 텍스트 자체 단축키와 충돌하지 않도록 건너뜀
        if ctx.egui_wants_keyboard_input() {
            return;
        }

        let (want_undo, want_redo, want_close_tab) = ctx.input(|i| {
            let undo = i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z);
            let redo = (i.modifiers.command && i.key_pressed(egui::Key::Y))
                || (i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Z));
            let close_tab = i.modifiers.command && i.key_pressed(egui::Key::W); // 현재 탭 닫기
            (undo, redo, close_tab)
        });

        // Ctrl+W 입력 시 현재 활성 탭 닫기 요청 (더티면 확인 팝업 표시)
        if want_close_tab {
            self.request_close_tab(self.tabs.active());
            return;
        }

        // 현재 활성 탭에 맞는 독립 이력에 Undo/Redo 적용
        match self.tabs.active() {
            TabKind::SpriteEditor => {
                if want_undo {
                    self.canvas_history.undo(&mut self.editor); // 스프라이트 실행 취소
                } else if want_redo {
                    self.canvas_history.redo(&mut self.editor); // 스프라이트 다시 실행
                }
            }
            TabKind::SoundTool => {
                if want_undo {
                    self.sound_tab.history.undo(&mut self.sound_tab.melody); // 효과음 실행 취소
                } else if want_redo {
                    self.sound_tab.history.redo(&mut self.sound_tab.melody); // 효과음 다시 실행
                }
            }
            _ => {}
        }
    }

    // 현재 열린 프로젝트 기준으로 dotbox_tools::doctor::run 비동기 시작
    fn start_doctor_check(&mut self) {
        let Some(opened) = &self.opened else {
            return;
        };
        let rx = run_doctor_async(opened.project.clone(), self.doctor_host_only);
        self.doctor_rx = Some(rx); // 수신 채널 저장
    }

    // 매 프레임 백그라운드 작업(doctor, CLI 명령) 완료 여부를 확인해 상태 갱신
    fn poll_background_tasks(&mut self, ctx: &egui::Context) {
        // 1) doctor::run 결과 수신 확인
        if let Some(rx) = &self.doctor_rx {
            match rx.try_recv() {
                Ok(report) => {
                    self.doctor_report = Some(report); // 완료된 점검 결과 반영
                    self.doctor_rx = None; // 수신기 정리
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint(); // 점검 중이면 계속 화면 갱신 요청
                }
                Err(TryRecvError::Disconnected) => {
                    self.doctor_rx = None; // 채널 끊김 시 정리
                }
            }
        }

        // 2) 실행 중인 CLI 프로세스의 줄 단위 출력 수신 및 종료 확인
        if let Some(cmd) = &mut self.device_tab.running_cmd {
            // 새로 도착한 출력 줄을 즉시 로그 끝에 추가 및 cargo JSON 진단 파싱
            for line in cmd.poll_lines() {
                if let Some(diag) = parse_cargo_json_line(&line) {
                    self.build_diagnostics.push(diag);
                }
                self.device_tab.output_log.push_str(&line);
                self.device_tab.output_log.push('\n');
            }

            // 프로세스가 끝났는지 확인
            if let Some(status_line) = cmd.try_finish() {
                // 종료 직전 파이프에 남은 마지막 줄까지 모두 비우기
                for line in cmd.poll_lines() {
                    if let Some(diag) = parse_cargo_json_line(&line) {
                        self.build_diagnostics.push(diag);
                    }
                    self.device_tab.output_log.push_str(&line);
                    self.device_tab.output_log.push('\n');
                }
                self.device_tab.output_log.push_str(&status_line);
                self.device_tab.output_log.push('\n');
                self.device_tab.running_cmd = None; // 실행 핸들 정리
            } else {
                ctx.request_repaint(); // 실행 중이면 계속 실시간 갱신 요청
            }
        }

        // 3) 왼쪽·오른쪽 시리얼 모니터 수신 폴링
        let left_active = self.serial_left.poll();
        let right_active = self.serial_right.poll();
        if left_active || right_active {
            ctx.request_repaint(); // 시리얼 연결 중이면 실시간 로그 표시 갱신
        }
    }

    // 시리얼 포트 목록을 다시 스캔해 device_tab 상태 갱신
    fn refresh_serial_ports(&mut self) {
        match scan_serial_ports() {
            Ok(ports) => {
                self.device_tab.port_status = format!("감지된 포트: {}개", ports.len());
                self.device_tab.ports = ports;
            }
            Err(err) => {
                self.device_tab.ports.clear();
                self.device_tab.port_status = err;
            }
        }
    }

    // 저장하지 않은 변경 사항 닫기 확인 모달 창 표시
    fn show_confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_confirm else {
            return; // 확인 대기 중인 동작이 없으면 표시 안 함
        };

        // 대기 동작에 맞는 안내 문구와 확인 버튼 라벨 결정
        let (message, confirm_label) = match pending {
            PendingConfirm::Tab(kind) => (
                format!(
                    "'{}' 탭에 저장하지 않은 변경 사항이 있습니다.\n변경 사항을 버리고 닫을까요?",
                    kind.title()
                ),
                "버리고 닫기",
            ),
            PendingConfirm::Project => (
                "저장하지 않은 탭이 있습니다.\n변경 사항을 버리고 프로젝트를 닫을까요?".to_string(),
                "버리고 닫기",
            ),
            PendingConfirm::App => (
                "저장하지 않은 탭이 있습니다.\n변경 사항을 버리고 DotBox Studio를 종료할까요?"
                    .to_string(),
                "버리고 종료",
            ),
        };

        // 화면 중앙에 고정된 확인 팝업 표시
        egui::Window::new("저장하지 않은 변경 사항")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(message);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button(confirm_label).clicked() {
                        match pending {
                            PendingConfirm::Tab(kind) => {
                                // 탭을 닫고 해당 편집기 이력 초기화
                                if kind == TabKind::SoundTool {
                                    self.sound_tab.history.reset();
                                }
                                if kind == TabKind::SpriteEditor {
                                    self.canvas_history.reset();
                                    self.canvas_drag_before = None;
                                    self.shape_start = None;
                                }
                                self.tabs.close(kind);
                                self.pending_confirm = None;
                            }
                            PendingConfirm::Project => {
                                self.force_close_project(); // 프로젝트 강제 닫기
                            }
                            PendingConfirm::App => {
                                self.allow_app_close = true; // 다음 프레임 종료 허용
                                self.pending_confirm = None; // 팝업 닫기
                                self.stop_cli_command(); // 실행 중인 자식 프로세스 정리
                                self.save_window_size(ctx); // 마지막 창 크기 저장
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close); // 앱 종료 실행
                            }
                        }
                    }
                    if ui.button("취소").clicked() {
                        self.pending_confirm = None; // 닫기·종료 취소
                    }
                });
            });
    }

    // 에셋 탐색기 관리 모달 창 렌더링 (새로 만들기 · 이름 바꾸기 · 복제 · 삭제 확인)
    fn show_asset_modal(&mut self, ctx: &egui::Context) {
        let Some(mut modal) = self.asset_modal.take() else {
            return;
        };
        let Some(assets_dir) = self.opened.as_ref().map(|o| o.project.assets_dir()) else {
            return;
        };

        let mut keep_open = true;
        let mut open_after: Option<(AssetKind, String)> = None;

        match &mut modal {
            AssetModal::Create { kind, name, error } => {
                egui::Window::new("새 에셋 만들기")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("종류:");
                            for candidate in [AssetKind::Sprite, AssetKind::Sound, AssetKind::Font]
                            {
                                ui.selectable_value(kind, candidate, candidate.label());
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("이름:");
                            ui.add(
                                egui::TextEdit::singleline(name)
                                    .hint_text("예: player_run")
                                    .desired_width(180.0),
                            );
                        });
                        let trimmed = name.trim();
                        let validation = validate_asset_name(trimmed);
                        if let Err(msg) = &validation {
                            ui.colored_label(egui::Color32::from_rgb(240, 200, 80), msg);
                        }
                        if !error.is_empty() {
                            ui.colored_label(
                                egui::Color32::from_rgb(240, 100, 100),
                                error.as_str(),
                            );
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(validation.is_ok(), egui::Button::new("만들기"))
                                .clicked()
                            {
                                match create_default_asset(&assets_dir, *kind, trimmed) {
                                    Ok(_) => {
                                        self.explorer_status = format!("생성 완료: {trimmed}");
                                        open_after = Some((*kind, trimmed.to_string()));
                                        keep_open = false;
                                    }
                                    Err(err) => *error = err,
                                }
                            }
                            if ui.button("취소").clicked() {
                                keep_open = false;
                            }
                        });
                    });
            }
            AssetModal::Rename {
                kind,
                old_name,
                new_name,
                error,
            } => {
                egui::Window::new("에셋 이름 바꾸기")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.label(format!("대상: {} ({old_name})", kind.label()));
                        ui.horizontal(|ui| {
                            ui.label("새 이름:");
                            ui.add(egui::TextEdit::singleline(new_name).desired_width(180.0));
                        });
                        let trimmed = new_name.trim();
                        let validation = validate_asset_name(trimmed);
                        if let Err(msg) = &validation {
                            ui.colored_label(egui::Color32::from_rgb(240, 200, 80), msg);
                        }
                        if !error.is_empty() {
                            ui.colored_label(
                                egui::Color32::from_rgb(240, 100, 100),
                                error.as_str(),
                            );
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(validation.is_ok(), egui::Button::new("변경"))
                                .clicked()
                            {
                                match rename_asset(&assets_dir, *kind, old_name, trimmed) {
                                    Ok(_) => {
                                        // 현재 편집 중인 에셋 이름도 함께 갱신
                                        match *kind {
                                            AssetKind::Sprite
                                                if self.sprite_asset_name == *old_name =>
                                            {
                                                self.sprite_asset_name = trimmed.to_string();
                                            }
                                            AssetKind::Sound
                                                if self.sound_tab.asset_name == *old_name =>
                                            {
                                                self.sound_tab.asset_name = trimmed.to_string();
                                            }
                                            AssetKind::Font
                                                if self.font_tab.asset_name == *old_name =>
                                            {
                                                self.font_tab.asset_name = trimmed.to_string();
                                            }
                                            _ => {}
                                        }
                                        self.explorer_status =
                                            format!("이름 변경: {old_name} → {trimmed}");
                                        keep_open = false;
                                    }
                                    Err(err) => *error = err,
                                }
                            }
                            if ui.button("취소").clicked() {
                                keep_open = false;
                            }
                        });
                    });
            }
            AssetModal::Duplicate {
                kind,
                src_name,
                dst_name,
                error,
            } => {
                egui::Window::new("에셋 복제")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.label(format!("원본: {} ({src_name})", kind.label()));
                        ui.horizontal(|ui| {
                            ui.label("복제 이름:");
                            ui.add(egui::TextEdit::singleline(dst_name).desired_width(180.0));
                        });
                        let trimmed = dst_name.trim();
                        let validation = validate_asset_name(trimmed);
                        if let Err(msg) = &validation {
                            ui.colored_label(egui::Color32::from_rgb(240, 200, 80), msg);
                        }
                        if !error.is_empty() {
                            ui.colored_label(
                                egui::Color32::from_rgb(240, 100, 100),
                                error.as_str(),
                            );
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(validation.is_ok(), egui::Button::new("복제"))
                                .clicked()
                            {
                                match duplicate_asset(&assets_dir, *kind, src_name, trimmed) {
                                    Ok(_) => {
                                        self.explorer_status =
                                            format!("복제 완료: {src_name} → {trimmed}");
                                        open_after = Some((*kind, trimmed.to_string()));
                                        keep_open = false;
                                    }
                                    Err(err) => *error = err,
                                }
                            }
                            if ui.button("취소").clicked() {
                                keep_open = false;
                            }
                        });
                    });
            }
            AssetModal::Delete { kind, name, error } => {
                egui::Window::new("에셋 삭제 확인")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.label(format!(
                            "'{}' ({}) 파일을 삭제할까요?
삭제한 파일은 되돌릴 수 없습니다.",
                            name,
                            kind.label()
                        ));
                        if !error.is_empty() {
                            ui.colored_label(
                                egui::Color32::from_rgb(240, 100, 100),
                                error.as_str(),
                            );
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("삭제").clicked() {
                                match delete_asset(&assets_dir, *kind, name) {
                                    Ok(()) => {
                                        self.explorer_status = format!("삭제 완료: {name}");
                                        keep_open = false;
                                    }
                                    Err(err) => *error = err,
                                }
                            }
                            if ui.button("취소").clicked() {
                                keep_open = false;
                            }
                        });
                    });
            }
        }

        if keep_open {
            self.asset_modal = Some(modal);
        } else {
            self.refresh_asset_lists();
            if let Some((kind, name)) = open_after {
                self.open_asset_by_kind(kind, name);
            }
        }
    }

    // 중앙 작업 공간 상단의 파일 단위 탭 바 렌더링 ([개요] [효과음 편집기 ● ×] ...)
    fn show_editor_tab_bar(&mut self, ui: &mut egui::Ui) {
        let open_tabs = self.tabs.open_tabs().to_vec(); // 순회 중 변경 충돌 방지 복사본
        let active = self.tabs.active();
        let mut activate_target: Option<TabKind> = None;
        let mut close_target: Option<TabKind> = None;

        ui.horizontal_wrapped(|ui| {
            for kind in open_tabs {
                let dirty = self.is_tab_dirty(kind); // 더티 상태 조회
                let label = TabManager::tab_label(kind, dirty); // 더티면 뒤에 ● 추가
                let is_active = kind == active;

                // 탭 이름 클릭 시 활성화
                if ui.selectable_label(is_active, label).clicked() {
                    activate_target = Some(kind);
                }
                // 닫기 가능한 탭에만 × 버튼 표시
                if kind.is_closable() && ui.small_button("×").clicked() {
                    close_target = Some(kind);
                }
                ui.separator();
            }
        });

        // 루프 종료 후 탭 전환 및 닫기 요청 반영
        if let Some(kind) = activate_target {
            self.tabs.set_active(kind);
        }
        if let Some(kind) = close_target {
            self.request_close_tab(kind);
        }
    }

    // 시작 화면 렌더링 (최근 프로젝트 · 폴더 열기 · 새 프로젝트)
    fn show_start_screen(&mut self, ui: &mut egui::Ui) {
        ui.heading("DotBox Studio");
        ui.label("게임 프로젝트를 열거나 새로 만들어 시작하세요.");
        ui.separator();
        ui.add_space(6.0);

        // 오류 메시지가 있으면 빨간색으로 강조 표시
        if let Some(err) = &self.start_screen.error_message {
            ui.colored_label(egui::Color32::from_rgb(240, 100, 100), err);
            ui.add_space(6.0);
        }
        // 생성 시 경로 경고 등 안내 메시지 표시
        if let Some(notice) = &self.start_screen.notice_message {
            ui.colored_label(egui::Color32::from_rgb(240, 200, 80), notice);
            ui.add_space(6.0);
        }

        // 1) 폴더 열기 입력칸과 새 프로젝트 폼 토글 버튼
        ui.horizontal(|ui| {
            ui.label("프로젝트 폴더:");
            ui.add(
                egui::TextEdit::singleline(&mut self.start_screen.open_dir_input)
                    .hint_text("예: C:\\dev\\my-game")
                    .desired_width(360.0),
            );
            if ui.button("폴더 열기").clicked() {
                let path = PathBuf::from(self.start_screen.open_dir_input.trim());
                if !path.as_os_str().is_empty() {
                    self.open_project_path(&path);
                }
            }
            ui.separator();
            let toggle_text = if self.start_screen.show_new_form {
                "새 프로젝트 닫기"
            } else {
                "+ 새 프로젝트"
            };
            if ui.button(toggle_text).clicked() {
                self.start_screen.show_new_form = !self.start_screen.show_new_form;
            }
        });
        ui.add_space(8.0);

        // 2) [새 프로젝트] 폼 (펼쳐졌을 때만 표시)
        if self.start_screen.show_new_form {
            self.show_new_project_form(ui);
            ui.add_space(8.0);
        }

        ui.separator();
        ui.add_space(6.0);

        // 3) 최근 프로젝트 목록 (사라진 경로는 흐리게 표시 + 목록에서 제거 가능)
        ui.heading("최근 프로젝트");
        ui.add_space(4.0);

        if self.settings.recent_projects.is_empty() {
            ui.label("최근에 연 프로젝트가 없습니다.");
            return;
        }

        // 순회 중 상태 변경 충돌 방지를 위해 클릭된 경로만 임시 기록
        let mut open_target: Option<PathBuf> = None;
        let mut remove_target: Option<PathBuf> = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            for path in &self.settings.recent_projects {
                let exists = path.is_dir(); // 디스크에 폴더가 실제로 존재하는지 확인
                ui.horizontal(|ui| {
                    if exists {
                        if ui.button("열기").clicked() {
                            open_target = Some(path.clone());
                        }
                        ui.label(path.display().to_string());
                    } else {
                        // 사라진 경로는 비활성화·흐리게 표시
                        ui.add_enabled(false, egui::Button::new("없음"));
                        ui.colored_label(
                            egui::Color32::from_gray(120),
                            format!("{} (경로 없음)", path.display()),
                        );
                    }
                    if ui.button("목록에서 제거").clicked() {
                        remove_target = Some(path.clone());
                    }
                });
            }
        });

        // 루프 종료 후 실제 제거·열기 반영
        if let Some(path) = remove_target {
            self.settings.remove_recent(&path);
            let _ = self.settings.save();
        }
        if let Some(path) = open_target {
            self.open_project_path(&path);
        }
    }

    // 시작 화면 내 [새 프로젝트] 입력 폼 렌더링
    fn show_new_project_form(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.heading("새 프로젝트 만들기");
            ui.add_space(4.0);

            // 프로젝트 이름 입력 + 실시간 유효성 검사
            ui.horizontal(|ui| {
                ui.label("프로젝트 이름:");
                ui.text_edit_singleline(&mut self.start_screen.new_draft.name);
            });
            let name_err = self.start_screen.new_draft.name_validation_error();
            if let Some(err) = &name_err {
                ui.colored_label(egui::Color32::from_rgb(240, 100, 100), err);
            }

            // 생성할 상위 폴더 경로 입력 + 최종 경로 미리보기
            ui.horizontal(|ui| {
                ui.label("생성 위치(상위 폴더):");
                ui.add(
                    egui::TextEdit::singleline(&mut self.start_screen.new_draft.parent_dir)
                        .desired_width(360.0),
                );
            });
            ui.label(format!(
                "생성될 경로: {}",
                self.start_screen.new_draft.target_path().display()
            ));

            // 엔진 소스 선택 (Git / 로컬 경로)
            ui.horizontal(|ui| {
                ui.label("엔진 소스:");
                ui.selectable_value(
                    &mut self.start_screen.new_draft.engine_kind,
                    EngineSourceKind::Git,
                    "Git 저장소",
                );
                ui.selectable_value(
                    &mut self.start_screen.new_draft.engine_kind,
                    EngineSourceKind::LocalPath,
                    "로컬 경로",
                );
            });

            // 선택한 엔진 소스에 맞는 세부 입력칸 표시
            match self.start_screen.new_draft.engine_kind {
                EngineSourceKind::Git => {
                    ui.horizontal(|ui| {
                        ui.label("Git URL:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.start_screen.new_draft.git_url)
                                .desired_width(360.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Rev (선택):");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.start_screen.new_draft.git_rev)
                                .hint_text("비우면 기본 브랜치")
                                .desired_width(200.0),
                        );
                    });
                }
                EngineSourceKind::LocalPath => {
                    ui.horizontal(|ui| {
                        ui.label("엔진 루트 경로:");
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut self.start_screen.new_draft.local_engine_path,
                            )
                            .hint_text("예: C:\\DotBox\\dotbox")
                            .desired_width(360.0),
                        );
                    });
                }
            }

            // 펌웨어 포함 및 git init 체크박스
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut self.start_screen.new_draft.firmware,
                    "펌웨어(firmware/) 포함",
                );
                ui.checkbox(&mut self.start_screen.new_draft.git_init, "git init 실행");
            });

            // 만들기 전 경로 문제(한글·공백·OneDrive·길이) 사전 경고 표시
            for warning in &self.start_screen.new_draft.path_warnings() {
                ui.colored_label(
                    egui::Color32::from_rgb(240, 200, 80),
                    format!("경고: {warning}"),
                );
            }

            ui.add_space(4.0);
            // 이름 오류가 없을 때만 [프로젝트 생성 후 열기] 버튼 활성화
            if ui
                .add_enabled(
                    name_err.is_none(),
                    egui::Button::new("프로젝트 생성 후 열기"),
                )
                .clicked()
            {
                match self.start_screen.new_draft.create_project() {
                    Ok((root, warnings)) => {
                        self.start_screen.show_new_form = false; // 폼 닫기
                        self.start_screen.notice_message = (!warnings.is_empty())
                            .then(|| format!("생성 경고:\n{}", warnings.join("\n")));
                        self.open_project_path(&root); // 만든 프로젝트 바로 열기
                    }
                    Err(err) => {
                        self.start_screen.error_message = Some(err); // 실패 이유 표시
                    }
                }
            }
        });
    }

    // 상단 작업 공간 도구 모음 (프로젝트 닫기 · 실행 · 전역 밝기 · 상태 요약)
    fn show_top_bar(&mut self, ui: &mut egui::Ui) -> bool {
        let mut want_close = false;
        let is_cli_busy = self.device_tab.running_cmd.is_some(); // CLI 실행 중 여부

        // 상단 상태 표시용 프로젝트 이름·포트 문자열 추출
        let (project_name, left_port, right_port) = match &self.opened {
            Some(opened) => (
                opened.project.name().to_string(),
                opened
                    .local_config
                    .ports
                    .left
                    .clone()
                    .unwrap_or_else(|| "미지정".to_string()),
                opened
                    .local_config
                    .ports
                    .right
                    .clone()
                    .unwrap_or_else(|| "미지정".to_string()),
            ),
            None => return false,
        };

        ui.horizontal(|ui| {
            ui.strong(format!("DotBox Studio — {project_name}"));
            if ui.button("프로젝트 닫기").clicked() {
                want_close = true; // 닫기 요청 기록
            }

            ui.separator();

            // 시뮬레이터 실행 / 중지 버튼 (실행 중이면 [■ 중지] 표시)
            if is_cli_busy {
                if ui.button("■ 중지").clicked() {
                    self.stop_cli_command(); // 실행 중인 자식 프로세스 종료
                }
            } else if ui.button("▶ 실행").clicked() {
                self.show_bottom_panel = true;
                self.bottom_tab = BottomTab::Output;
                self.launch_cli_command(build_run_args());
            }

            // 환경 점검(doctor) 즉시 재실행 버튼
            let doctor_busy = self.doctor_rx.is_some();
            if ui
                .add_enabled(!doctor_busy, egui::Button::new("환경 점검"))
                .clicked()
            {
                self.start_doctor_check();
            }

            ui.separator();

            // 전역 실기기 밝기 미리보기 설정 (self.settings가 단일 진실 공급원)
            if ui
                .checkbox(&mut self.settings.led_preview, "실기기 밝기")
                .changed()
            {
                self.editor.set_led_preview(self.settings.led_preview);
                let _ = self.settings.save();
            }
            if self.settings.led_preview
                && ui
                    .add(
                        egui::Slider::new(&mut self.settings.brightness_percent, 15..=25)
                            .suffix("%"),
                    )
                    .changed()
            {
                self.editor
                    .set_brightness_percent(self.settings.brightness_percent);
                let _ = self.settings.save();
            }

            ui.separator();

            // 하단 패널 표시 토글 버튼
            let panel_label = if self.show_bottom_panel {
                "하단 패널 숨기기"
            } else {
                "하단 패널 보기"
            };
            if ui.button(panel_label).clicked() {
                self.show_bottom_panel = !self.show_bottom_panel;
            }

            ui.separator();

            // 오른쪽 상태 요약: 왼쪽·오른쪽 포트 현황 표시
            ui.label(format!("왼쪽 {left_port} · 오른쪽 {right_port}"));
        });

        want_close
    }

    // 좌측 프로젝트 탐색기 패널 렌더링 (클릭 시 해당 탭 열기/전환)
    fn show_explorer_panel(&mut self, ui: &mut egui::Ui) {
        let active = self.tabs.active(); // 현재 활성 탭

        ui.heading("탐색기");
        ui.separator();

        // 프로젝트 관리 항목
        if ui
            .selectable_label(active == TabKind::Overview, "개요")
            .clicked()
        {
            self.tabs.open(TabKind::Overview);
        }
        if ui
            .selectable_label(active == TabKind::ProjectSettings, "프로젝트 설정")
            .clicked()
        {
            self.tabs.open(TabKind::ProjectSettings);
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.strong("에셋");
            if ui
                .small_button("+")
                .on_hover_text("새 에셋 만들기")
                .clicked()
            {
                self.asset_modal = Some(AssetModal::Create {
                    kind: AssetKind::Sprite,
                    name: String::new(),
                    error: String::new(),
                });
            }
            if ui
                .small_button("📂")
                .on_hover_text("OS 탐색기에서 assets/ 폴더 열기")
                .clicked()
                && let Some(assets_dir) = self.opened.as_ref().map(|o| o.project.assets_dir())
            {
                match open_in_file_explorer(&assets_dir) {
                    Ok(()) => self.explorer_status.clear(),
                    Err(err) => self.explorer_status = err,
                }
            }
            if ui
                .small_button("↻")
                .on_hover_text("에셋 목록 새로고침")
                .clicked()
            {
                self.refresh_asset_lists();
            }
        });
        ui.colored_label(
            egui::Color32::from_gray(125),
            "항목 우클릭: 이름변경·복제·삭제",
        );

        // 클릭 및 우클릭 메뉴 동작을 루프 종료 후 반영하기 위한 임시 변수
        let mut open_target: Option<(AssetKind, String)> = None;
        let mut next_modal: Option<AssetModal> = None;
        let mut open_icon = false;

        // 1) sprites/ 목록
        egui::CollapsingHeader::new("sprites/")
            .default_open(true)
            .show(ui, |ui| {
                if self.sprite_files.is_empty() {
                    ui.colored_label(egui::Color32::from_gray(130), "  (비어 있음)");
                } else {
                    for name in &self.sprite_files {
                        let selected = active == TabKind::SpriteEditor
                            && !self.editing_icon
                            && self.sprite_asset_name == *name;
                        let resp = ui.selectable_label(selected, format!("{name}.png"));
                        if resp.clicked() {
                            open_target = Some((AssetKind::Sprite, name.clone()));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("이름 바꾸기").clicked() {
                                next_modal = Some(AssetModal::Rename {
                                    kind: AssetKind::Sprite,
                                    old_name: name.clone(),
                                    new_name: name.clone(),
                                    error: String::new(),
                                });
                                ui.close();
                            }
                            if ui.button("복제").clicked() {
                                next_modal = Some(AssetModal::Duplicate {
                                    kind: AssetKind::Sprite,
                                    src_name: name.clone(),
                                    dst_name: format!("{name}_copy"),
                                    error: String::new(),
                                });
                                ui.close();
                            }
                            if ui.button("삭제").clicked() {
                                next_modal = Some(AssetModal::Delete {
                                    kind: AssetKind::Sprite,
                                    name: name.clone(),
                                    error: String::new(),
                                });
                                ui.close();
                            }
                        });
                    }
                }
            });

        // 2) sounds/ 목록
        egui::CollapsingHeader::new("sounds/")
            .default_open(true)
            .show(ui, |ui| {
                if self.sound_files.is_empty() {
                    ui.colored_label(egui::Color32::from_gray(130), "  (비어 있음)");
                } else {
                    for name in &self.sound_files {
                        let selected =
                            active == TabKind::SoundTool && self.sound_tab.asset_name == *name;
                        let resp = ui.selectable_label(selected, format!("{name}.sound.toml"));
                        if resp.clicked() {
                            open_target = Some((AssetKind::Sound, name.clone()));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("이름 바꾸기").clicked() {
                                next_modal = Some(AssetModal::Rename {
                                    kind: AssetKind::Sound,
                                    old_name: name.clone(),
                                    new_name: name.clone(),
                                    error: String::new(),
                                });
                                ui.close();
                            }
                            if ui.button("복제").clicked() {
                                next_modal = Some(AssetModal::Duplicate {
                                    kind: AssetKind::Sound,
                                    src_name: name.clone(),
                                    dst_name: format!("{name}_copy"),
                                    error: String::new(),
                                });
                                ui.close();
                            }
                            if ui.button("삭제").clicked() {
                                next_modal = Some(AssetModal::Delete {
                                    kind: AssetKind::Sound,
                                    name: name.clone(),
                                    error: String::new(),
                                });
                                ui.close();
                            }
                        });
                    }
                }
            });

        // 3) fonts/ 목록
        egui::CollapsingHeader::new("fonts/")
            .default_open(true)
            .show(ui, |ui| {
                if self.font_files.is_empty() {
                    ui.colored_label(egui::Color32::from_gray(130), "  (비어 있음)");
                } else {
                    for name in &self.font_files {
                        let selected =
                            active == TabKind::FontTool && self.font_tab.asset_name == *name;
                        let resp = ui.selectable_label(selected, format!("{name}.font.toml"));
                        if resp.clicked() {
                            open_target = Some((AssetKind::Font, name.clone()));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("이름 바꾸기").clicked() {
                                next_modal = Some(AssetModal::Rename {
                                    kind: AssetKind::Font,
                                    old_name: name.clone(),
                                    new_name: name.clone(),
                                    error: String::new(),
                                });
                                ui.close();
                            }
                            if ui.button("복제").clicked() {
                                next_modal = Some(AssetModal::Duplicate {
                                    kind: AssetKind::Font,
                                    src_name: name.clone(),
                                    dst_name: format!("{name}_copy"),
                                    error: String::new(),
                                });
                                ui.close();
                            }
                            if ui.button("삭제").clicked() {
                                next_modal = Some(AssetModal::Delete {
                                    kind: AssetKind::Font,
                                    name: name.clone(),
                                    error: String::new(),
                                });
                                ui.close();
                            }
                        });
                    }
                }
            });

        // 4) icon.png (클릭 시 16×16 아이콘 편집 모드로 바로 열기)
        let icon_label = if self.has_icon {
            "icon.png"
        } else {
            "icon.png (새로 만들기)"
        };
        let icon_selected = active == TabKind::SpriteEditor && self.editing_icon;
        if ui.selectable_label(icon_selected, icon_label).clicked() {
            open_icon = true;
        }

        // 5) 기타 (알 수 없는 형식의 파일은 표시만 하고 무시)
        if !self.other_files.is_empty() {
            egui::CollapsingHeader::new(format!("기타 ({})", self.other_files.len()))
                .default_open(false)
                .show(ui, |ui| {
                    for item in &self.other_files {
                        ui.colored_label(egui::Color32::from_gray(130), format!("  {item}"));
                    }
                });
        }

        if !self.explorer_status.is_empty() {
            ui.add_space(4.0);
            ui.colored_label(egui::Color32::from_rgb(240, 200, 80), &self.explorer_status);
        }

        // 클릭된 에셋 파일 즉시 불러오기 및 해당 편집기 탭 활성화
        if let Some((kind, name)) = open_target {
            self.open_asset_by_kind(kind, name);
        }
        if let Some(modal) = next_modal {
            self.asset_modal = Some(modal);
        }
        if open_icon {
            self.open_icon_png();
        }

        ui.add_space(8.0);
        ui.separator();
        ui.label("도구");
        // 기존 1~4순위 도구 탭 열기/전환
        for kind in [
            TabKind::SpriteEditor,
            TabKind::FontTool,
            TabKind::SoundTool,
            TabKind::ScenePreview,
            TabKind::DevicePanel,
        ] {
            let label = TabManager::tab_label(kind, self.is_tab_dirty(kind));
            if ui.selectable_label(active == kind, label).clicked() {
                self.tabs.open(kind);
            }
        }
    }

    // 하단 패널 렌더링 ([출력] · [문제 N])
    fn show_bottom_panel(&mut self, ui: &mut egui::Ui) {
        // doctor 점검 및 cargo 컴파일 결과에서 경고·오류 항목 개수 계산
        let doctor_problem_count = self
            .doctor_report
            .as_ref()
            .map(|r| r.count(Level::Error) + r.count(Level::Warn))
            .unwrap_or(0);
        let cargo_problem_count = self
            .build_diagnostics
            .iter()
            .filter(|d| matches!(d.level, DiagnosticLevel::Error | DiagnosticLevel::Warning))
            .count();
        let problem_count = doctor_problem_count + cargo_problem_count;

        // 하단 탭 헤더 바
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.bottom_tab, BottomTab::Output, "출력");
            ui.selectable_value(
                &mut self.bottom_tab,
                BottomTab::Problems,
                format!("문제 ({problem_count})"),
            );
            ui.selectable_value(
                &mut self.bottom_tab,
                BottomTab::Serial,
                "시리얼: 왼쪽 | 오른쪽",
            );
        });
        ui.separator();

        // 시리얼 탭은 자체 상단 툴바와 좌/우 분할 스크롤 영역을 사용
        if self.bottom_tab == BottomTab::Serial {
            self.show_serial_tab(ui);
            return;
        }

        // 선택된 하단 탭 내용 표시
        egui::ScrollArea::vertical()
            .max_height(130.0)
            .show(ui, |ui| match self.bottom_tab {
                BottomTab::Serial => {}
                BottomTab::Output => {
                    ui.monospace(&self.device_tab.output_log); // CLI 실행 로그 출력
                }
                BottomTab::Problems => {
                    let mut total_problems = 0;
                    let mut jump_to_overview = false;

                    // 1) cargo 빌드/실행 컴파일 에러 및 경고 (종류 · 위치 · 메시지)
                    if !self.build_diagnostics.is_empty() {
                        ui.strong("빌드 / 컴파일 진단");
                        for diag in &self.build_diagnostics {
                            total_problems += 1;
                            let color = match diag.level {
                                DiagnosticLevel::Error => egui::Color32::from_rgb(240, 100, 100),
                                DiagnosticLevel::Warning => egui::Color32::from_rgb(240, 200, 80),
                                DiagnosticLevel::Note => egui::Color32::from_rgb(160, 200, 240),
                                DiagnosticLevel::Help => egui::Color32::from_rgb(120, 220, 140),
                            };

                            let loc = match (&diag.file, diag.line, diag.column) {
                                (Some(f), Some(l), Some(c)) => format!("{f}:{l}:{c}"),
                                (Some(f), Some(l), None) => format!("{f}:{l}"),
                                (Some(f), None, None) => f.clone(),
                                _ => "위치 미상".to_string(),
                            };

                            let line_text =
                                format!("[{}] {} — {}", diag.level.as_str(), loc, diag.message);
                            ui.label(egui::RichText::new(line_text).color(color));
                        }
                        ui.separator();
                    }

                    // 2) doctor 환경 점검 오류 및 경고
                    if let Some(report) = &self.doctor_report {
                        ui.strong("환경 점검 (doctor)");
                        let mut doc_count = 0;
                        for check in &report.checks {
                            if check.level == Level::Error || check.level == Level::Warn {
                                total_problems += 1;
                                doc_count += 1;
                                let color = level_color(check.level);
                                let line = format!(
                                    "[{}] {} — {}",
                                    check.level.as_str().to_uppercase(),
                                    check.id,
                                    check.message
                                );
                                if ui
                                    .add(
                                        egui::Label::new(egui::RichText::new(line).color(color))
                                            .sense(egui::Sense::click()),
                                    )
                                    .on_hover_text("클릭 시 개요 탭으로 이동")
                                    .clicked()
                                {
                                    jump_to_overview = true;
                                }
                            }
                        }
                        if doc_count == 0 {
                            ui.colored_label(
                                egui::Color32::from_gray(140),
                                "  (환경 점검 문제 없음)",
                            );
                        }
                    }

                    if total_problems == 0 {
                        ui.label("발견된 문제가 없습니다.");
                    }
                    if jump_to_overview {
                        self.tabs.open(TabKind::Overview);
                    }
                }
            });
    }

    // 하단 시리얼 로그 뷰어 탭 렌더링 (왼쪽·오른쪽 나란히/단독 보기 · 검색 · 일시정지 · 저장)
    fn show_serial_tab(&mut self, ui: &mut egui::Ui) {
        let project_root = self.opened.as_ref().map(|o| o.project.root().to_path_buf());

        // 상단 공통 바: 보기 방식 전환 · 검색어 입력 · 저장 결과 안내
        ui.horizontal(|ui| {
            ui.label("보기:");
            ui.selectable_value(&mut self.serial_view_mode, SerialViewMode::Split, "나란히");
            ui.selectable_value(&mut self.serial_view_mode, SerialViewMode::Left, "왼쪽만");
            ui.selectable_value(
                &mut self.serial_view_mode,
                SerialViewMode::Right,
                "오른쪽만",
            );
            ui.separator();
            ui.label("검색:");
            ui.add(
                egui::TextEdit::singleline(&mut self.serial_search)
                    .hint_text("필터 키워드...")
                    .desired_width(150.0),
            );
            if !self.serial_search.is_empty() && ui.small_button("×").clicked() {
                self.serial_search.clear();
            }
            if !self.serial_save_status.is_empty() {
                ui.separator();
                ui.label(&self.serial_save_status);
            }
        });
        ui.add_space(4.0);

        let search = self.serial_search.clone();
        let save_status = &mut self.serial_save_status;

        match self.serial_view_mode {
            SerialViewMode::Split => {
                ui.columns(2, |cols| {
                    show_serial_pane(
                        &mut cols[0],
                        "왼쪽 모듈",
                        "serial_left",
                        &mut self.serial_left,
                        &search,
                        project_root.as_deref(),
                        save_status,
                    );
                    show_serial_pane(
                        &mut cols[1],
                        "오른쪽 모듈",
                        "serial_right",
                        &mut self.serial_right,
                        &search,
                        project_root.as_deref(),
                        save_status,
                    );
                });
            }
            SerialViewMode::Left => {
                show_serial_pane(
                    ui,
                    "왼쪽 모듈",
                    "serial_left",
                    &mut self.serial_left,
                    &search,
                    project_root.as_deref(),
                    save_status,
                );
            }
            SerialViewMode::Right => {
                show_serial_pane(
                    ui,
                    "오른쪽 모듈",
                    "serial_right",
                    &mut self.serial_right,
                    &search,
                    project_root.as_deref(),
                    save_status,
                );
            }
        }
    }

    // 개요 탭 화면 (명세서 4.2)
    fn show_overview_tab(&mut self, ui: &mut egui::Ui) {
        let Some(opened) = self.opened.clone() else {
            return;
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("프로젝트 개요");
            ui.separator();

            // 1) dotbox.toml 기본 정보 및 [paths]
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.strong("프로젝트 정보 (dotbox.toml)");
                ui.label(format!("이름: {}", opened.project.name()));
                ui.label(format!("경로: {}", opened.project.root().display()));
                let paths = &opened.project.manifest().paths;
                ui.label(format!(
                    "[paths] game: {} | sim: {} | firmware: {} | assets: {}",
                    paths.game.display(),
                    paths.sim.display(),
                    paths.firmware.display(),
                    paths.assets.display()
                ));
                ui.label(format!(
                    "펌웨어 포함: {}",
                    if opened.project.has_firmware() {
                        "예"
                    } else {
                        "아니오"
                    }
                ));
            });
            ui.add_space(6.0);

            // 2) 엔진 버전 일치 여부 및 doctor::run 환경 점검 요약
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("환경 점검 (dotbox doctor)");
                    ui.checkbox(&mut self.doctor_host_only, "시뮬레이터 전용(host-only)");
                    let busy = self.doctor_rx.is_some();
                    if ui
                        .add_enabled(!busy, egui::Button::new("다시 점검"))
                        .clicked()
                    {
                        self.start_doctor_check();
                    }
                    if busy {
                        ui.label("점검 중...");
                    }
                });

                if let Some(report) = &self.doctor_report {
                    // engine.version 항목 별도 강조 표시
                    if let Some(engine_check) =
                        report.checks.iter().find(|c| c.id == "engine.version")
                    {
                        ui.colored_label(
                            level_color(engine_check.level),
                            format!("엔진/CLI 버전: {}", engine_check.message),
                        );
                    }

                    // 수준별 개수 요약 표시
                    let errors = report.count(Level::Error);
                    let warns = report.count(Level::Warn);
                    let oks = report.count(Level::Ok);
                    let infos = report.count(Level::Info);
                    ui.label(format!(
                        "요약: 오류 {errors} · 경고 {warns} · 정상 {oks} · 정보 {infos}"
                    ));

                    // 펼치면 개별 점검 항목과 힌트를 모두 확인 가능
                    egui::CollapsingHeader::new("항목별 점검 결과 보기")
                        .default_open(errors > 0 || warns > 0)
                        .show(ui, |ui| {
                            for check in &report.checks {
                                ui.colored_label(
                                    level_color(check.level),
                                    format!(
                                        "[{}] {}: {}",
                                        check.level.as_str().to_uppercase(),
                                        check.id,
                                        check.message
                                    ),
                                );
                                if let Some(hint) = &check.hint {
                                    ui.label(format!("    힌트: {hint}"));
                                }
                            }
                        });
                } else {
                    ui.label("점검 결과를 불러오는 중입니다...");
                }
            });
            ui.add_space(6.0);

            // 3) 포트 설정 및 모듈별 마지막 플래시 build_id (.dotbox/local.toml)
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.strong("기기 및 플래시 상태 (.dotbox/local.toml)");
                let ports = &opened.local_config.ports;
                let flashed = &opened.local_config.flashed;

                ui.label(format!(
                    "왼쪽 모듈 — 포트: {} | 마지막 build_id: {}",
                    ports.left.as_deref().unwrap_or("미지정"),
                    flashed.left.as_deref().unwrap_or("기록 없음")
                ));
                ui.label(format!(
                    "오른쪽 모듈 — 포트: {} | 마지막 build_id: {}",
                    ports.right.as_deref().unwrap_or("미지정"),
                    flashed.right.as_deref().unwrap_or("기록 없음")
                ));

                // 좌/우 모듈에 구워진 이미지가 서로 다르면 경고 표시
                if opened.local_config.flashed_mismatch() {
                    ui.colored_label(
                        egui::Color32::from_rgb(240, 200, 80),
                        "경고: 왼쪽과 오른쪽 모듈의 마지막 플래시 build_id가 다릅니다.",
                    );
                }

                if ui.button("포트 설정으로 이동").clicked() {
                    self.tabs.open(TabKind::ProjectSettings); // 설정 탭 열기
                }
            });
            ui.add_space(6.0);

            // 4) 에셋 상태 (5장 합의 전이므로 안내 표시)
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.strong("에셋 상태");
                ui.label(format!(
                    "에셋 폴더: {}",
                    opened.project.assets_dir().display()
                ));
                ui.colored_label(
                    egui::Color32::from_gray(150),
                    "에셋 수와 검사 결과(assets::check)는 S2 에셋 형식 합의 후 표시됩니다.",
                );
            });
        });
    }

    // 프로젝트 설정 탭 화면 (명세서 4.3)
    fn show_project_settings_tab(&mut self, ui: &mut egui::Ui) {
        let Some(project_name) = self.opened.as_ref().map(|o| o.project.name().to_string()) else {
            return;
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("프로젝트 설정");
            ui.separator();

            // 저장 성공 또는 실패 메시지 표시
            if !self.settings_form.status_message.is_empty() {
                ui.colored_label(
                    egui::Color32::from_rgb(120, 220, 140),
                    &self.settings_form.status_message,
                );
            }
            if !self.settings_form.error_message.is_empty() {
                ui.colored_label(
                    egui::Color32::from_rgb(240, 100, 100),
                    &self.settings_form.error_message,
                );
            }

            // 1) dotbox.toml 편집 섹션 (1차에서는 이름 변경 막고 안내)
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.strong("프로젝트 매니페스트 (dotbox.toml)");
                ui.horizontal(|ui| {
                    ui.label("프로젝트 이름:");
                    ui.monospace(&project_name);
                });
                ui.colored_label(
                    egui::Color32::from_gray(150),
                    "안내: 프로젝트 이름을 바꾸면 패키지 이름도 바뀌므로 1차 버전에서는 이름 변경을 지원하지 않습니다.",
                );
                ui.add_space(4.0);

                // [paths] 4개 상대 경로 입력칸
                ui.horizontal(|ui| {
                    ui.label("game 경로:");
                    ui.text_edit_singleline(&mut self.settings_form.game_path);
                });
                ui.horizontal(|ui| {
                    ui.label("sim 경로:");
                    ui.text_edit_singleline(&mut self.settings_form.sim_path);
                });
                ui.horizontal(|ui| {
                    ui.label("firmware 경로:");
                    ui.text_edit_singleline(&mut self.settings_form.firmware_path);
                });
                ui.horizontal(|ui| {
                    ui.label("assets 경로:");
                    ui.text_edit_singleline(&mut self.settings_form.assets_path);
                });

                // 버튼 클릭과 열린 프로젝트가 있을 때만 dotbox.toml 경로 저장 실행
                if ui.button("dotbox.toml 경로 저장").clicked()
                    && let Some(opened) = &mut self.opened
                {
                    match opened.save_paths(
                        &self.settings_form.game_path,
                        &self.settings_form.sim_path,
                        &self.settings_form.firmware_path,
                        &self.settings_form.assets_path,
                    ) {
                        Ok(()) => {
                            self.settings_form.status_message =
                                "dotbox.toml 경로 설정을 저장했습니다.".to_string();
                            self.settings_form.error_message.clear();
                        }
                        Err(err) => {
                            self.settings_form.error_message = err;
                            self.settings_form.status_message.clear();
                        }
                    }
                }
            });
            ui.add_space(8.0);

            // 2) 개인 설정 (.dotbox/local.toml) 왼쪽/오른쪽 포트 설정 섹션
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.strong("시리얼 포트 설정 (.dotbox/local.toml)");
                ui.horizontal(|ui| {
                    if ui.button("감지된 포트 새로고침").clicked() {
                        self.refresh_serial_ports();
                    }
                    ui.label(&self.device_tab.port_status);
                });

                // 감지된 포트 목록에서 클릭 한 번으로 왼쪽/오른쪽 포트 지정
                for port in &self.device_tab.ports {
                    ui.horizontal(|ui| {
                        ui.monospace(&port.name);
                        if ui.button("왼쪽으로 지정").clicked() {
                            self.settings_form.left_port = port.name.clone();
                        }
                        if ui.button("오른쪽으로 지정").clicked() {
                            self.settings_form.right_port = port.name.clone();
                        }
                        ui.label(&port.description);
                    });
                }
                ui.add_space(4.0);

                // 직접 포트 문자열 입력칸
                ui.horizontal(|ui| {
                    ui.label("왼쪽 모듈 포트:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings_form.left_port)
                            .hint_text("예: COM5 (비우면 미지정)")
                            .desired_width(180.0),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("오른쪽 모듈 포트:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings_form.right_port)
                            .hint_text("예: COM6 (비우면 미지정)")
                            .desired_width(180.0),
                    );
                });

                // 버튼 클릭과 열린 프로젝트가 있을 때만 좌/우 포트 설정 저장 실행
                if ui.button("포트 설정 저장").clicked()
                    && let Some(opened) = &mut self.opened
                {
                    match opened.save_ports(
                        &self.settings_form.left_port,
                        &self.settings_form.right_port,
                    ) {
                        Ok(()) => {
                            // 기기 패널 및 시리얼 모니터 포트에도 즉시 반영
                            self.device_tab.selected_port =
                                opened.local_config.ports.left.clone().unwrap_or_default();
                            if !self.serial_left.is_connected() {
                                self.serial_left.port_name =
                                    opened.local_config.ports.left.clone().unwrap_or_default();
                            }
                            if !self.serial_right.is_connected() {
                                self.serial_right.port_name =
                                    opened.local_config.ports.right.clone().unwrap_or_default();
                            }
                            self.settings_form.status_message =
                                ".dotbox/local.toml 포트 설정을 저장했습니다.".to_string();
                            self.settings_form.error_message.clear();
                        }
                        Err(err) => {
                            self.settings_form.error_message = err;
                            self.settings_form.status_message.clear();
                        }
                    }
                }
            });
        });
    }

    // 스프라이트 편집기 도구 모음 (화면 모드 · 전체 지우기 · Undo/Redo · 크기 · 6종 도구 · 파일 저장)
    fn show_sprite_toolbar(&mut self, ui: &mut egui::Ui) {
        // 1줄: 화면 모드 전환 · 전체 지우기 · 실행 취소/다시 실행
        ui.horizontal(|ui| {
            ui.label("화면 모드:");
            let is_wide = matches!(self.editor.layout(), ScreenLayout::Wide)
                && self.editor.width() == 32
                && self.editor.height() == 16;
            let is_mirrored = matches!(self.editor.layout(), ScreenLayout::Mirrored)
                && self.editor.width() == 16
                && self.editor.height() == 16;

            if ui.selectable_label(is_wide, "Wide (32×16)").clicked() && !is_wide {
                self.editing_icon = false;
                self.editor.set_layout(ScreenLayout::Wide);
                self.canvas_history.reset();
                self.canvas_drag_before = None;
                self.shape_start = None;
            }
            if ui
                .selectable_label(is_mirrored, "Mirrored (16×16)")
                .clicked()
                && !is_mirrored
            {
                self.editor.set_layout(ScreenLayout::Mirrored);
                self.canvas_history.reset();
                self.canvas_drag_before = None;
                self.shape_start = None;
            }

            ui.separator();

            if ui.button("전체 지우기").clicked() {
                let before = self.editor.snapshot_state();
                self.editor.clear();
                let after = self.editor.snapshot_state();
                if before != after {
                    let cmd = Box::new(ReplaceCanvasCommand::new(before, after));
                    self.canvas_history.execute(&mut self.editor, cmd);
                }
            }

            ui.separator();

            if ui
                .add_enabled(
                    self.canvas_history.can_undo(),
                    egui::Button::new("↶ 실행 취소 (Ctrl+Z)"),
                )
                .clicked()
            {
                self.canvas_history.undo(&mut self.editor);
            }
            if ui
                .add_enabled(
                    self.canvas_history.can_redo(),
                    egui::Button::new("↷ 다시 실행 (Ctrl+Y)"),
                )
                .clicked()
            {
                self.canvas_history.redo(&mut self.editor);
            }
        });

        // 2줄: 캔버스 크기 지정 (1×1 ~ 32×16) 및 6종 그리기 도구
        ui.horizontal(|ui| {
            ui.label("크기:");
            let mut w = self.editor.width();
            let mut h = self.editor.height();
            let w_changed = ui
                .add_enabled(
                    !self.editing_icon,
                    egui::DragValue::new(&mut w).range(1..=32).prefix("W:"),
                )
                .changed();
            let h_changed = ui
                .add_enabled(
                    !self.editing_icon,
                    egui::DragValue::new(&mut h).range(1..=16).prefix("H:"),
                )
                .changed();
            if w_changed || h_changed {
                self.editor.resize(w, h);
                self.canvas_history.reset();
                self.canvas_drag_before = None;
                self.shape_start = None;
            }

            ui.separator();
            ui.label("도구:");
            for tool in [
                DrawTool::Pencil,
                DrawTool::Eraser,
                DrawTool::Fill,
                DrawTool::Eyedropper,
                DrawTool::Line,
                DrawTool::Rect,
            ] {
                ui.selectable_value(&mut self.draw_tool, tool, tool.label());
            }
        });

        // 3줄: 스프라이트 / 아이콘 파일 저장·불러오기 바
        ui.horizontal(|ui| {
            if self.editing_icon {
                ui.strong("아이콘 편집 모드 (assets/icon.png · 16×16)");
                if ui.button("💾 icon.png 저장 (Ctrl+S)").clicked() {
                    self.save_current_sprite();
                }
                if ui.button("일반 스프라이트로 전환").clicked() {
                    self.editing_icon = false;
                }
            } else {
                ui.label("파일명 (sprites/*.png):");
                ui.add(
                    egui::TextEdit::singleline(&mut self.sprite_asset_name)
                        .hint_text("예: player_run")
                        .desired_width(140.0),
                );
                if ui.button("💾 저장 (Ctrl+S)").clicked() {
                    self.save_current_sprite();
                }
                if ui.button("📂 불러오기").clicked() {
                    self.load_current_sprite();
                }
            }
            if !self.sprite_io_status.is_empty() {
                ui.label(&self.sprite_io_status);
            }
        });
    }

    // 색상 팔레트 및 커스텀 색상 선택기 표시
    fn show_palette(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("팔레트:");

            // 기본 프리셋 8색 버튼 순회하며 그리기
            for color in PALETTE {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(3), to_color32(color));

                // 선택된 색이면 흰색 굵은 테두리, 아니면 회색 테두리
                let is_selected = self.editor.selected_color() == color;
                let stroke = if is_selected {
                    egui::Stroke::new(2.5, egui::Color32::WHITE)
                } else {
                    egui::Stroke::new(1.0, egui::Color32::DARK_GRAY)
                };
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::same(3),
                    stroke,
                    egui::StrokeKind::Inside,
                );

                if response.clicked() {
                    self.editor.set_selected_color(color);
                }
            }

            ui.separator();

            // 사용자가 RGB 값을 직접 고르는 컬러 피커
            ui.label("직접 선택:");
            let current = self.editor.selected_color();
            let mut rgb = [current.r, current.g, current.b];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                self.editor
                    .set_selected_color(Rgb::new(rgb[0], rgb[1], rgb[2]));
            }
        });
    }

    // 1×1 ~ 32×16 도트 캔버스 렌더링 및 마우스 입력 처리
    fn show_canvas(&mut self, ui: &mut egui::Ui) {
        let width = self.editor.width(); // 가로 도트 수
        let height = self.editor.height(); // 세로 도트 수
        let canvas_size = egui::vec2(width as f32 * CELL_SIZE, height as f32 * CELL_SIZE);

        let (rect, response) = ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());

        // 마우스 입력 처리: 우클릭은 항상 빠른 지우개, 좌클릭은 선택된 도구 실행
        let primary_clicked = response.clicked_by(egui::PointerButton::Primary);
        let primary_active = primary_clicked || response.dragged_by(egui::PointerButton::Primary);
        let secondary_active = response.clicked_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Secondary);

        if (primary_active || secondary_active)
            && let Some(pointer_pos) = response.interact_pointer_pos()
        {
            self.anim_playing = false;

            let rel = pointer_pos - rect.min;
            if rel.x >= 0.0 && rel.y >= 0.0 {
                let x = (rel.x / CELL_SIZE) as usize;
                let y = (rel.y / CELL_SIZE) as usize;
                if x < width && y < height {
                    if secondary_active {
                        if self.canvas_drag_before.is_none() {
                            self.canvas_drag_before = Some(self.editor.snapshot_state());
                        }
                        self.editor.erase_pixel(x, y);
                    } else {
                        match self.draw_tool {
                            DrawTool::Pencil => {
                                if self.canvas_drag_before.is_none() {
                                    self.canvas_drag_before = Some(self.editor.snapshot_state());
                                }
                                self.editor.paint_pixel(x, y);
                            }
                            DrawTool::Eraser => {
                                if self.canvas_drag_before.is_none() {
                                    self.canvas_drag_before = Some(self.editor.snapshot_state());
                                }
                                self.editor.erase_pixel(x, y);
                            }
                            DrawTool::Eyedropper => {
                                self.editor.pick_color(x, y);
                                self.draw_tool = DrawTool::Pencil; // 색 추출 후 연필로 복귀
                            }
                            DrawTool::Fill => {
                                if primary_clicked {
                                    let before = self.editor.snapshot_state();
                                    self.editor.flood_fill(x, y);
                                    let after = self.editor.snapshot_state();
                                    if before != after {
                                        let cmd =
                                            Box::new(ReplaceCanvasCommand::new(before, after));
                                        self.canvas_history.execute(&mut self.editor, cmd);
                                    }
                                }
                            }
                            DrawTool::Line | DrawTool::Rect => {
                                if self.shape_start.is_none() {
                                    self.shape_start = Some((x, y));
                                    self.canvas_drag_before = Some(self.editor.snapshot_state());
                                }
                                // 드래그 중에는 원본 스냅샷 위에 실시간으로 도형 미리보기
                                if let (Some((sx, sy)), Some(before)) =
                                    (self.shape_start, self.canvas_drag_before.as_ref())
                                {
                                    self.editor.restore_state(before);
                                    if self.draw_tool == DrawTool::Line {
                                        self.editor.draw_line(sx, sy, x, y);
                                    } else {
                                        self.editor.draw_rect(sx, sy, x, y);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 마우스 클릭/드래그가 끝난 순간, 변경이 있었으면 1개의 명령 객체로 묶어 기록
        let stroke_finished = !response.dragged() && !response.is_pointer_button_down_on();
        if stroke_finished {
            self.shape_start = None;
            if let Some(before) = self.canvas_drag_before.take() {
                let after = self.editor.snapshot_state();
                if before != after {
                    let cmd = Box::new(ReplaceCanvasCommand::new(before, after));
                    self.canvas_history.execute(&mut self.editor, cmd);
                }
            }
        }

        // 모든 도트 칸과 격자선 그리기
        let painter = ui.painter();
        let grid_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(45));

        for y in 0..height {
            for x in 0..width {
                let cell_min = egui::pos2(
                    rect.min.x + x as f32 * CELL_SIZE,
                    rect.min.y + y as f32 * CELL_SIZE,
                );
                let cell_rect =
                    egui::Rect::from_min_size(cell_min, egui::vec2(CELL_SIZE, CELL_SIZE));

                let color = to_color32(self.editor.display_color(x, y));
                painter.rect_filled(cell_rect, egui::CornerRadius::ZERO, color);
                painter.rect_stroke(
                    cell_rect,
                    egui::CornerRadius::ZERO,
                    grid_stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }

        // 가로가 16px을 넘을 때 중앙(x=16) 패널 경계선 표시
        if width > usize::from(PANEL_WIDTH) {
            draw_panel_divider(painter, rect);
        }
    }

    // 스프라이트 애니메이션 프레임 관리(추가·복제·삭제·순서·시간) 및 재생 미리보기 바
    fn show_animation_bar(&mut self, ui: &mut egui::Ui) {
        // 아이콘 편집 모드(16×16 정지 이미지)에서는 애니메이션 바 숨김
        if self.editing_icon {
            return;
        }

        // 재생 중이면 프레임별 duration_ms에 맞춰 다음 프레임으로 자동 전환
        if self.anim_playing && self.editor.frame_count() > 1 {
            let dt_ms = ui.ctx().input(|i| i.stable_dt) * 1000.0;
            self.anim_elapsed_ms += dt_ms;
            let current_dur = f32::from(
                self.editor
                    .frame_duration_ms(self.editor.current_frame_index()),
            );
            if self.anim_elapsed_ms >= current_dur {
                self.anim_elapsed_ms %= current_dur.max(1.0);
                let next = (self.editor.current_frame_index() + 1) % self.editor.frame_count();
                self.editor.select_frame(next);
            }
            ui.ctx().request_repaint(); // 부드러운 재생을 위해 다음 프레임 요청
        } else {
            self.anim_playing = false;
            self.anim_elapsed_ms = 0.0;
        }

        let count = self.editor.frame_count();
        let current = self.editor.current_frame_index();

        // 1줄: 재생·추가·복제·순서 이동·삭제·프레임 시간 조절
        ui.horizontal(|ui| {
            ui.strong(format!("애니메이션 ({}/{count})", current + 1));

            let play_label = if self.anim_playing {
                "■ 정지"
            } else {
                "▶ 재생"
            };
            if ui
                .add_enabled(count > 1, egui::Button::new(play_label))
                .clicked()
            {
                self.anim_playing = !self.anim_playing;
                self.anim_elapsed_ms = 0.0;
            }

            ui.separator();

            if ui.button("+ 빈 프레임").clicked() {
                self.anim_playing = false;
                let before = self.editor.snapshot_state();
                self.editor.add_frame();
                let after = self.editor.snapshot_state();
                self.canvas_history.execute(
                    &mut self.editor,
                    Box::new(ReplaceCanvasCommand::new(before, after)),
                );
            }
            if ui.button("복제").clicked() {
                self.anim_playing = false;
                let before = self.editor.snapshot_state();
                self.editor.duplicate_frame();
                let after = self.editor.snapshot_state();
                self.canvas_history.execute(
                    &mut self.editor,
                    Box::new(ReplaceCanvasCommand::new(before, after)),
                );
            }
            if ui
                .add_enabled(current > 0, egui::Button::new("◀"))
                .on_hover_text("프레임 앞으로 이동")
                .clicked()
            {
                self.anim_playing = false;
                let before = self.editor.snapshot_state();
                if self.editor.move_frame_left(current) {
                    let after = self.editor.snapshot_state();
                    self.canvas_history.execute(
                        &mut self.editor,
                        Box::new(ReplaceCanvasCommand::new(before, after)),
                    );
                }
            }
            if ui
                .add_enabled(current + 1 < count, egui::Button::new("▶"))
                .on_hover_text("프레임 뒤로 이동")
                .clicked()
            {
                self.anim_playing = false;
                let before = self.editor.snapshot_state();
                if self.editor.move_frame_right(current) {
                    let after = self.editor.snapshot_state();
                    self.canvas_history.execute(
                        &mut self.editor,
                        Box::new(ReplaceCanvasCommand::new(before, after)),
                    );
                }
            }
            if ui
                .add_enabled(count > 1, egui::Button::new("삭제"))
                .clicked()
            {
                self.anim_playing = false;
                let before = self.editor.snapshot_state();
                if self.editor.delete_frame(current) {
                    let after = self.editor.snapshot_state();
                    self.canvas_history.execute(
                        &mut self.editor,
                        Box::new(ReplaceCanvasCommand::new(before, after)),
                    );
                }
            }

            ui.separator();

            // 현재 프레임 재생 시간(ms) 편집
            ui.label("시간:");
            let mut dur = self.editor.frame_duration_ms(current);
            let before = self.editor.snapshot_state();
            if ui
                .add(egui::DragValue::new(&mut dur).range(20..=5000).suffix("ms"))
                .changed()
            {
                self.editor.set_frame_duration_ms(current, dur);
                let after = self.editor.snapshot_state();
                if before != after {
                    self.canvas_history.execute(
                        &mut self.editor,
                        Box::new(ReplaceCanvasCommand::new(before, after)),
                    );
                }
            }
        });

        // 2줄: 프레임 선택 버튼 목록 (#1, #2, ...)
        ui.horizontal_wrapped(|ui| {
            for idx in 0..count {
                let dur = self.editor.frame_duration_ms(idx);
                let label = format!("#{} ({dur}ms)", idx + 1);
                if ui.selectable_label(idx == current, label).clicked() {
                    self.anim_playing = false;
                    self.editor.select_frame(idx);
                }
            }
        });
    }

    // 글꼴 변환과 미리보기 탭 화면 (기준선 정렬 + 글자별 결과 표 포함)
    fn show_font_tab(&mut self, ui: &mut egui::Ui) {
        // 1) 글꼴 설정 파일 저장·불러오기 바 (assets/fonts/<이름>.font.toml)
        ui.horizontal(|ui| {
            ui.label("파일명 (fonts/*.font.toml):");
            ui.add(
                egui::TextEdit::singleline(&mut self.font_tab.asset_name)
                    .hint_text("예: main")
                    .desired_width(120.0),
            );
            if ui.button("💾 저장 (Ctrl+S)").clicked() {
                self.save_current_font();
            }
            if ui.button("📂 불러오기").clicked() {
                self.load_current_font();
            }
            if !self.font_tab.io_status.is_empty() {
                ui.label(&self.font_tab.io_status);
            }
        });

        // 2) 원본 TTF/OTF 폰트 경로 입력 및 불러오기
        ui.horizontal(|ui| {
            ui.label("TTF/OTF 경로:");
            ui.add(egui::TextEdit::singleline(&mut self.font_tab.font_path).desired_width(280.0));
            if ui.button("폰트 불러오기").clicked() {
                match FontConverter::from_file(Path::new(&self.font_tab.font_path)) {
                    Ok(converter) => {
                        self.font_tab.converter = Some(converter);
                        self.font_tab.status_message = "폰트 불러오기 완료!".to_string();
                    }
                    Err(err) => {
                        self.font_tab.converter = None;
                        self.font_tab.status_message = err;
                    }
                }
            }
        });
        ui.label(&self.font_tab.status_message);

        // 3) 라이선스 · 픽셀 크기 · 임계값 · 문자 집합 설정
        ui.horizontal(|ui| {
            ui.label("라이선스:");
            ui.add(
                egui::TextEdit::singleline(&mut self.font_tab.license)
                    .hint_text("예: OFL-1.1 / MIT")
                    .desired_width(200.0),
            );
        });
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label("픽셀 크기:");
            ui.add(
                egui::Slider::new(&mut self.font_tab.config.pixel_size, 6.0..=16.0).suffix("px"),
            );
            ui.separator();
            ui.label("이진화 임계값:");
            ui.add(egui::Slider::new(
                &mut self.font_tab.config.threshold,
                1..=255,
            ));
        });

        ui.horizontal(|ui| {
            ui.label("미리보기 문구:");
            ui.text_edit_singleline(&mut self.font_tab.preview_text);
            ui.separator();
            ui.label("문자 집합:");
            ui.text_edit_singleline(&mut self.font_tab.config.charset);
        });
        ui.add_space(8.0);

        if let Some(converter) = &self.font_tab.converter {
            let glyphs = converter.rasterize_charset(&self.font_tab.config);
            ui.horizontal(|ui| {
                ui.label(format!(
                    "변환된 고유 글자 수: {}자 (1비트 비트맵)",
                    glyphs.len()
                ));
                ui.separator();
                ui.checkbox(&mut self.font_tab.align_baseline, "기준선(baseline) 정렬");
            });
            ui.add_space(4.0);

            // 글꼴 미리보기 색상은 흰색 고정 (스프라이트 브러시 색과 상태 분리)
            let fb = converter.render_preview(
                &self.font_tab.preview_text,
                self.font_tab.config.pixel_size,
                self.font_tab.config.threshold,
                Rgb::WHITE,
                self.font_tab.align_baseline,
            );
            // 전역 밝기 설정(self.settings)을 직접 참조해 스프라이트 편집기와 상태 완전 분리
            draw_framebuffer_preview(
                ui,
                &fb,
                self.settings.led_preview,
                self.settings.brightness_percent,
            );
            ui.add_space(8.0);

            // 글자별 변환 결과 표 (문자 · 크기 · 자간 · 기준선 오프셋 · 1비트 미니 미리보기)
            egui::CollapsingHeader::new("글자별 결과 표")
                .default_open(true)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .show(ui, |ui| {
                            egui::Grid::new("font_glyph_table")
                                .striped(true)
                                .spacing([14.0, 4.0])
                                .show(ui, |ui| {
                                    ui.strong("문자");
                                    ui.strong("크기 (W×H)");
                                    ui.strong("간격 (adv)");
                                    ui.strong("기준선 Y");
                                    ui.strong("비트맵");
                                    ui.end_row();

                                    for g in &glyphs {
                                        let ch_label = if g.ch == ' ' {
                                            "(공백)".to_string()
                                        } else {
                                            format!("'{}'", g.ch)
                                        };
                                        ui.monospace(ch_label);
                                        ui.label(format!("{}×{}", g.width, g.height));
                                        ui.label(format!("{}px", g.advance_x));
                                        ui.label(format!("{:+}", g.y_offset));

                                        // 1비트 미니 글리프 미리보기 (도트 1개 = 2×2px)
                                        let dot = 2.0;
                                        let preview_w = (g.width.max(1) as f32) * dot;
                                        let preview_h = (g.height.max(1) as f32) * dot;
                                        let (rect, _) = ui.allocate_exact_size(
                                            egui::vec2(preview_w, preview_h),
                                            egui::Sense::hover(),
                                        );
                                        let painter = ui.painter();
                                        painter.rect_filled(
                                            rect,
                                            egui::CornerRadius::ZERO,
                                            egui::Color32::BLACK,
                                        );
                                        for y in 0..g.height {
                                            for x in 0..g.width {
                                                if g.pixel(x, y) {
                                                    let px_rect = egui::Rect::from_min_size(
                                                        egui::pos2(
                                                            rect.min.x + x as f32 * dot,
                                                            rect.min.y + y as f32 * dot,
                                                        ),
                                                        egui::vec2(dot, dot),
                                                    );
                                                    painter.rect_filled(
                                                        px_rect,
                                                        egui::CornerRadius::ZERO,
                                                        egui::Color32::WHITE,
                                                    );
                                                }
                                            }
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                });
        }
    }

    // 효과음·멜로디 편집기 탭 화면 (모든 변경을 명령 객체로 기록해 Ctrl+Z/Y 지원)
    fn show_sound_tab(&mut self, ui: &mut egui::Ui) {
        // 효과음 파일 저장·불러오기 바 (assets/sounds/<이름>.sound.toml)
        ui.horizontal(|ui| {
            ui.label("파일명 (sounds/*.sound.toml):");
            ui.add(
                egui::TextEdit::singleline(&mut self.sound_tab.asset_name)
                    .hint_text("예: jump")
                    .desired_width(140.0),
            );
            if ui.button("💾 저장 (Ctrl+S)").clicked() {
                self.save_current_sound();
            }
            if ui.button("📂 불러오기").clicked() {
                self.load_current_sound();
            }
            if !self.sound_tab.status_message.is_empty() {
                ui.label(&self.sound_tab.status_message);
            }
        });
        ui.add_space(4.0);

        // 1) 상단 재생·초기화·실행 취소/다시 실행 버튼 바
        ui.horizontal(|ui| {
            if ui.button("▶ 전체 재생").clicked() {
                play_melody_async(self.sound_tab.melody.notes().to_vec());
            }
            if ui.button("전체 지우기").clicked() {
                self.sound_tab.commit_notes_change(Vec::new()); // 빈 목록으로 명령 실행
            }
            ui.separator();

            // 실행 취소(Ctrl+Z) 버튼
            if ui
                .add_enabled(
                    self.sound_tab.history.can_undo(),
                    egui::Button::new("↶ 실행 취소 (Ctrl+Z)"),
                )
                .clicked()
            {
                self.sound_tab.history.undo(&mut self.sound_tab.melody);
            }
            // 다시 실행(Ctrl+Y) 버튼
            if ui
                .add_enabled(
                    self.sound_tab.history.can_redo(),
                    egui::Button::new("↷ 다시 실행 (Ctrl+Y)"),
                )
                .clicked()
            {
                self.sound_tab.history.redo(&mut self.sound_tab.melody);
            }

            ui.separator();
            ui.label(format!(
                "음표: {}개 | 총 길이: {} ms",
                self.sound_tab.melody.notes().len(),
                self.sound_tab.melody.total_duration_ms()
            ));
        });
        ui.add_space(6.0);

        // 2) 빠른 음계 프리셋 추가 (명령 객체로 기록)
        ui.horizontal(|ui| {
            ui.label("빠른 추가:");
            for (name, freq_hz) in NOTE_PRESETS {
                if ui.button(name).clicked() {
                    let mut next = self.sound_tab.snapshot_notes();
                    next.push((freq_hz, self.sound_tab.new_duration_ms));
                    self.sound_tab.commit_notes_change(next);
                }
            }
        });

        // 3) 직접 주파수·길이 입력 후 추가 (명령 객체로 기록 + 부저 범위 밖 경고)
        ui.horizontal(|ui| {
            ui.label("주파수(Hz, 0=쉼표):");
            ui.add(egui::DragValue::new(&mut self.sound_tab.new_freq_hz).range(0..=6000));
            if !is_valid_buzzer_freq(self.sound_tab.new_freq_hz) {
                ui.colored_label(
                    egui::Color32::from_rgb(240, 200, 80),
                    format!("⚠ 부저 권장 범위({BUZZER_MIN_FREQ_HZ}~{BUZZER_MAX_FREQ_HZ}Hz) 밖"),
                );
            }
            ui.label("길이(ms):");
            ui.add(egui::DragValue::new(&mut self.sound_tab.new_duration_ms).range(20..=2000));
            if ui.button("+ 음표 추가").clicked() {
                let mut next = self.sound_tab.snapshot_notes();
                next.push((self.sound_tab.new_freq_hz, self.sound_tab.new_duration_ms));
                self.sound_tab.commit_notes_change(next);
            }
        });

        // 전체 음표 중 부저 권장 범위를 벗어난 항목이 있으면 상단 경고 배너 표시
        let out_of_range_count = self
            .sound_tab
            .melody
            .notes()
            .iter()
            .filter(|n| !is_valid_buzzer_freq(n.freq_hz))
            .count();
        if out_of_range_count > 0 {
            ui.colored_label(
                egui::Color32::from_rgb(240, 200, 80),
                format!(
                    "⚠ 경고: 부저 발음 범위({BUZZER_MIN_FREQ_HZ}~{BUZZER_MAX_FREQ_HZ}Hz)를 벗어난 음표가 {out_of_range_count}개 있습니다."
                ),
            );
        }
        ui.separator();

        // 4) 음표 목록 순회 (인라인 값 수정·이동·삭제 모두 명령 객체로 일괄 기록)
        let before_snapshot = self.sound_tab.snapshot_notes(); // 루프 전 스냅샷
        let mut edited_inline = false; // DragValue로 값이 바뀌었는지 추적
        let mut remove_idx = None;
        let mut move_up_idx = None;
        let mut move_down_idx = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            let len = self.sound_tab.melody.notes().len();
            for (idx, note) in self.sound_tab.melody.notes_mut().iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("#{:02}", idx + 1));
                    ui.label("Hz:");
                    if ui
                        .add(egui::DragValue::new(&mut note.freq_hz).range(0..=6000))
                        .changed()
                    {
                        edited_inline = true;
                    }
                    if !is_valid_buzzer_freq(note.freq_hz) {
                        ui.colored_label(egui::Color32::from_rgb(240, 200, 80), "⚠ 범위 밖");
                    }
                    ui.label("ms:");
                    if ui
                        .add(egui::DragValue::new(&mut note.duration_ms).range(20..=2000))
                        .changed()
                    {
                        edited_inline = true;
                    }

                    if ui.button("▶").clicked() {
                        play_melody_async(vec![*note]); // 단일 음표 미리 듣기
                    }
                    if ui.add_enabled(idx > 0, egui::Button::new("▲")).clicked() {
                        move_up_idx = Some(idx);
                    }
                    if ui
                        .add_enabled(idx + 1 < len, egui::Button::new("▼"))
                        .clicked()
                    {
                        move_down_idx = Some(idx);
                    }
                    if ui.button("삭제").clicked() {
                        remove_idx = Some(idx);
                    }
                });
            }
        });

        // 루프 내 이동·삭제 요청을 MelodyEditor 메서드로 직접 적용
        if let Some(idx) = move_up_idx {
            self.sound_tab.melody.move_up(idx); // 선택한 음표를 한 칸 위로 이동
        } else if let Some(idx) = move_down_idx {
            self.sound_tab.melody.move_down(idx); // 선택한 음표를 한 칸 아래로 이동
        } else if let Some(idx) = remove_idx {
            self.sound_tab.melody.remove_note(idx); // 선택한 음표 삭제
        }

        // 인라인 수정·이동·삭제 중 하나라도 발생해 음표가 바뀌었으면 명령 객체로 기록
        if edited_inline || move_up_idx.is_some() || move_down_idx.is_some() || remove_idx.is_some()
        {
            let after_snapshot = self.sound_tab.snapshot_notes(); // 변경 후 음표 스냅샷 추출
            if before_snapshot != after_snapshot {
                let cmd = Box::new(ReplaceMelodyCommand::new(before_snapshot, after_snapshot));
                self.sound_tab
                    .history
                    .execute(&mut self.sound_tab.melody, cmd); // Undo/Redo 이력에 기록
            }
        }
    }

    // 장면(화면) 미리보기 탭 화면 (32×16 스프라이트·텍스트 임시 배치, 파일 저장 안 함)
    fn show_scene_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("장면 미리보기 (32×16)");
            ui.colored_label(
                egui::Color32::from_gray(145),
                "임시 배치 전용 — 파일로 저장되지 않습니다.",
            );
        });
        ui.add_space(4.0);

        // 1) 배경색 · 폰트 소스 · 전체 비우기 바
        ui.horizontal(|ui| {
            ui.label("배경색:");
            let mut bg = [
                self.scene_tab.scene.background.r,
                self.scene_tab.scene.background.g,
                self.scene_tab.scene.background.b,
            ];
            if ui.color_edit_button_srgb(&mut bg).changed() {
                self.scene_tab.scene.background = Rgb::new(bg[0], bg[1], bg[2]);
            }

            ui.separator();

            let has_ttf = self.font_tab.converter.is_some();
            ui.add_enabled(
                has_ttf,
                egui::Checkbox::new(&mut self.scene_tab.use_loaded_font, "글꼴 탭 TTF 사용"),
            );
            if !has_ttf {
                ui.colored_label(
                    egui::Color32::from_gray(140),
                    "(기본: 내장 3×5 폰트 · 글꼴 탭에서 TTF 불러오면 전환 가능)",
                );
            }

            ui.separator();
            if ui.button("전체 비우기").clicked() {
                self.scene_tab.scene.clear();
                self.scene_tab.selected_idx = None;
                self.scene_tab.drag_anchor = None;
                self.scene_tab.status_message.clear();
            }
        });

        // 2) 스프라이트 추가 바 (프로젝트 파일 또는 현재 편집 캔버스)
        ui.horizontal(|ui| {
            ui.label("스프라이트:");
            if self.scene_tab.selected_sprite_name.is_empty()
                && let Some(first) = self.sprite_files.first()
            {
                self.scene_tab.selected_sprite_name = first.clone();
            }

            egui::ComboBox::from_id_salt("scene_sprite_picker")
                .selected_text(if self.scene_tab.selected_sprite_name.is_empty() {
                    "(없음)".to_string()
                } else {
                    format!("{}.png", self.scene_tab.selected_sprite_name)
                })
                .show_ui(ui, |ui| {
                    for name in &self.sprite_files {
                        ui.selectable_value(
                            &mut self.scene_tab.selected_sprite_name,
                            name.clone(),
                            format!("{name}.png"),
                        );
                    }
                });

            let can_add_file = !self.scene_tab.selected_sprite_name.is_empty();
            if ui
                .add_enabled(can_add_file, egui::Button::new("+ 스프라이트 추가"))
                .clicked()
                && let Some(assets_dir) = self.opened.as_ref().map(|o| o.project.assets_dir())
            {
                let name = self.scene_tab.selected_sprite_name.clone();
                let path = assets_dir.join("sprites").join(format!("{name}.png"));
                match load_sprite_png(&path) {
                    Ok((w, h, pixels)) => {
                        let idx = self.scene_tab.scene.add_sprite(name, w, h, pixels, 0, 0);
                        self.scene_tab.selected_idx = Some(idx);
                        self.scene_tab.status_message.clear();
                    }
                    Err(err) => self.scene_tab.status_message = format!("오류: {err}"),
                }
            }

            if ui.button("+ 현재 캔버스 추가").clicked() {
                let w = self.editor.width();
                let h = self.editor.height();
                let pixels = snapshot_canvas(&self.editor);
                let label = if self.editing_icon {
                    "icon".to_string()
                } else {
                    format!("canvas_{}", self.sprite_asset_name)
                };
                let idx = self.scene_tab.scene.add_sprite(label, w, h, pixels, 0, 0);
                self.scene_tab.selected_idx = Some(idx);
                self.scene_tab.status_message.clear();
            }
        });

        // 3) 텍스트 추가 바
        ui.horizontal(|ui| {
            ui.label("텍스트:");
            ui.add(
                egui::TextEdit::singleline(&mut self.scene_tab.new_text)
                    .hint_text("예: SCORE 00")
                    .desired_width(140.0),
            );
            let mut rgb = [
                self.scene_tab.new_text_color.r,
                self.scene_tab.new_text_color.g,
                self.scene_tab.new_text_color.b,
            ];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                self.scene_tab.new_text_color = Rgb::new(rgb[0], rgb[1], rgb[2]);
            }
            if ui.button("+ 텍스트 추가").clicked() {
                let text = self.scene_tab.new_text.clone();
                let idx = self
                    .scene_tab
                    .scene
                    .add_text(text, 1, 1, self.scene_tab.new_text_color);
                self.scene_tab.selected_idx = Some(idx);
            }
            if !self.scene_tab.status_message.is_empty() {
                ui.colored_label(
                    egui::Color32::from_rgb(240, 100, 100),
                    &self.scene_tab.status_message,
                );
            }
        });
        ui.add_space(6.0);

        // 4) 32×16 합성 미리보기 캔버스 (선택된 항목 드래그 이동 지원)
        let active_font = if self.scene_tab.use_loaded_font {
            self.font_tab.converter.as_ref()
        } else {
            None
        };
        let fb = self.scene_tab.scene.render(active_font);
        let width = usize::from(fb.width());
        let height = usize::from(fb.height());
        let canvas_size = egui::vec2(width as f32 * CELL_SIZE, height as f32 * CELL_SIZE);
        let (rect, response) = ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());

        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
            && let Some(sel) = self.scene_tab.selected_idx
            && let Some(item) = self.scene_tab.scene.items.get_mut(sel)
        {
            let gx = ((pos.x - rect.min.x) / CELL_SIZE).floor() as i32;
            let gy = ((pos.y - rect.min.y) / CELL_SIZE).floor() as i32;
            let (start_gx, start_gy, orig_x, orig_y) = *self
                .scene_tab
                .drag_anchor
                .get_or_insert((gx, gy, item.x, item.y));
            item.x = (orig_x + (gx - start_gx)).clamp(-32, 32);
            item.y = (orig_y + (gy - start_gy)).clamp(-16, 16);
        } else if !response.dragged() {
            self.scene_tab.drag_anchor = None;
        }

        let painter = ui.painter();
        let grid_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(45));
        for y in 0..height {
            for x in 0..width {
                let cell_min = egui::pos2(
                    rect.min.x + x as f32 * CELL_SIZE,
                    rect.min.y + y as f32 * CELL_SIZE,
                );
                let cell_rect =
                    egui::Rect::from_min_size(cell_min, egui::vec2(CELL_SIZE, CELL_SIZE));
                let raw = fb.pixel(x as i32, y as i32).unwrap_or(Rgb::BLACK);
                let display = if self.settings.led_preview {
                    scale_brightness(raw, self.settings.brightness_percent)
                } else {
                    raw
                };
                painter.rect_filled(cell_rect, egui::CornerRadius::ZERO, to_color32(display));
                painter.rect_stroke(
                    cell_rect,
                    egui::CornerRadius::ZERO,
                    grid_stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
        draw_panel_divider(painter, rect);

        ui.add_space(4.0);
        ui.colored_label(
            egui::Color32::from_gray(145),
            "팁: 아래 목록에서 항목을 선택한 뒤 32×16 화면 위를 드래그하면 위치를 바로 옮길 수 있습니다.",
        );
        ui.separator();

        // 5) 배치된 레이어 목록 및 위치·속성·순서 편집
        let len = self.scene_tab.scene.items.len();
        if len == 0 {
            ui.label("배치된 항목이 없습니다. 위에서 스프라이트나 텍스트를 추가하세요.");
            return;
        }

        let mut move_up_idx = None;
        let mut move_down_idx = None;
        let mut remove_idx = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (idx, item) in self.scene_tab.scene.items.iter_mut().enumerate() {
                let is_selected = self.scene_tab.selected_idx == Some(idx);
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(
                            is_selected,
                            format!("#{} {}", idx + 1, item.display_label()),
                        )
                        .clicked()
                    {
                        self.scene_tab.selected_idx = Some(idx);
                    }
                    ui.checkbox(&mut item.visible, "표시");
                    ui.label("X:");
                    ui.add(egui::DragValue::new(&mut item.x).range(-32..=32));
                    ui.label("Y:");
                    ui.add(egui::DragValue::new(&mut item.y).range(-16..=16));

                    match &mut item.kind {
                        SceneItemKind::Sprite {
                            transparent_black, ..
                        } => {
                            ui.checkbox(transparent_black, "검정 투명");
                        }
                        SceneItemKind::Text {
                            text,
                            color,
                            pixel_size,
                            ..
                        } => {
                            ui.add(egui::TextEdit::singleline(text).desired_width(90.0));
                            let mut rgb = [color.r, color.g, color.b];
                            if ui.color_edit_button_srgb(&mut rgb).changed() {
                                *color = Rgb::new(rgb[0], rgb[1], rgb[2]);
                            }
                            if self.scene_tab.use_loaded_font && self.font_tab.converter.is_some() {
                                ui.add(
                                    egui::DragValue::new(pixel_size)
                                        .range(6.0..=16.0)
                                        .speed(0.5)
                                        .suffix("px"),
                                );
                            }
                        }
                    }

                    if ui.add_enabled(idx > 0, egui::Button::new("▲")).clicked() {
                        move_up_idx = Some(idx);
                    }
                    if ui
                        .add_enabled(idx + 1 < len, egui::Button::new("▼"))
                        .clicked()
                    {
                        move_down_idx = Some(idx);
                    }
                    if ui.button("삭제").clicked() {
                        remove_idx = Some(idx);
                    }
                });
            }
        });

        if let Some(idx) = move_up_idx {
            if self.scene_tab.scene.move_up(idx) {
                self.scene_tab.selected_idx = match self.scene_tab.selected_idx {
                    Some(sel) if sel == idx => Some(idx - 1),
                    Some(sel) if sel == idx - 1 => Some(idx),
                    other => other,
                };
            }
        } else if let Some(idx) = move_down_idx {
            if self.scene_tab.scene.move_down(idx) {
                self.scene_tab.selected_idx = match self.scene_tab.selected_idx {
                    Some(sel) if sel == idx => Some(idx + 1),
                    Some(sel) if sel == idx + 1 => Some(idx),
                    other => other,
                };
            }
        } else if let Some(idx) = remove_idx
            && self.scene_tab.scene.remove_item(idx)
        {
            self.scene_tab.selected_idx = match self.scene_tab.selected_idx {
                Some(sel) if sel == idx => None,
                Some(sel) if sel > idx => Some(sel - 1),
                other => other,
            };
        }
    }

    // 기기 패널 탭 화면 (열린 프로젝트 경로 고정 · 구조화 doctor · 실시간 실행/중지)
    fn show_device_tab(&mut self, ui: &mut egui::Ui) {
        let is_busy = self.device_tab.running_cmd.is_some();
        let project_root = self
            .opened
            .as_ref()
            .map(|o| o.project.root().display().to_string())
            .unwrap_or_default();

        // 프로젝트 경로는 열린 프로젝트 루트로 고정(읽기 전용)
        ui.horizontal(|ui| {
            ui.label("열린 프로젝트:");
            ui.monospace(&project_root);
            ui.separator();
            ui.label("CLI 명령:");
            ui.add(
                egui::TextEdit::singleline(&mut self.device_tab.dotbox_bin).desired_width(120.0),
            );
        });
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            // 구조화된 dotbox_tools::doctor::run으로 점검 후 개요 탭으로 이동
            let doctor_busy = self.doctor_rx.is_some();
            if ui
                .add_enabled(!doctor_busy, egui::Button::new("환경 점검 (개요/문제)"))
                .clicked()
            {
                self.start_doctor_check();
                self.tabs.open(TabKind::Overview);
            }

            if ui
                .add_enabled(
                    !is_busy,
                    egui::Button::new("▶ 시뮬레이터 실행 (dotbox run)"),
                )
                .clicked()
            {
                self.show_bottom_panel = true;
                self.bottom_tab = BottomTab::Output;
                self.launch_cli_command(build_run_args());
            }

            // 실행 중인 명령이 있을 때 즉시 종료할 수 있는 중지 버튼
            if is_busy {
                if ui.button("■ 중지").clicked() {
                    self.stop_cli_command();
                }
                ui.label("실행 중... (하단 출력 패널에 실시간 표시)");
            }
        });
        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("시리얼 포트 새로고침").clicked() {
                self.refresh_serial_ports();
            }
            ui.label(&self.device_tab.port_status);
        });

        for port in &self.device_tab.ports {
            ui.horizontal(|ui| {
                if ui.button(&port.name).clicked() {
                    self.device_tab.selected_port = port.name.clone();
                }
                ui.label(&port.description);
            });
        }
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("플래시 대상:");
            ui.selectable_value(
                &mut self.device_tab.selected_target,
                FlashTarget::Left,
                "왼쪽 모듈 (left)",
            );
            ui.selectable_value(
                &mut self.device_tab.selected_target,
                FlashTarget::Right,
                "오른쪽 모듈 (right)",
            );
            ui.selectable_value(
                &mut self.device_tab.selected_target,
                FlashTarget::Both,
                "양쪽 모두 (both)",
            );
        });

        ui.horizontal(|ui| {
            ui.label("지정 포트 (비우면 자동):");
            let port_enabled = self.device_tab.selected_target != FlashTarget::Both;
            ui.add_enabled(
                port_enabled,
                egui::TextEdit::singleline(&mut self.device_tab.selected_port).desired_width(120.0),
            );

            if ui
                .add_enabled(!is_busy, egui::Button::new("펌웨어 플래시 실행"))
                .clicked()
            {
                let right_port = self
                    .opened
                    .as_ref()
                    .and_then(|o| o.local_config.ports.right.as_deref())
                    .unwrap_or("");
                let args = build_flash_args(
                    self.device_tab.selected_target,
                    &self.device_tab.selected_port,
                    right_port,
                );
                self.show_bottom_panel = true;
                self.bottom_tab = BottomTab::Output;
                self.launch_cli_command(args);
            }
        });
    }

    // 현재 열린 프로젝트 루트에서 CLI 명령을 줄 단위 스트리밍으로 시작
    fn launch_cli_command(&mut self, args: Vec<String>) {
        let Some(work_dir) = self.opened.as_ref().map(|o| o.project.root().to_path_buf()) else {
            return;
        };

        let header = format!("$ {}\n", format_command(&self.device_tab.dotbox_bin, &args));
        match spawn_command_streaming(&self.device_tab.dotbox_bin, &args, &work_dir) {
            Ok(running) => {
                self.build_diagnostics.clear(); // 새 빌드/실행 시 이전 컴파일 문제 목록 초기화
                self.device_tab.output_log = header; // 명령 헤더 표시 후 실시간 줄 추가
                self.device_tab.running_cmd = Some(running);
            }
            Err(err) => {
                self.device_tab.output_log = err; // 실행 실패 시 즉시 오류 출력
                self.device_tab.running_cmd = None;
            }
        }
    }

    // 실행 중인 CLI 자식 프로세스를 즉시 중지하고 로그에 기록
    fn stop_cli_command(&mut self) {
        if let Some(mut cmd) = self.device_tab.running_cmd.take() {
            cmd.stop();
            self.device_tab
                .output_log
                .push_str("\n[중지됨] 사용자 요청으로 프로세스를 종료했습니다.\n");
        }
    }
}

// 단일 모듈(왼쪽 또는 오른쪽) 시리얼 로그 패널 렌더링 헬퍼
fn show_serial_pane(
    ui: &mut egui::Ui,
    title: &str,
    id_salt: &str,
    monitor: &mut SerialMonitor,
    search: &str,
    project_root: Option<&Path>,
    save_status: &mut String,
) {
    ui.horizontal(|ui| {
        ui.strong(title);
        ui.add_enabled(
            !monitor.is_connected(),
            egui::TextEdit::singleline(&mut monitor.port_name)
                .hint_text("COM5")
                .desired_width(68.0),
        );
        if monitor.is_connected() {
            if ui.small_button("해제").clicked() {
                monitor.disconnect();
            }
        } else if ui.small_button("연결").clicked() {
            monitor.connect();
        }

        let pause_label = if monitor.paused {
            format!("재개 (+{})", monitor.pending_count())
        } else {
            "일시 정지".to_string()
        };
        if ui.small_button(pause_label).clicked() {
            monitor.toggle_pause();
        }
        if ui.small_button("지우기").clicked() {
            monitor.clear();
        }
        if ui.small_button("저장").clicked()
            && let Some(root) = project_root
        {
            let path = root.join(".dotbox").join(format!("{id_salt}.log"));
            match monitor.save_to_file(&path) {
                Ok(()) => *save_status = format!("저장 완료: {}", path.display()),
                Err(err) => *save_status = format!("오류: {err}"),
            }
        }
    });
    ui.colored_label(egui::Color32::from_gray(145), &monitor.status);

    let filtered = monitor.filtered_lines(search);
    egui::ScrollArea::vertical()
        .id_salt(id_salt)
        .max_height(105.0)
        .stick_to_bottom(!monitor.paused)
        .show(ui, |ui| {
            if filtered.is_empty() {
                ui.colored_label(egui::Color32::from_gray(120), "(수신된 로그 없음)");
            } else {
                for line in filtered {
                    if line.is_reboot_or_panic {
                        // 패닉·재부팅 징후가 보이면 빨간색 굵은 글씨로 강조
                        ui.label(
                            egui::RichText::new(&line.text)
                                .monospace()
                                .strong()
                                .color(egui::Color32::from_rgb(255, 95, 95)),
                        );
                    } else {
                        ui.monospace(&line.text);
                    }
                }
            }
        });
}

// doctor 점검 수준별 표시 색상 반환
fn level_color(level: Level) -> egui::Color32 {
    match level {
        Level::Ok => egui::Color32::from_rgb(120, 220, 140), // 정상: 초록
        Level::Info => egui::Color32::from_rgb(160, 200, 240), // 정보: 하늘색
        Level::Warn => egui::Color32::from_rgb(240, 200, 80), // 경고: 노랑
        Level::Error => egui::Color32::from_rgb(240, 100, 100), // 오류: 빨강
    }
}

// 읽기 전용 FrameBuffer를 32×16 도트 매트릭스로 화면에 그리는 헬퍼 함수
fn draw_framebuffer_preview(
    ui: &mut egui::Ui,
    fb: &FrameBuffer,
    led_preview: bool,
    brightness_percent: u8,
) {
    let width = usize::from(fb.width());
    let height = usize::from(fb.height());
    let size = egui::vec2(width as f32 * CELL_SIZE, height as f32 * CELL_SIZE);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());

    let painter = ui.painter();
    let grid_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(45));

    for y in 0..height {
        for x in 0..width {
            let cell_min = egui::pos2(
                rect.min.x + x as f32 * CELL_SIZE,
                rect.min.y + y as f32 * CELL_SIZE,
            );
            let cell_rect = egui::Rect::from_min_size(cell_min, egui::vec2(CELL_SIZE, CELL_SIZE));

            let raw = fb.pixel(x as i32, y as i32).unwrap_or(Rgb::BLACK);
            let display = if led_preview {
                scale_brightness(raw, brightness_percent)
            } else {
                raw
            };

            painter.rect_filled(cell_rect, egui::CornerRadius::ZERO, to_color32(display));
            painter.rect_stroke(
                cell_rect,
                egui::CornerRadius::ZERO,
                grid_stroke,
                egui::StrokeKind::Inside,
            );
        }
    }

    draw_panel_divider(painter, rect);
}

// 32×16 화면 중앙(x=16)에 왼쪽·오른쪽 패널 경계선 그리기
fn draw_panel_divider(painter: &egui::Painter, rect: egui::Rect) {
    let split_x = rect.min.x + f32::from(PANEL_WIDTH) * CELL_SIZE;
    painter.line_segment(
        [
            egui::pos2(split_x, rect.min.y),
            egui::pos2(split_x, rect.max.y),
        ],
        egui::Stroke::new(2.5, egui::Color32::from_rgb(255, 200, 40)),
    );
}

// 엔진 Rgb 타입을 egui 색상 타입(Color32)으로 변환
fn to_color32(color: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(color.r, color.g, color.b)
}

// 매 프레임 화면을 갱신하는 egui 루프
impl eframe::App for StudioApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // OS 창 닫기(X) 요청 감지: 미저장 탭 확인·CLI 정리·마지막 창 크기 저장
        self.handle_window_close(ui.ctx());
        // 백그라운드 작업(doctor, CLI) 완료 여부 매 프레임 확인
        self.poll_background_tasks(ui.ctx());

        // 열린 프로젝트가 없으면 전체 화면에 시작 화면만 표시
        if self.opened.is_none() {
            egui::CentralPanel::default().show(ui, |ui| {
                self.show_start_screen(ui); // 시작 화면 렌더링
            });
            return;
        }

        // 전역 단축키(Ctrl+S / Ctrl+Z / Ctrl+Y / Ctrl+W) 입력 처리
        self.handle_shortcuts(ui.ctx());

        // 1) 상단 도구 모음 패널
        let mut want_close = false;
        egui::Panel::top("top_toolbar").show(ui, |ui| {
            want_close = self.show_top_bar(ui); // 상단 바 렌더링
        });
        if want_close {
            self.request_close_project(); // 미저장 탭 확인 후 닫기 처리
        }

        // 2) 하단 패널 ([출력] · [문제 N]) — 표시 옵션이 켜져 있을 때만 렌더링
        if self.show_bottom_panel {
            egui::Panel::bottom("bottom_panel")
                .resizable(true)
                .default_size(150.0)
                .show(ui, |ui| {
                    self.show_bottom_panel(ui); // 하단 패널 렌더링
                });
        }

        // 3) 좌측 프로젝트 탐색기 패널
        egui::Panel::left("explorer_panel")
            .resizable(true)
            .default_size(180.0)
            .show(ui, |ui| {
                self.show_explorer_panel(ui); // 좌측 탐색기 렌더링
            });

        // 4) 중앙 작업 공간 패널 (상단 탭 바 + 활성 탭 편집기)
        egui::CentralPanel::default().show(ui, |ui| {
            self.show_editor_tab_bar(ui); // 파일 단위 탭 바 표시
            ui.separator();
            ui.add_space(4.0);

            match self.tabs.active() {
                TabKind::Overview => self.show_overview_tab(ui), // 프로젝트 개요 탭
                TabKind::ProjectSettings => self.show_project_settings_tab(ui), // 프로젝트 설정 탭
                TabKind::SpriteEditor => {
                    self.show_sprite_toolbar(ui); // 화면 모드·크기·도구·파일 저장 바
                    ui.add_space(6.0);
                    self.show_palette(ui); // 색상 팔레트 및 컬러 피커
                    ui.add_space(10.0);
                    self.show_canvas(ui); // 도트 캔버스
                    ui.add_space(8.0);
                    self.show_animation_bar(ui); // 애니메이션 프레임 타임라인 및 재생
                    ui.add_space(6.0);
                    ui.label(
                        "조작: 마우스 왼쪽 클릭/드래그 = 선택 도구 사용 | 오른쪽 클릭/드래그 = 빠른 지우기",
                    );
                }
                TabKind::FontTool => self.show_font_tab(ui), // 글꼴 변환·미리보기 탭
                TabKind::SoundTool => self.show_sound_tab(ui), // 효과음·멜로디 편집 탭
                TabKind::ScenePreview => self.show_scene_tab(ui), // 장면(화면) 미리보기 탭
                TabKind::DevicePanel => self.show_device_tab(ui), // 기기 패널 탭
            }
        });

        // 5) 미저장 변경 사항 닫기 확인 및 에셋 관리 모달 표시 (필요 시)
        self.show_confirm_dialog(ui.ctx());
        self.show_asset_modal(ui.ctx());
    }
}

// 프로그램 시작점 (실행 인자 `dotbox-studio [DIR]` 지원)
fn main() -> eframe::Result {
    // 첫 번째 명령행 인자가 있으면 열 프로젝트 경로로 사용
    let initial_dir = std::env::args_os().nth(1).map(PathBuf::from);
    // 저장된 사용자 설정(창 크기, 최근 프로젝트, 밝기) 불러오기
    let settings = UserSettings::load();

    // 창 초기 크기 설정
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([settings.window_width, settings.window_height]),
        ..Default::default()
    };

    // 네이티브 윈도우 실행
    eframe::run_native(
        "DotBox Studio",
        options,
        Box::new(move |_cc| Ok(Box::new(StudioApp::new(initial_dir, settings)))),
    )
}
