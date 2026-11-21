mod editor; // 1순위: 스프라이트·화면 편집 로직 모듈
mod font; // 2순위: 글꼴 변환·미리보기 로직 모듈
mod sound; // 3순위: 효과음·멜로디 편집 및 재생 로직 모듈

use dotbox_core::display::{FrameBuffer, PANEL_WIDTH, ScreenLayout};
use dotbox_core::hal::Rgb;
use editor::{CanvasEditor, PALETTE, scale_brightness};
use eframe::egui;
use font::{FontConfig, FontConverter};
use sound::{MelodyEditor, NOTE_PRESETS, play_melody_async};
use std::path::Path;

// 캔버스 한 칸(도트 1개)의 화면 픽셀 크기
const CELL_SIZE: f32 = 24.0;

// 상단 탭 종류
#[derive(Default, PartialEq, Eq)]
enum ActiveTab {
    #[default]
    SpriteEditor, // 1번 탭: 스프라이트·화면 편집기
    FontTool,  // 2번 탭: 글꼴 변환과 미리보기
    SoundTool, // 3번 탭: 효과음·멜로디 편집기
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

// GUI 앱 전체 상태 (Default 자동 구현)
#[derive(Default)]
struct StudioApp {
    active_tab: ActiveTab,    // 현재 열려 있는 탭
    editor: CanvasEditor,     // 1순위: 도트 캔버스 편집기 상태
    font_tab: FontTabState,   // 2순위: 글꼴 변환기 상태
    sound_tab: SoundTabState, // 3순위: 효과음·멜로디 편집기 상태
}

impl StudioApp {
    // 상단 도구 모음 (레이아웃 전환, 전체 지우기, 실기기 밝기 미리보기)
    fn show_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("화면 모드:");
            // 현재 Wide 모드인지 확인
            let is_wide = matches!(self.editor.layout(), ScreenLayout::Wide);

            // Wide(32×16) 전환 버튼
            if ui.selectable_label(is_wide, "Wide (32×16)").clicked() && !is_wide {
                self.editor.set_layout(ScreenLayout::Wide);
            }
            // Mirrored(16×16) 전환 버튼
            if ui.selectable_label(!is_wide, "Mirrored (16×16)").clicked() && is_wide {
                self.editor.set_layout(ScreenLayout::Mirrored);
            }

            ui.separator(); // 세로 구분선

            // 캔버스 전체를 검은색으로 초기화
            if ui.button("전체 지우기").clicked() {
                self.editor.clear();
            }

            ui.separator();

