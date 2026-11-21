// 엔진의 핵심 프레임버퍼와 레이아웃(와이드 등) 타입 불러오기
use dotbox_core::display::{FrameBuffer, ScreenLayout};
// GUI 렌더링을 담당하는 egui 프레임워크 불러오기
use eframe::egui;

// 앱 전체의 데이터와 상태를 담는 구조체
struct StudioApp {
    framebuffer: FrameBuffer, // 게임기와 1:1로 일치하는 엔진 프레임버퍼 (32×16)
}

// 앱 시작 시 초기 상태를 정의하는 트레잇 구현
impl Default for StudioApp {
    fn default() -> Self {
        Self {
            // 패널 2개를 이어붙인 32×16 기본 화면으로 버퍼 초기화
            framebuffer: FrameBuffer::new(ScreenLayout::Wide),
        }
    }
}

// 매 프레임 화면을 그리는 GUI 루프
impl eframe::App for StudioApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 창 전체를 채우는 기본 중앙 패널 시작
        egui::CentralPanel::default().show(ui, |ui| {
            // 상단 큰 글씨로 프로그램 제목 출력
            ui.heading("DotBox Studio");

            // 프레임버퍼에서 현재 너비·높이를 읽어와 텍스트로 표시
            ui.label(format!(
                "캔버스: {}×{} (Wide)",
                self.framebuffer.width(),
                self.framebuffer.height()
            ));
        });
    }
}

// 프로그램 시작 지점
fn main() -> eframe::Result {
    // 윈도우 창 설정 (가로 800px, 세로 500px 크기로 지정)
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    // 네이티브 창을 띄우고 앱 인스턴스(StudioApp) 이벤트 루프 실행
    eframe::run_native(
        "DotBox Studio",
        options,
        Box::new(|_cc| Ok(Box::new(StudioApp::default()))),
    )
}

// 화면을 띄우지 않고 백그라운드에서 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;
    // 엔진에 정의된 화면 표준 상수(32, 16) 가져오기
    use dotbox_core::display::{SCREEN_HEIGHT, SCREEN_WIDTH};

    #[test]
    fn 기본_프레임버퍼_크기_검증() {
        // 기본 앱 인스턴스 생성
        let app = StudioApp::default();
        // 생성된 버퍼 크기가 엔진 표준 32x16과 같은지 확인
        assert_eq!(app.framebuffer.width(), SCREEN_WIDTH);
        assert_eq!(app.framebuffer.height(), SCREEN_HEIGHT);
    }
}