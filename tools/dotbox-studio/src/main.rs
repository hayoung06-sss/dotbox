mod editor; // 편집 로직 모듈(editor.rs) 연결

use dotbox_core::display::{PANEL_WIDTH, ScreenLayout};
use dotbox_core::hal::Rgb;
use editor::{CanvasEditor, PALETTE};
use eframe::egui;

// 캔버스 한 칸(도트 1개)의 화면 픽셀 크기
const CELL_SIZE: f32 = 24.0;

// GUI 앱 전체 상태 (Default 자동 구현)
#[derive(Default)]
struct StudioApp {
    editor: CanvasEditor, // 도트 데이터 및 편집 로직 담당
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

            ui.separator(); // 구분선

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
                let egui_color = to_color32(color);

                // 팔레트 색상으로 사각형 채우기
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(3), egui_color);

                // 선택된 색은 흰색 굵은 테두리, 나머지는 어두운 회색 테두리
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
                // 현재 칸의 좌상단 좌표와 사각형 영역 계산
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

        // Wide(32×16) 모드일 때 왼쪽·오른쪽 패널 경계(x=16)에 노란 세로선 표시
        if matches!(self.editor.layout(), ScreenLayout::Wide) {
            let split_x = rect.min.x + PANEL_WIDTH as f32 * CELL_SIZE;
            painter.line_segment(
                [
                    egui::pos2(split_x, rect.min.y),
                    egui::pos2(split_x, rect.max.y),
                ],
                egui::Stroke::new(2.5, egui::Color32::from_rgb(255, 200, 40)),
            );
        }
    }
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
            ui.heading("DotBox Studio — 스프라이트·화면 편집기");
            ui.add_space(6.0);

            self.show_toolbar(ui); // 상단 도구 모음
            ui.add_space(6.0);

            self.show_palette(ui); // 색상 팔레트
            ui.add_space(10.0);

            self.show_canvas(ui); // 도트 캔버스
            ui.add_space(8.0);

            // 하단 조작 안내 문구
            ui.label("조작: 마우스 왼쪽 클릭/드래그 = 픽셀 찍기 | 오른쪽 클릭/드래그 = 지우기");
        });
    }
}

// 프로그램 시작점
fn main() -> eframe::Result {
    // 창 초기 크기 설정 (840×560)
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([840.0, 560.0]),
        ..Default::default()
    };

    // 네이티브 윈도우 실행
    eframe::run_native(
        "DotBox Studio",
        options,
        Box::new(|_cc| Ok(Box::new(StudioApp::default()))),
    )
}