            // 실기기 LED 밝기(15~25%) 미리보기 체크박스
            let mut preview = self.editor.led_preview();
            if ui.checkbox(&mut preview, "실기기 밝기 미리보기").changed() {
                self.editor.set_led_preview(preview);
            }
            // 미리보기가 켜져 있을 때만 밝기 슬라이더 표시
            if preview {
                let mut brightness = self.editor.brightness_percent();
                if ui
                    .add(egui::Slider::new(&mut brightness, 15..=25).suffix("%"))
                    .changed()
                {
                    self.editor.set_brightness_percent(brightness);
                }
            }
        });
    }

    // 색상 팔레트 및 커스텀 색상 선택기 표시
    fn show_palette(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("팔레트:");

            // 기본 프리셋 8색 버튼 순회하며 그리기
            for color in PALETTE {
                // 24×24 클릭 가능한 사각형 영역 확보
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                // 팔레트 색상으로 사각형 채우기
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

                // 클릭 시 해당 색으로 브러시 변경
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

        // 클릭과 드래그를 모두 감지하는 캔버스 영역 확보
        let (rect, response) = ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());

        // 마우스 입력 처리: 좌클릭/드래그 = 찍기, 우클릭/드래그 = 지우기
        if let Some(pointer_pos) = response.interact_pointer_pos() {
            let rel = pointer_pos - rect.min; // 캔버스 좌상단 기준 상대 좌표
            if rel.x >= 0.0 && rel.y >= 0.0 {
                // 화면 좌표를 도트 격자 인덱스(x, y)로 변환
                let x = (rel.x / CELL_SIZE) as usize;
                let y = (rel.y / CELL_SIZE) as usize;

                if x < width && y < height {
                    // 우클릭/드래그: 픽셀 지우기
                    if response.clicked_by(egui::PointerButton::Secondary)
                        || response.dragged_by(egui::PointerButton::Secondary)
                    {
                        self.editor.erase_pixel(x, y);
                    // 좌클릭/드래그: 픽셀 찍기
                    } else if response.clicked_by(egui::PointerButton::Primary)
                        || response.dragged_by(egui::PointerButton::Primary)
                    {
                        self.editor.paint_pixel(x, y);
                    }
                }
            }
        }

        // 캔버스 격자와 도트 그리기 준비
        let painter = ui.painter();
        let grid_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(45));

        // 모든 (x, y) 칸을 순회하며 화면에 렌더링
        for y in 0..height {
            for x in 0..width {
                let cell_min = egui::pos2(
                    rect.min.x + x as f32 * CELL_SIZE,
                    rect.min.y + y as f32 * CELL_SIZE,
                );
                let cell_rect =
                    egui::Rect::from_min_size(cell_min, egui::vec2(CELL_SIZE, CELL_SIZE));

                // 엔진 프레임버퍼 색상을 읽어 칸 채우기 및 격자선 그리기
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

        // Wide(32×16) 모드일 때 패널 경계(x=16) 노란 선 표시
        if matches!(self.editor.layout(), ScreenLayout::Wide) {
            draw_panel_divider(painter, rect);
        }
    }

    // 2순위: 글꼴 변환과 미리보기 탭 화면
    fn show_font_tab(&mut self, ui: &mut egui::Ui) {
        // 1) 폰트 파일 경로 입력 및 불러오기 버튼
        ui.horizontal(|ui| {
            ui.label("TTF/OTF 경로:");
            ui.text_edit_singleline(&mut self.font_tab.font_path);
            if ui.button("폰트 불러오기").clicked() {
                // 파일 로드 결과를 처리해 패닉 없이 상태 메시지로 표시
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
        ui.label(&self.font_tab.status_message); // 현재 상태 또는 에러 메시지 출력
        ui.add_space(6.0);

        // 2) 픽셀 크기(6~16px) 및 이진화 임계값(1~255) 조절 슬라이더
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

        // 3) 미리보기 문구 및 변환할 문자 집합 입력칸
        ui.horizontal(|ui| {
            ui.label("미리보기 문구:");
            ui.text_edit_singleline(&mut self.font_tab.preview_text);
            ui.separator();
            ui.label("문자 집합:");
            ui.text_edit_singleline(&mut self.font_tab.config.charset);
        });
        ui.add_space(8.0);

        // 4) 폰트가 불러와졌으면 엔진 FrameBuffer로 32×16 실기기 미리보기 표시
        if let Some(converter) = &self.font_tab.converter {
            // 문자 집합 변환 후 고유 글자 수 표시
            let glyphs = converter.rasterize_charset(&self.font_tab.config);
            ui.label(format!(
                "변환된 고유 글자 수: {}자 (1비트 비트맵)",
                glyphs.len()
            ));
            ui.add_space(4.0);

            // 미리보기 문자열을 엔진 프레임버퍼에 렌더링
            let fb = converter.render_preview(
                &self.font_tab.preview_text,
                self.font_tab.config.pixel_size,
                self.font_tab.config.threshold,
                self.editor.selected_color(),
            );
            // 렌더링된 프레임버퍼를 32×16 도트 매트릭스 UI로 출력
            draw_framebuffer_preview(
                ui,
                &fb,
                self.editor.led_preview(),
                self.editor.brightness_percent(),
            );
        }
    }

    // 3순위: 효과음·멜로디 편집기 탭 화면
    fn show_sound_tab(&mut self, ui: &mut egui::Ui) {
        // 1) 상단 재생·초기화 버튼 및 전체 길이 요약
        ui.horizontal(|ui| {
            // 현재 시퀀스 전체를 PC 스피커로 비동기 재생
            if ui.button("▶ 전체 재생").clicked() {
                play_melody_async(self.sound_tab.melody.notes().to_vec());
            }
            // 모든 음표 삭제
            if ui.button("전체 지우기").clicked() {
                self.sound_tab.melody.clear();
            }
            ui.separator();
            // 현재 음표 개수와 총 재생 시간(ms) 표시
            ui.label(format!(
                "음표: {}개 | 총 길이: {} ms",
                self.sound_tab.melody.notes().len(),
                self.sound_tab.melody.total_duration_ms()
            ));
        });
        ui.add_space(6.0);

        // 2) 빠른 음계 프리셋 버튼 (클릭 즉시 현재 길이로 시퀀스 끝에 추가)
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

        // 3) 사용자가 원하는 주파수(Hz)·길이(ms)를 직접 입력해 추가
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

        // 4) 순회 중 벡터 수정 충돌을 피하기 위해 이동·삭제할 인덱스만 임시 기록
        let mut remove_idx = None;
        let mut move_up_idx = None;
        let mut move_down_idx = None;

        // 음표 시퀀스 목록 (세로 스크롤 지원)
        egui::ScrollArea::vertical().show(ui, |ui| {
            let len = self.sound_tab.melody.notes().len(); // 전체 음표 수
            for (idx, note) in self.sound_tab.melody.notes_mut().iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("#{:02}", idx + 1)); // 음표 순서 번호
                    ui.label("Hz:");
                    // 개별 음표 주파수 수정
                    ui.add(egui::DragValue::new(&mut note.freq_hz).range(0..=4000));
                    ui.label("ms:");
                    // 개별 음표 길이 수정
                    ui.add(egui::DragValue::new(&mut note.duration_ms).range(20..=2000));

                    // 이 음표 1개만 스피커로 미리 듣기
                    if ui.button("▶").clicked() {
                        play_melody_async(vec![*note]);
                    }
                    // 위로 이동 버튼 (첫 번째 항목은 비활성화)
                    if ui.add_enabled(idx > 0, egui::Button::new("▲")).clicked() {
                        move_up_idx = Some(idx);
                    }
                    // 아래로 이동 버튼 (마지막 항목은 비활성화)
                    if ui
                        .add_enabled(idx + 1 < len, egui::Button::new("▼"))
                        .clicked()
                    {
                        move_down_idx = Some(idx);
                    }
                    // 개별 음표 삭제 버튼
                    if ui.button("삭제").clicked() {
                        remove_idx = Some(idx);
                    }
                });
            }
        });

        // 루프 종료 후 기록해 둔 이동·삭제 작업 실제 반영
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
}

