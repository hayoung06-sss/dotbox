//! ESP-IDF 빌드를 깨뜨리는 경로 문제 검사.

use std::path::Path;

/// 게임 프로젝트 경로 길이 기준(잠정값). 넘으면 경고한다.
pub const PROJECT_PATH_MAX_CHARS: usize = 80;
/// 펌웨어 target 디렉토리 길이 기준(잠정값). 넘으면 경고한다.
pub const TARGET_DIR_MAX_CHARS: usize = 24;

/// 경로 문제.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathIssue {
    /// ASCII가 아닌 문자가 있다(예: 한글 사용자 이름).
    NonAscii,
    /// 공백이 있다.
    Spaces,
    /// OneDrive 동기화 폴더 아래에 있다.
    OneDrive,
    /// 기준보다 길다.
    TooLong { chars: usize, max_chars: usize },
}

impl PathIssue {
    /// 빌드를 반드시 막아야 하는 문제인지. 길이는 경고만 한다.
    pub fn is_fatal(&self) -> bool {
        !matches!(self, Self::TooLong { .. })
    }

    /// 사람이 읽을 설명.
    pub fn message(&self) -> String {
        match self {
            Self::NonAscii => "경로에 ASCII가 아닌 문자(한글 등)가 있습니다".to_owned(),
            Self::Spaces => "경로에 공백이 있습니다".to_owned(),
            Self::OneDrive => "경로가 OneDrive 폴더 아래에 있습니다".to_owned(),
            Self::TooLong { chars, max_chars } => {
                format!("경로가 깁니다({chars}자, 기준 {max_chars}자)")
            }
        }
    }
}

/// `path`의 문제를 모두 찾는다. 경로는 호출하는 쪽이 절대 경로로 만들어 넘긴다.
pub fn path_issues(path: &Path, max_chars: usize) -> Vec<PathIssue> {
    let text = path.to_string_lossy();
    let mut issues = Vec::new();
    if !text.is_ascii() {
        issues.push(PathIssue::NonAscii);
    }
    if text.contains(' ') {
        issues.push(PathIssue::Spaces);
    }
    if is_under_onedrive(path) {
        issues.push(PathIssue::OneDrive);
    }
    let chars = text.chars().count();
    if chars > max_chars {
        issues.push(PathIssue::TooLong { chars, max_chars });
    }
    issues
}

/// 경로 구성 요소 중 `OneDrive`로 시작하는 폴더가 있는지(대소문자 무시).
/// 개인용 `OneDrive`와 회사용 `OneDrive - 회사명`을 모두 잡는다.
pub fn is_under_onedrive(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_string_lossy()
            .to_ascii_lowercase()
            .starts_with("onedrive")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_short_path_has_no_issues() {
        assert!(path_issues(Path::new("E:/DotBoxEngine"), 80).is_empty());
    }

    #[test]
    fn detects_korean_user_name() {
        let issues = path_issues(Path::new("C:/Users/홍길동/games/snake"), 80);
        assert_eq!(issues, vec![PathIssue::NonAscii]);
    }

    #[test]
    fn detects_spaces() {
        let issues = path_issues(Path::new("C:/My Games/snake"), 80);
        assert_eq!(issues, vec![PathIssue::Spaces]);
    }

    #[test]
    fn detects_personal_and_business_onedrive() {
        assert!(is_under_onedrive(Path::new("C:/Users/a/OneDrive/game")));
        assert!(is_under_onedrive(Path::new(
            "C:/Users/a/OneDrive - School/game"
        )));
        assert!(is_under_onedrive(Path::new("C:/Users/a/onedrive/game")));
        assert!(!is_under_onedrive(Path::new(
            "C:/Users/a/MyOneDriveBackup/game"
        )));
    }

    #[test]
    fn reports_all_issues_together() {
        let issues = path_issues(Path::new("C:/Users/홍길동/OneDrive - 학교/게임 폴더"), 10);
        assert!(issues.contains(&PathIssue::NonAscii));
        assert!(issues.contains(&PathIssue::Spaces));
        assert!(issues.contains(&PathIssue::OneDrive));
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, PathIssue::TooLong { .. }))
        );
    }

    #[test]
    fn length_counts_characters_not_bytes() {
        let path = Path::new("C:/aaaa"); // 7자
        assert!(path_issues(path, 7).is_empty());
        assert_eq!(
            path_issues(path, 6),
            vec![PathIssue::TooLong {
                chars: 7,
                max_chars: 6
            }]
        );
    }

    #[test]
    fn only_length_is_non_fatal() {
        assert!(PathIssue::NonAscii.is_fatal());
        assert!(PathIssue::Spaces.is_fatal());
        assert!(PathIssue::OneDrive.is_fatal());
        assert!(
            !PathIssue::TooLong {
                chars: 1,
                max_chars: 0
            }
            .is_fatal()
        );
    }
}
