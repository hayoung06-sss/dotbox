// 편집기 상태를 바꾸고 되돌릴 수 있는 명령 객체 공통 인터페이스
pub trait EditCommand<T> {
    // 명령을 대상 상태에 적용 (최초 실행 및 다시 실행 시 호출)
    fn apply(&self, target: &mut T);
    // 명령을 적용 이전 상태로 되돌림 (실행 취소 시 호출)
    fn undo(&self, target: &mut T);
}

// 최대 보관 가능한 실행 취소 단계 수
const MAX_HISTORY_LEN: usize = 100;

// 파일(편집기) 하나마다 독립적으로 관리하는 실행 취소/다시 실행 이력
pub struct CommandHistory<T> {
    commands: Vec<Box<dyn EditCommand<T>>>, // 실행된 명령 객체 목록
    cursor: usize,                          // 현재 적용된 명령 개수 (0이면 초기 상태)
    saved_cursor: Option<usize>,            // 마지막 저장 시점의 커서 위치 (None이면 저장 불일치)
}

impl<T> Default for CommandHistory<T> {
    // 비어 있는 깨끗한 이력으로 초기화
    fn default() -> Self {
        Self::new()
    }
}

impl<T> CommandHistory<T> {
    // 새 이력 관리자 생성 (초기 상태는 저장됨 상태)
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),  // 명령 목록 비우기
            cursor: 0,             // 현재 위치 0
            saved_cursor: Some(0), // 초기 상태를 저장 지점으로 기록
        }
    }

    // 새 편집 명령을 실행하고 이력에 기록
    pub fn execute(&mut self, target: &mut T, cmd: Box<dyn EditCommand<T>>) {
        // 명령을 실제 상태에 적용
        cmd.apply(target);

        // 실행 취소 상태에서 새 편집을 하면 기존 redo 분기를 모두 폐기
        if self.cursor < self.commands.len() {
            self.commands.truncate(self.cursor);
            // 저장 지점이 잘려 나간 redo 영역에 있었다면 더 이상 원본과 일치하지 않음
            if let Some(saved) = self.saved_cursor
                && saved > self.cursor
            {
                self.saved_cursor = None;
            }
        }

        // 새 명령을 스택 끝에 추가하고 커서 전진
        self.commands.push(cmd);
        self.cursor += 1;

        // 이력이 최대 개수를 넘으면 가장 오래된 명령부터 제거
        if self.commands.len() > MAX_HISTORY_LEN {
            let overflow = self.commands.len() - MAX_HISTORY_LEN;
            self.commands.drain(0..overflow);
            self.cursor -= overflow;
            // 저장 지점 인덱스도 함께 당기되, 밀려났으면 None 처리
            self.saved_cursor = self
                .saved_cursor
                .and_then(|saved| saved.checked_sub(overflow));
        }
    }

    // 가장 최근 명령을 취소 (Ctrl+Z). 취소했으면 true 반환
    pub fn undo(&mut self, target: &mut T) -> bool {
        if self.cursor == 0 {
            return false; // 되돌릴 명령이 없음
        }
        self.cursor -= 1; // 커서를 한 칸 뒤로 이동
        self.commands[self.cursor].undo(target); // 이전 상태로 복원
        true
    }

    // 취소했던 명령을 다시 적용 (Ctrl+Y). 다시 적용했으면 true 반환
    pub fn redo(&mut self, target: &mut T) -> bool {
        if self.cursor >= self.commands.len() {
            return false; // 다시 실행할 명령이 없음
        }
        self.commands[self.cursor].apply(target); // 명령 재적용
        self.cursor += 1; // 커서를 한 칸 앞으로 이동
        true
    }

    // 실행 취소 가능 여부 반환
    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    // 다시 실행 가능 여부 반환
    pub fn can_redo(&self) -> bool {
        self.cursor < self.commands.len()
    }

    // 마지막 저장 시점과 현재 상태가 다르면 true (탭에 ● 표시 기준)
    pub fn is_dirty(&self) -> bool {
        self.saved_cursor != Some(self.cursor)
    }

    // 현재 커서 위치를 마지막 저장 시점으로 기록 (저장 성공 시 ● 표시 해제)
    pub fn mark_saved(&mut self) {
        self.saved_cursor = Some(self.cursor);
    }

    // 이력을 모두 비우고 초기 깨끗한 상태로 되돌림 (프로젝트 전환 등)
    pub fn reset(&mut self) {
        self.commands.clear();
        self.cursor = 0;
        self.saved_cursor = Some(0);
    }
}