// 읽기 전용 FrameBuffer를 32×16 도트 매트릭스로 화면에 그리는 헬퍼 함수
fn draw_framebuffer_preview(
    ui: &mut egui::Ui,
    fb: &FrameBuffer,
    led_preview: bool,
    brightness_percent: u8,
) {
    let width = usize::from(fb.width()); // 가로 도트 수 (32)
    let height = usize::from(fb.height()); // 세로 도트 수 (16)
    let size = egui::vec2(width as f32 * CELL_SIZE, height as f32 * CELL_SIZE);
    // 읽기 전용이므로 hover 감지 영역만 확보
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());

    let painter = ui.painter();
    let grid_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(45));

    // 모든 칸을 순회하며 프레임버퍼 픽셀 그리기
    for y in 0..height {
        for x in 0..width {
            let cell_min = egui::pos2(
                rect.min.x + x as f32 * CELL_SIZE,
                rect.min.y + y as f32 * CELL_SIZE,
            );
            let cell_rect = egui::Rect::from_min_size(cell_min, egui::vec2(CELL_SIZE, CELL_SIZE));

            // 엔진 프레임버퍼에서 픽셀 색상 조회
            let raw = fb.pixel(x as i32, y as i32).unwrap_or(Rgb::BLACK);
            // 실기기 밝기 미리보기 옵션이 켜져 있으면 밝기 비율 적용
            let display = if led_preview {
                scale_brightness(raw, brightness_percent)
            } else {
                raw
            };

            // 도트 칸 채우기 및 격자선 그리기
            painter.rect_filled(cell_rect, egui::CornerRadius::ZERO, to_color32(display));
            painter.rect_stroke(
                cell_rect,
                egui::CornerRadius::ZERO,
                grid_stroke,
                egui::StrokeKind::Inside,
            );
        }
    }

    // 왼쪽·오른쪽 패널 경계선(x=16) 표시
    draw_panel_divider(painter, rect);
}

