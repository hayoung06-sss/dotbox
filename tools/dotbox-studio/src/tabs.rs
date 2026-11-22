// 작업 공간 중앙에 탭으로 열 수 있는 항목 종류
// (S2 에셋 형식 합의 후 파일 경로 기반 에셋 탭을 여기에 바로 추가할 수 있음)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabKind {
    Overview,        // 프로젝트 개요
    ProjectSettings, // 프로젝트 설정
    SpriteEditor,    // 스프라이트·화면 편집기
    FontTool,        // 글꼴 변환기
    SoundTool,       // 효과음·멜로디 편집기
    DevicePanel,     // 기기 패널
    ScenePreview,    // 장면(화면) 미리보기 (임시 배치용)
}

impl TabKind {
    // 탭 헤더에 표시할 기본 이름 반환
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "개요",
            Self::ProjectSettings => "프로젝트 설정",
            Self::SpriteEditor => "스프라이트 편집기",
            Self::FontTool => "글꼴 변환기",
            Self::SoundTool => "효과음 편집기",
            Self::DevicePanel => "기기 패널",
            Self::ScenePreview => "장면 미리보기",
        }
    }

    // 탭 닫기(×) 버튼 허용 여부 (개요 탭은 항상 고정 유지)
    pub fn is_closable(self) -> bool {
        self != Self::Overview
    }
}

// 열린 탭 목록과 활성 탭·더티 상태를 관리하는 순수 로직 구조체
#[derive(Debug, Clone)]
pub struct TabManager {
    open_tabs: Vec<TabKind>, // 현재 열려 있는 탭 순서 목록
    active: TabKind,         // 현재 화면에 표시 중인 활성 탭
}

impl Default for TabManager {
    // 프로젝트를 처음 열면 [개요] 탭 하나만 열린 상태로 시작
    fn default() -> Self {
        Self::new()
    }
}

impl TabManager {
    // 기본 상태([개요] 탭 활성화)로 탭 관리자 생성
    pub fn new() -> Self {
        Self {
            open_tabs: vec![TabKind::Overview], // 개요 탭 기본 열림
            active: TabKind::Overview,          // 개요 탭 활성화
        }
    }

    // 탭 열기: 이미 열려 있으면 중복 추가하지 않고 해당 탭으로 포커스 이동
    pub fn open(&mut self, kind: TabKind) {
        if !self.open_tabs.contains(&kind) {
            self.open_tabs.push(kind); // 목록 끝에 새 탭 추가
        }
        self.active = kind; // 해당 탭 활성화
    }

    // 탭 닫기: 고정 탭(개요)이 아니면 목록에서 제거하고 활성 탭 보정
    pub fn close(&mut self, kind: TabKind) {
        if !kind.is_closable() {
            return; // 개요 탭은 닫지 않음
        }
        let Some(pos) = self.open_tabs.iter().position(|&t| t == kind) else {
            return; // 열려 있지 않은 탭이면 무시
        };
        self.open_tabs.remove(pos); // 목록에서 제거

        // 닫힌 탭이 현재 활성 탭이었다면 바로 왼쪽 탭(또는 개요)으로 이동
        if self.active == kind {
            let fallback_idx = pos.saturating_sub(1);
            self.active = self
                .open_tabs
                .get(fallback_idx)
                .copied()
                .unwrap_or(TabKind::Overview);
        }
    }

    // 현재 열려 있는 탭 목록 읽기 전용 반환
    pub fn open_tabs(&self) -> &[TabKind] {
        &self.open_tabs
    }

    // 현재 활성 탭 반환
    pub fn active(&self) -> TabKind {
        self.active
    }

    // 특정 탭을 활성 탭으로 전환 (열려 있지 않으면 함께 열기)
    pub fn set_active(&mut self, kind: TabKind) {
        self.open(kind);
    }

    // 모든 열린 탭 중 저장하지 않은(더티) 탭이 하나라도 있는지 확인
    pub fn has_any_dirty<F>(&self, is_dirty: F) -> bool
    where
        F: Fn(TabKind) -> bool,
    {
        self.open_tabs.iter().any(|&tab| is_dirty(tab))
    }

    // 탭 표시 라벨 생성 (저장되지 않은 변경이 있으면 뒤에 " ●" 표시)
    pub fn tab_label(kind: TabKind, dirty: bool) -> String {
        if dirty {
            format!("{} ●", kind.title()) // 미저장 변경 표시
        } else {
            kind.title().to_string() // 깨끗한 상태
        }
    }
}

// GUI 없이 탭 열기·중복 방지·닫기 후 포커스 보정·더티 라벨을 검증하는 단위 테스트
#[cfg(test)]
mod tests {
    use super::*;

    // 탭을 여러 번 열어도 중복되지 않고 활성 탭만 전환되는지 검증
    #[test]
    fn open_tab_avoids_duplicates_and_activates() {
        let mut mgr = TabManager::new();
        assert_eq!(mgr.open_tabs(), &[TabKind::Overview]);
        assert_eq!(mgr.active(), TabKind::Overview);

        // 효과음 탭 열기
        mgr.open(TabKind::SoundTool);
        assert_eq!(mgr.open_tabs(), &[TabKind::Overview, TabKind::SoundTool]);
        assert_eq!(mgr.active(), TabKind::SoundTool);

        // 같은 탭을 다시 열어도 목록 길이는 늘어나지 않음
        mgr.open(TabKind::SoundTool);
        assert_eq!(mgr.open_tabs().len(), 2);
    }

    // 활성 탭을 닫으면 왼쪽 탭으로 안전하게 포커스가 이동하는지 검증
    #[test]
    fn close_active_tab_falls_back_to_previous() {
        let mut mgr = TabManager::new();
        mgr.open(TabKind::SpriteEditor);
        mgr.open(TabKind::SoundTool);
        assert_eq!(mgr.active(), TabKind::SoundTool);

        // 활성 상태인 효과음 탭 닫기 -> 스프라이트 탭으로 이동
        mgr.close(TabKind::SoundTool);
        assert_eq!(mgr.active(), TabKind::SpriteEditor);
        assert_eq!(mgr.open_tabs(), &[TabKind::Overview, TabKind::SpriteEditor]);

        // 개요 탭은 닫기 요청해도 유지되어야 함
        mgr.close(TabKind::Overview);
        assert!(mgr.open_tabs().contains(&TabKind::Overview));
    }

    // 더티 상태 판별 및 ● 라벨 형식이 명세대로 동작하는지 검증
    #[test]
    fn dirty_label_and_any_dirty_check() {
        let mut mgr = TabManager::new();
        mgr.open(TabKind::SoundTool);

        assert_eq!(
            TabManager::tab_label(TabKind::SoundTool, false),
            "효과음 편집기"
        );
        assert_eq!(
            TabManager::tab_label(TabKind::SoundTool, true),
            "효과음 편집기 ●"
        );

        // 효과음 탭만 더티인 경우 true 반환
        assert!(mgr.has_any_dirty(|t| t == TabKind::SoundTool));
        // 모든 탭이 깨끗하면 false 반환
        assert!(!mgr.has_any_dirty(|_| false));
    }
}
