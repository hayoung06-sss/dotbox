mod device; // 4순위: 환경 점검·포트 감지·플래시 로직 모듈
mod editor; // 1순위: 스프라이트·화면 편집 로직 모듈
mod font; // 2순위: 글꼴 변환·미리보기 로직 모듈
mod settings; // 사용자 전역 설정 (최근 프로젝트, 창 크기, 밝기)
mod sound; // 3순위: 효과음·멜로디 편집 및 재생 로직 모듈
mod workspace; // 프로젝트 열기·생성·설정 저장·doctor 실행 모듈

use device::{
    DetectedPort, FlashTarget, build_doctor_args, build_flash_args, build_run_args,
    run_command_async, scan_serial_ports,
};
use dotbox_core::display::{FrameBuffer, PANEL_WIDTH, ScreenLayout};
use dotbox_core::hal::Rgb;
use dotbox_tools::doctor::{Level, Report};
use editor::{CanvasEditor, PALETTE, scale_brightness};
use eframe::egui;
use font::{FontConfig, FontConverter};
use settings::UserSettings;
use sound::{MelodyEditor, NOTE_PRESETS, play_melody_async};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use workspace::{EngineSourceKind, NewProjectDraft, OpenedProject, run_doctor_async};

// 캔버스 한 칸(도트 1개)의 화면 픽셀 크기
const CELL_SIZE: f32 = 24.0;

// 중앙 작업 공간 탭 종류 (프로젝트 열면 개요 탭이 기본)
#[derive(Default, PartialEq, Eq)]
enum ActiveTab {
    #[default]
    Overview, // 프로젝트 개요 (dotbox.toml · doctor · 포트/build_id)
    ProjectSettings, // 프로젝트 설정 (dotbox.toml [paths] · 좌/우 포트)
    SpriteEditor,    // 스프라이트·화면 편집기
    FontTool,        // 글꼴 변환과 미리보기
    SoundTool,       // 효과음·멜로디 편집기
    DevicePanel,     // 기기 패널 (CLI 실행 · 포트 · 플래시)
}

// 하단 패널 탭 종류
#[derive(Default, PartialEq, Eq)]
enum BottomTab {
    #[default]
    Output, // CLI 실행 출력 로그
    Problems, // doctor 점검 오류·경고 문제 목록
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
    preview_text: String,             // 32×16 화면에 미리 볼 문구
    converter: Option<FontConverter>, // 불러온 폰트 변환기 (불러오기 전엔 None)
    status_message: String,           // 성공/오류 안내 메시지 (패닉 방지)
}

impl Default for FontTabState {
    // Windows 기본 맑은 고딕 폰트 경로로 초기 상태 설정
    fn default() -> Self {
        Self {
            font_path: r"C:\Windows\Fonts\malgun.ttf".to_string(),
            config: FontConfig::default(),
            preview_text: "DotBox".to_string(),
            converter: None,
            status_message: "TTF/OTF 경로를 입력하고 [폰트 불러오기]를 누르세요.".to_string(),
        }
    }
}

// 효과음·멜로디 탭 전용 UI 상태
struct SoundTabState {
    melody: MelodyEditor, // 음표 시퀀스 편집기
    new_freq_hz: u16,     // 새로 추가할 음표의 주파수 (Hz)
    new_duration_ms: u16, // 새로 추가할 음표의 길이 (ms)
}

impl Default for SoundTabState {
    // 기본 음표 입력값(도 523Hz, 150ms)으로 초기화
    fn default() -> Self {
        Self {
            melody: MelodyEditor::default(),
            new_freq_hz: 523,
            new_duration_ms: 150,
        }
    }
}