// 32×16 화면 중앙(x=16)에 왼쪽·오른쪽 패널 경계선 그리기
fn draw_panel_divider(painter: &egui::Painter, rect: egui::Rect) {
    let split_x = rect.min.x + f32::from(PANEL_WIDTH) * CELL_SIZE; // x=16 픽셀 위치 계산
    painter.line_segment(
        [
            egui::pos2(split_x, rect.min.y), // 상단 끝점
            egui::pos2(split_x, rect.max.y), // 하단 끝점
        ],
        egui::Stroke::new(2.5, egui::Color32::from_rgb(255, 200, 40)), // 노란색 구분선
    );
}

// 엔진 Rgb 타입을 egui 색상 타입(Color32)으로 변환
fn to_color32(color: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(color.r, color.g, color.b)
}

// 매 프레임 화면을 갱신하는 egui 루프
impl eframe::App for StudioApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 창 전체를 채우는 중앙 패널 배치
        egui::CentralPanel::default().show(ui, |ui| {
            // 상단 제목 및 탭 전환 바
            ui.horizontal(|ui| {
                ui.heading("DotBox Studio");
                ui.separator();
                ui.selectable_value(
                    &mut self.active_tab,
                    ActiveTab::SpriteEditor,
                    "1. 스프라이트·화면 편집기",
                );
                ui.selectable_value(
                    &mut self.active_tab,
                    ActiveTab::FontTool,
                    "2. 글꼴 변환·미리보기",
                );
                ui.selectable_value(
                    &mut self.active_tab,
                    ActiveTab::SoundTool,
                    "3. 효과음·멜로디 편집기",
                );
            });
            ui.separator();
            ui.add_space(6.0);

            // 현재 선택된 탭에 맞는 화면 렌더링
            match self.active_tab {
                ActiveTab::SpriteEditor => {
                    self.show_toolbar(ui); // 상단 도구 모음
                    ui.add_space(6.0);
                    self.show_palette(ui); // 색상 팔레트
                    ui.add_space(10.0);
                    self.show_canvas(ui); // 도트 캔버스
                    ui.add_space(8.0);
                    ui.label(
                        "조작: 마우스 왼쪽 클릭/드래그 = 픽셀 찍기 | 오른쪽 클릭/드래그 = 지우기",
                    );
                }
                ActiveTab::FontTool => {
                    self.show_font_tab(ui); // 글꼴 변환 및 미리보기 화면
                }
                ActiveTab::SoundTool => {
                    self.show_sound_tab(ui); // 효과음·멜로디 편집 화면
                }
            }
        });
    }
}

// 프로그램 시작점
fn main() -> eframe::Result {
    // 창 초기 크기 설정 (860×600)
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([860.0, 600.0]),
        ..Default::default()
    };

    // 네이티브 윈도우 실행
    eframe::run_native(
        "DotBox Studio",
        options,
        Box::new(|_cc| Ok(Box::new(StudioApp::default()))),
    )
}