// 멜로디 편집기의 음표 목록을 이전/이후 스냅샷으로 교체하는 명령 객체
// 각 음표는 (주파수 Hz, 길이 ms) 튜플로 보관해 모듈 간 결합도를 낮춤
pub struct ReplaceMelodyCommand {
    before: Vec<(u16, u16)>, // 실행 전 음표 목록
    after: Vec<(u16, u16)>,  // 실행 후 음표 목록
}

impl ReplaceMelodyCommand {
    // 변경 전·후 음표 목록으로 명령 객체 생성
    pub fn new(before: Vec<(u16, u16)>, after: Vec<(u16, u16)>) -> Self {
        Self { before, after }
    }
}

impl EditCommand<crate::sound::MelodyEditor> for ReplaceMelodyCommand {
    // 변경 후 음표 목록을 멜로디 편집기에 반영
    fn apply(&self, target: &mut crate::sound::MelodyEditor) {
        target.clear(); // 기존 음표 초기화
        for &(freq_hz, duration_ms) in &self.after {
            target.add_note(freq_hz, duration_ms); // 새 음표 순서대로 복원
        }
    }

    // 변경 전 음표 목록으로 멜로디 편집기 복원
    fn undo(&self, target: &mut crate::sound::MelodyEditor) {
        target.clear(); // 현재 음표 초기화
        for &(freq_hz, duration_ms) in &self.before {
            target.add_note(freq_hz, duration_ms); // 이전 음표 순서대로 복원
        }
    }
}

// 캔버스의 원본 RGB 픽셀 전체를 1차원 벡터로 추출하는 헬퍼
pub fn snapshot_canvas(editor: &mut crate::editor::CanvasEditor) -> Vec<dotbox_core::hal::Rgb> {
    let width = editor.width(); // 현재 캔버스 가로 크기
    let height = editor.height(); // 현재 캔버스 세로 크기
    // 실기기 밝기 미리보기가 켜져 있으면 어두워진 색이 나오므로 잠시 끄고 원본 색 조회
    let prev_preview = editor.led_preview();
    editor.set_led_preview(false);

    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            pixels.push(editor.display_color(x, y)); // (x, y) 원본 RGB 저장
        }
    }

    // 원래의 밝기 미리보기 설정으로 즉시 복원
    editor.set_led_preview(prev_preview);
    pixels
}

// 보관해 둔 1차원 RGB 픽셀 스냅샷을 캔버스에 그대로 복원하는 헬퍼
pub fn restore_canvas(editor: &mut crate::editor::CanvasEditor, pixels: &[dotbox_core::hal::Rgb]) {
    let width = editor.width(); // 현재 캔버스 가로 크기
    let height = editor.height(); // 현재 캔버스 세로 크기
    // 사용자가 선택해 둔 브러시 색이 바뀌지 않도록 미리 백업
    let prev_color = editor.selected_color();

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            // 스냅샷 크기가 현재 캔버스와 맞을 때만 안전하게 픽셀 복원
            if let Some(&color) = pixels.get(idx) {
                editor.set_selected_color(color);
                editor.paint_pixel(x, y);
            }
        }
    }

    // 백업해 둔 사용자 브러시 색 복원
    editor.set_selected_color(prev_color);
}

// 스프라이트 캔버스의 픽셀 전체를 변경 전/후 스냅샷으로 교체하는 명령 객체
pub struct ReplaceCanvasCommand {
    before: Vec<dotbox_core::hal::Rgb>, // 편집 전 캔버스 픽셀
    after: Vec<dotbox_core::hal::Rgb>,  // 편집 후 캔버스 픽셀
}

impl ReplaceCanvasCommand {
    // 변경 전·후 픽셀 스냅샷으로 명령 객체 생성
    pub fn new(before: Vec<dotbox_core::hal::Rgb>, after: Vec<dotbox_core::hal::Rgb>) -> Self {
        Self { before, after }
    }
}

impl EditCommand<crate::editor::CanvasEditor> for ReplaceCanvasCommand {
    // 변경 후 캔버스 상태로 적용
    fn apply(&self, target: &mut crate::editor::CanvasEditor) {
        restore_canvas(target, &self.after);
    }

    // 변경 전 캔버스 상태로 되돌리기
    fn undo(&self, target: &mut crate::editor::CanvasEditor) {
        restore_canvas(target, &self.before);
    }
}