// 기기 패널 탭 전용 UI 상태
struct DeviceTabState {
    dotbox_bin: String,                   // 실행할 dotbox CLI 경로 또는 명령 이름
    project_dir: String,                  // 명령을 실행할 DotBox 게임 프로젝트 경로
    ports: Vec<DetectedPort>,             // 마지막으로 스캔한 시리얼 포트 목록
    port_status: String,                  // 포트 스캔 상태 메시지
    selected_target: FlashTarget,         // 플래시 대상 (좌측 / 우측 / 양쪽)
    selected_port: String,                // 플래시 시 지정할 시리얼 포트 (비우면 자동)
    pending_rx: Option<Receiver<String>>, // 백그라운드 명령 결과 수신기
    output_log: String,                   // CLI 실행 로그 출력 내용
}

impl Default for DeviceTabState {
    // 현재 작업 디렉터리를 기본 프로젝트 경로로 설정
    fn default() -> Self {
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string());

        Self {
            dotbox_bin: "dotbox".to_string(),
            project_dir: cwd,
            ports: Vec::new(),
            port_status: "[시리얼 포트 새로고침]을 눌러 연결된 기기를 확인하세요.".to_string(),
            selected_target: FlashTarget::Left,
            selected_port: String::new(),
            pending_rx: None,
            output_log: "실행 로그가 여기에 표시됩니다.".to_string(),
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
    doctor_report: Option<Report>,  // 마지막 doctor::run 점검 결과
    doctor_rx: Option<Receiver<Report>>, // 백그라운드 doctor 실행 수신기
    doctor_host_only: bool,         // 시뮬레이터 전용 점검 옵션 여부
    active_tab: ActiveTab,          // 현재 열려 있는 중앙 탭
    bottom_tab: BottomTab,          // 현재 열려 있는 하단 패널 탭
    show_bottom_panel: bool,        // 하단 패널 펼침 여부
    editor: CanvasEditor,           // 스프라이트·화면 편집기 상태
    font_tab: FontTabState,         // 글꼴 변환기 상태
    sound_tab: SoundTabState,       // 효과음·멜로디 편집기 상태
    device_tab: DeviceTabState,     // 기기 패널 상태
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
            doctor_rx: None,
            doctor_host_only: false,
            active_tab: ActiveTab::Overview, // 프로젝트 열면 개요 탭부터 시작
            bottom_tab: BottomTab::Output,
            show_bottom_panel: true, // 하단 패널 기본 표시
            editor,
            font_tab: FontTabState::default(),
            sound_tab: SoundTabState::default(),
            device_tab: DeviceTabState::default(),
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
                // 기기 패널의 프로젝트 경로 및 왼쪽 포트 동기화
                self.device_tab.project_dir = root.display().to_string();
                self.device_tab.selected_port =
                    opened.local_config.ports.left.clone().unwrap_or_default();
                // 프로젝트 설정 폼 동기화
                self.settings_form.sync_from(&opened);
                // 최근 프로젝트 목록 맨 앞에 추가 후 사용자 설정 저장
                self.settings.push_recent(root);
                let _ = self.settings.save();
                // 열린 프로젝트 저장 후 개요 탭으로 전환
                self.opened = Some(opened);
                self.active_tab = ActiveTab::Overview;
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

    // 현재 프로젝트 닫고 시작 화면으로 돌아가기
    fn close_project(&mut self) {
        self.opened = None; // 열린 프로젝트 비우기
        self.doctor_report = None; // 이전 프로젝트의 doctor 결과 정리
        self.doctor_rx = None; // 진행 중이던 doctor 수신기 정리
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

        // 2) 기기 패널 CLI 명령 결과 수신 확인
        if let Some(rx) = &self.device_tab.pending_rx {
            match rx.try_recv() {
                Ok(log) => {
                    self.device_tab.output_log = log; // 실행 로그 반영
                    self.device_tab.pending_rx = None; // 수신기 정리
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint(); // 실행 중이면 계속 화면 갱신 요청
                }
                Err(TryRecvError::Disconnected) => {
                    self.device_tab.pending_rx = None; // 채널 끊김 시 정리
                }
            }
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
        let is_cli_busy = self.device_tab.pending_rx.is_some(); // CLI 실행 중 여부

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

            // 시뮬레이터 실행 버튼 (하단 출력 패널 자동 열기)
            if ui
                .add_enabled(!is_cli_busy, egui::Button::new("▶ 실행"))
                .clicked()
            {
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

            // 전역 실기기 밝기 미리보기 설정 (명세서 4.5: 도구 모음에 전역 1개로 통일)
            let mut preview = self.editor.led_preview();
            if ui.checkbox(&mut preview, "실기기 밝기").changed() {
                self.editor.set_led_preview(preview);
                self.settings.led_preview = preview;
                let _ = self.settings.save();
            }
            if preview {
                let mut brightness = self.editor.brightness_percent();
                if ui
                    .add(egui::Slider::new(&mut brightness, 15..=25).suffix("%"))
                    .changed()
                {
                    self.editor.set_brightness_percent(brightness);
                    self.settings.brightness_percent = brightness;
                    let _ = self.settings.save();
                }
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

    // 좌측 프로젝트 탐색기 패널 렌더링 (명세서 3장)
    fn show_explorer_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("탐색기");
        ui.separator();

        // 프로젝트 관리 항목
        ui.selectable_value(&mut self.active_tab, ActiveTab::Overview, "개요");
        ui.selectable_value(
            &mut self.active_tab,
            ActiveTab::ProjectSettings,
            "프로젝트 설정",
        );

        ui.add_space(8.0);
        ui.label("에셋 (S2 합의 후 활성화)");
        ui.colored_label(egui::Color32::from_gray(130), "  sprites/");
        ui.colored_label(egui::Color32::from_gray(130), "  sounds/");
        ui.colored_label(egui::Color32::from_gray(130), "  fonts/");
        ui.colored_label(egui::Color32::from_gray(130), "  icon.png");

        ui.add_space(8.0);
        ui.separator();
        ui.label("도구");
        // 기존 1~4순위 도구 탭 전환 항목
        ui.selectable_value(
            &mut self.active_tab,
            ActiveTab::SpriteEditor,
            "스프라이트 편집기",
        );
        ui.selectable_value(&mut self.active_tab, ActiveTab::FontTool, "글꼴 변환기");
        ui.selectable_value(&mut self.active_tab, ActiveTab::SoundTool, "효과음 편집기");
        ui.selectable_value(&mut self.active_tab, ActiveTab::DevicePanel, "기기 패널");
    }

    // 하단 패널 렌더링 ([출력] · [문제 N])
    fn show_bottom_panel(&mut self, ui: &mut egui::Ui) {
        // doctor 점검 결과에서 경고·오류 항목 개수 계산
        let problem_count = self
            .doctor_report
            .as_ref()
            .map(|r| r.count(Level::Error) + r.count(Level::Warn))
            .unwrap_or(0);

        // 하단 탭 헤더 바
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.bottom_tab, BottomTab::Output, "출력");
            ui.selectable_value(
                &mut self.bottom_tab,
                BottomTab::Problems,
                format!("문제 ({problem_count})"),
            );
        });
        ui.separator();

        // 선택된 하단 탭 내용 표시
        egui::ScrollArea::vertical()
            .max_height(130.0)
            .show(ui, |ui| match self.bottom_tab {
                BottomTab::Output => {
                    ui.monospace(&self.device_tab.output_log); // CLI 실행 로그 출력
                }
                BottomTab::Problems => {
                    let Some(report) = &self.doctor_report else {
                        ui.label("환경 점검 결과가 아직 없습니다.");
                        return;
                    };

                    // 오류·경고 항목만 필터링해 목록으로 표시
                    let mut jump_to_overview = false;
                    let mut count = 0;
                    for check in &report.checks {
                        if check.level == Level::Error || check.level == Level::Warn {
                            count += 1;
                            let color = level_color(check.level);
                            let line = format!(
                                "[{}] {} — {}",
                                check.level.as_str().to_uppercase(),
                                check.id,
                                check.message
                            );
                            // 항목 클릭 시 개요 탭으로 이동해 상세 힌트 확인 가능
                            if ui
                                .add(
                                    egui::Label::new(egui::RichText::new(line).color(color))
                                        .sense(egui::Sense::click()),
                                )
                                .clicked()
                            {
                                jump_to_overview = true;
                            }
                        }
                    }

                    if count == 0 {
                        ui.label("발견된 문제가 없습니다.");
                    }
                    if jump_to_overview {
                        self.active_tab = ActiveTab::Overview;
                    }
                }
            });
    }

    // S1-3: 개요 탭 화면 (명세서 4.2)
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
                                // 힌트가 있는 항목은 들여쓰기해 안내 표시
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
                    self.active_tab = ActiveTab::ProjectSettings;
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

    // S1-3: 프로젝트 설정 탭 화면 (명세서 4.3)
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
                                "dotbox.toml 경로 설정을 저장했습니다.".to_string(); // 성공 안내
                            self.settings_form.error_message.clear();
                        }
                        Err(err) => {
                            self.settings_form.error_message = err; // 실패 이유 표시
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
                            // 기기 패널의 기본 포트에도 즉시 반영
                            self.device_tab.selected_port =
                                opened.local_config.ports.left.clone().unwrap_or_default();
                            self.settings_form.status_message =
                                ".dotbox/local.toml 포트 설정을 저장했습니다.".to_string(); // 성공 안내
                            self.settings_form.error_message.clear();
                        }
                        Err(err) => {
                            self.settings_form.error_message = err; // 실패 이유 표시
                            self.settings_form.status_message.clear();
                        }
                    }
                }
            });
        });
    }

    // 스프라이트 편집기 도구 모음 (화면 모드 전환 · 전체 지우기)
    fn show_sprite_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("화면 모드:");
            let is_wide = matches!(self.editor.layout(), ScreenLayout::Wide);

            // Wide(32×16) 전환 버튼
            if ui.selectable_label(is_wide, "Wide (32×16)").clicked() && !is_wide {
                self.editor.set_layout(ScreenLayout::Wide);
            }
            // Mirrored(16×16) 전환 버튼
            if ui.selectable_label(!is_wide, "Mirrored (16×16)").clicked() && is_wide {
                self.editor.set_layout(ScreenLayout::Mirrored);
            }

            ui.separator();

            // 캔버스 전체를 검은색으로 초기화
            if ui.button("전체 지우기").clicked() {
                self.editor.clear();
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

    // 32×16 (또는 16×16) 도트 캔버스 렌더링 및 마우스 입력 처리
    fn show_canvas(&mut self, ui: &mut egui::Ui) {
        let width = self.editor.width(); // 가로 도트 수
        let height = self.editor.height(); // 세로 도트 수
        let canvas_size = egui::vec2(width as f32 * CELL_SIZE, height as f32 * CELL_SIZE);

        let (rect, response) = ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());

        // 마우스 입력 처리: 좌클릭/드래그 = 찍기, 우클릭/드래그 = 지우기
        if let Some(pointer_pos) = response.interact_pointer_pos() {
            let rel = pointer_pos - rect.min;
            if rel.x >= 0.0 && rel.y >= 0.0 {
                let x = (rel.x / CELL_SIZE) as usize;
                let y = (rel.y / CELL_SIZE) as usize;

                if x < width && y < height {
                    if response.clicked_by(egui::PointerButton::Secondary)
                        || response.dragged_by(egui::PointerButton::Secondary)
                    {
                        self.editor.erase_pixel(x, y); // 우클릭: 지우기
                    } else if response.clicked_by(egui::PointerButton::Primary)
                        || response.dragged_by(egui::PointerButton::Primary)
                    {
                        self.editor.paint_pixel(x, y); // 좌클릭: 찍기
                    }
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

        // Wide(32×16) 모드일 때 중앙(x=16) 패널 경계선 표시
        if matches!(self.editor.layout(), ScreenLayout::Wide) {
            draw_panel_divider(painter, rect);
        }
    }

    // 글꼴 변환과 미리보기 탭 화면
    fn show_font_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("TTF/OTF 경로:");
            ui.text_edit_singleline(&mut self.font_tab.font_path);
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
        ui.add_space(6.0);

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
            ui.label(format!(
                "변환된 고유 글자 수: {}자 (1비트 비트맵)",
                glyphs.len()
            ));
            ui.add_space(4.0);

            // 글꼴 미리보기 색상은 흰색 고정 (스프라이트 브러시 색과 상태 분리)
            let fb = converter.render_preview(
                &self.font_tab.preview_text,
                self.font_tab.config.pixel_size,
                self.font_tab.config.threshold,
                Rgb::WHITE,
            );
            draw_framebuffer_preview(
                ui,
                &fb,
                self.editor.led_preview(),
                self.editor.brightness_percent(),
            );
        }
    }

    // 효과음·멜로디 편집기 탭 화면
    fn show_sound_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("▶ 전체 재생").clicked() {
                play_melody_async(self.sound_tab.melody.notes().to_vec());
            }
            if ui.button("전체 지우기").clicked() {
                self.sound_tab.melody.clear();
            }
            ui.separator();
            ui.label(format!(
                "음표: {}개 | 총 길이: {} ms",
                self.sound_tab.melody.notes().len(),
                self.sound_tab.melody.total_duration_ms()
            ));
        });
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.label("빠른 추가:");
            for (name, freq_hz) in NOTE_PRESETS {
                if ui.button(name).clicked() {
                    self.sound_tab
                        .melody
                        .add_note(freq_hz, self.sound_tab.new_duration_ms);
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("주파수(Hz, 0=쉼표):");
            ui.add(egui::DragValue::new(&mut self.sound_tab.new_freq_hz).range(0..=4000));
            ui.label("길이(ms):");
            ui.add(egui::DragValue::new(&mut self.sound_tab.new_duration_ms).range(20..=2000));
            if ui.button("+ 음표 추가").clicked() {
                self.sound_tab
                    .melody
                    .add_note(self.sound_tab.new_freq_hz, self.sound_tab.new_duration_ms);
            }
        });
        ui.separator();

        // 순회 중 변경 충돌 방지를 위한 임시 인덱스 변수
        let mut remove_idx = None;
        let mut move_up_idx = None;
        let mut move_down_idx = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            let len = self.sound_tab.melody.notes().len();
            for (idx, note) in self.sound_tab.melody.notes_mut().iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("#{:02}", idx + 1));
                    ui.label("Hz:");
                    ui.add(egui::DragValue::new(&mut note.freq_hz).range(0..=4000));
                    ui.label("ms:");
                    ui.add(egui::DragValue::new(&mut note.duration_ms).range(20..=2000));

                    if ui.button("▶").clicked() {
                        play_melody_async(vec![*note]);
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

        // 루프 종료 후 예약된 이동·삭제 반영
        if let Some(idx) = move_up_idx {
            self.sound_tab.melody.move_up(idx);
        }
        if let Some(idx) = move_down_idx {
            self.sound_tab.melody.move_down(idx);
        }
        if let Some(idx) = remove_idx {
            self.sound_tab.melody.remove_note(idx);
        }
    }

    // 기기 패널 탭 화면
    fn show_device_tab(&mut self, ui: &mut egui::Ui) {
        let is_busy = self.device_tab.pending_rx.is_some();

        ui.horizontal(|ui| {
            ui.label("프로젝트 경로:");
            ui.text_edit_singleline(&mut self.device_tab.project_dir);
            ui.separator();
            ui.label("CLI 명령:");
            ui.text_edit_singleline(&mut self.device_tab.dotbox_bin);
        });
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            if ui
                .add_enabled(!is_busy, egui::Button::new("환경 점검 (dotbox doctor)"))
                .clicked()
            {
                self.show_bottom_panel = true;
                self.bottom_tab = BottomTab::Output;
                self.launch_cli_command(build_doctor_args());
            }
            if ui
                .add_enabled(!is_busy, egui::Button::new("시뮬레이터 실행 (dotbox run)"))
                .clicked()
            {
                self.show_bottom_panel = true;
                self.bottom_tab = BottomTab::Output;
                self.launch_cli_command(build_run_args());
            }
            if is_busy {
                ui.label("명령 실행 중...");
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
                // 프로젝트 설정에 저장된 오른쪽 포트도 함께 전달
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

    // 기기 패널에서 CLI 명령을 백그라운드로 시작하는 공통 헬퍼
    fn launch_cli_command(&mut self, args: Vec<String>) {
        self.device_tab.output_log = "명령을 실행하는 중입니다...".to_string();
        let rx = run_command_async(
            self.device_tab.dotbox_bin.clone(),
            args,
            PathBuf::from(&self.device_tab.project_dir),
        );
        self.device_tab.pending_rx = Some(rx);
    }
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
// 매 프레임 화면을 갱신하는 egui 루프
impl eframe::App for StudioApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 백그라운드 작업(doctor, CLI) 완료 여부 매 프레임 확인
        self.poll_background_tasks(ui.ctx());

        // 열린 프로젝트가 없으면 전체 화면에 시작 화면만 표시
        if self.opened.is_none() {
            egui::CentralPanel::default().show(ui, |ui| {
                self.show_start_screen(ui); // 시작 화면 렌더링
            });
            return;
        }

        // 1) 상단 도구 모음 패널 (egui 0.37 통합 Panel::top 사용)
        let mut want_close = false; // 프로젝트 닫기 클릭 여부 기록
        egui::Panel::top("top_toolbar").show(ui, |ui| {
            want_close = self.show_top_bar(ui); // 상단 바 렌더링 및 닫기 요청 수신
        });
        // 닫기 버튼이 눌렸으면 프로젝트를 닫고 다음 프레임부터 시작 화면 표시
        if want_close {
            self.close_project();
            return;
        }

        // 2) 하단 패널 ([출력] · [문제 N]) — 표시 옵션이 켜져 있을 때만 렌더링
        if self.show_bottom_panel {
            egui::Panel::bottom("bottom_panel")
                .resizable(true) // 사용자가 경계선을 끌어 높이 조절 가능
                .default_size(150.0) // 기본 패널 높이 150px
                .show(ui, |ui| {
                    self.show_bottom_panel(ui); // 하단 출력·문제 패널 렌더링
                });
        }

        // 3) 좌측 프로젝트 탐색기 패널 (egui 0.37 통합 Panel::left 사용)
        egui::Panel::left("explorer_panel")
            .resizable(true) // 사용자가 경계선을 끌어 너비 조절 가능
            .default_size(180.0) // 기본 패널 너비 180px
            .show(ui, |ui| {
                self.show_explorer_panel(ui); // 좌측 탐색기 렌더링
            });

        // 4) 중앙 작업 공간 패널 (남은 영역 전체에 선택된 탭 표시)
        egui::CentralPanel::default().show(ui, |ui| match self.active_tab {
            ActiveTab::Overview => self.show_overview_tab(ui), // 프로젝트 개요 탭
            ActiveTab::ProjectSettings => self.show_project_settings_tab(ui), // 프로젝트 설정 탭
            ActiveTab::SpriteEditor => {
                self.show_sprite_toolbar(ui); // 화면 모드·초기화 도구 모음
                ui.add_space(6.0);
                self.show_palette(ui); // 색상 팔레트 및 컬러 피커
                ui.add_space(10.0);
                self.show_canvas(ui); // 32×16 또는 16×16 도트 캔버스
                ui.add_space(8.0);
                ui.label("조작: 마우스 왼쪽 클릭/드래그 = 픽셀 찍기 | 오른쪽 클릭/드래그 = 지우기");
            }
            ActiveTab::FontTool => self.show_font_tab(ui), // 글꼴 변환·미리보기 탭
            ActiveTab::SoundTool => self.show_sound_tab(ui), // 효과음·멜로디 편집 탭
            ActiveTab::DevicePanel => self.show_device_tab(ui), // 기기 패널 탭
        });
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