// GUI 없이 명령 실행·실행 취소·다시 실행·더티 상태 추적 로직을 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::MelodyEditor;

    // 단순 정수 값을 더하고 빼는 테스트용 명령 객체
    struct AddValue(i32);

    impl EditCommand<i32> for AddValue {
        fn apply(&self, target: &mut i32) {
            *target += self.0; // 값 더하기
        }
        fn undo(&self, target: &mut i32) {
            *target -= self.0; // 값 빼기
        }
    }

    // 실행 -> 취소 -> 다시 실행 시 상태와 더티 여부가 정확한지 검증
    #[test]
    fn execute_undo_redo_and_dirty_tracking() {
        let mut value = 10;
        let mut history = CommandHistory::new();
        assert!(!history.is_dirty()); // 초기 상태는 깨끗함

        // 명령 2개 연속 실행 (10 -> 15 -> 18)
        history.execute(&mut value, Box::new(AddValue(5)));
        history.execute(&mut value, Box::new(AddValue(3)));
        assert_eq!(value, 18);
        assert!(history.is_dirty()); // 수정되었으므로 더티

        // 1회 실행 취소 (18 -> 15)
        assert!(history.undo(&mut value));
        assert_eq!(value, 15);
        assert!(history.is_dirty());

        // 한 번 더 실행 취소해 초기 상태(10)로 돌아오면 더티 해제
        assert!(history.undo(&mut value));
        assert_eq!(value, 10);
        assert!(!history.is_dirty());

        // 더 이상 취소할 명령이 없으면 false 반환
        assert!(!history.undo(&mut value));

        // 다시 실행 (10 -> 15)
        assert!(history.redo(&mut value));
        assert_eq!(value, 15);
        assert!(history.is_dirty());
    }

    // 실행 취소 후 새 명령을 실행하면 기존 redo 분기가 버려지는지 검증
    #[test]
    fn new_command_clears_redo_branch() {
        let mut value = 0;
        let mut history = CommandHistory::new();

        history.execute(&mut value, Box::new(AddValue(10))); // 0 -> 10
        assert!(history.undo(&mut value)); // 10 -> 0
        assert!(history.can_redo());

        // 새 명령 실행 시 기존 redo(+10)는 폐기되어야 함
        history.execute(&mut value, Box::new(AddValue(7))); // 0 -> 7
        assert_eq!(value, 7);
        assert!(!history.can_redo());
    }

    // 멜로디 편집기 명령 객체가 음표 추가·취소·재실행을 정확히 복원하는지 검증
    #[test]
    fn replace_melody_command_round_trip() {
        let mut melody = MelodyEditor::default();
        melody.clear(); // 빈 상태에서 시작
        let mut history = CommandHistory::new();

        let before = vec![];
        let after = vec![(523, 120), (784, 80)]; // 도 -> 솔
        history.execute(
            &mut melody,
            Box::new(ReplaceMelodyCommand::new(before, after)),
        );
        assert_eq!(melody.notes().len(), 2);
        assert_eq!(melody.notes()[0].freq_hz, 523);

        // 실행 취소하면 다시 비어 있어야 함
        assert!(history.undo(&mut melody));
        assert!(melody.notes().is_empty());

        // 다시 실행하면 음표 2개가 그대로 복원되어야 함
        assert!(history.redo(&mut melody));
        assert_eq!(melody.notes().len(), 2);
        assert_eq!(melody.notes()[1].freq_hz, 784);
    }

    // 스프라이트 캔버스 명령 객체가 픽셀 그리기·실행 취소·다시 실행을 정확히 복원하는지 검증
    #[test]
    fn replace_canvas_command_round_trip() {
        let mut editor = crate::editor::CanvasEditor::new();
        let mut history = CommandHistory::new();
        let red = dotbox_core::hal::Rgb::new(255, 0, 0);

        // 편집 전 스냅샷 추출 후 (2, 3) 위치에 빨간 점 찍기
        let before = snapshot_canvas(&mut editor);
        editor.set_selected_color(red);
        editor.paint_pixel(2, 3);
        let after = snapshot_canvas(&mut editor);

        // 명령 객체로 기록
        history.execute(
            &mut editor,
            Box::new(ReplaceCanvasCommand::new(before, after)),
        );
        assert_eq!(editor.display_color(2, 3), red);
        assert!(history.is_dirty());

        // 실행 취소하면 검은색으로 돌아가고 더티 상태도 해제되어야 함
        assert!(history.undo(&mut editor));
        assert_eq!(editor.display_color(2, 3), dotbox_core::hal::Rgb::BLACK);
        assert!(!history.is_dirty());

        // 다시 실행하면 빨간 점이 복원되어야 함
        assert!(history.redo(&mut editor));
        assert_eq!(editor.display_color(2, 3), red);
        assert!(history.is_dirty());
    }
}
