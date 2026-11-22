use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Error,
    Warning,
    Note,
    Help,
}

impl DiagnosticLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warning => "WARN",
            Self::Note => "NOTE",
            Self::Help => "HELP",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoDiagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Deserialize)]
struct RawCargoMessage {
    reason: Option<String>,
    message: Option<RawCompilerMessage>,
}

#[derive(Deserialize)]
struct RawCompilerMessage {
    message: Option<String>,
    level: Option<String>,
    spans: Option<Vec<RawSpan>>,
}

#[derive(Deserialize)]
struct RawSpan {
    file_name: Option<String>,
    line_start: Option<usize>,
    column_start: Option<usize>,
    is_primary: Option<bool>,
}

pub fn parse_cargo_json_line(line: &str) -> Option<CargoDiagnostic> {
    let trimmed = line.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return None;
    }

    let parsed: RawCargoMessage = serde_json::from_str(trimmed).ok()?;
    if parsed.reason.as_deref() != Some("compiler-message") {
        return None;
    }

    let msg = parsed.message?;
    let level_str = msg.level.as_deref().unwrap_or("error");
    let level = match level_str {
        "error" => DiagnosticLevel::Error,
        "warning" => DiagnosticLevel::Warning,
        "note" => DiagnosticLevel::Note,
        "help" => DiagnosticLevel::Help,
        _ => DiagnosticLevel::Error,
    };

    let mut file = None;
    let mut line = None;
    let mut column = None;

    if let Some(spans) = msg.spans {
        let primary = spans
            .iter()
            .find(|s| s.is_primary == Some(true))
            .or_else(|| spans.first());
        if let Some(span) = primary {
            file = span.file_name.clone();
            line = span.line_start;
            column = span.column_start;
        }
    }

    Some(CargoDiagnostic {
        level,
        message: msg.message.unwrap_or_default(),
        file,
        line,
        column,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_cargo_error_json() {
        let json = r#"{"reason":"compiler-message","package_id":"my-game 0.1.0","message":{"rendered":"error[E0425]...","message":"cannot find value `x` in this scope","level":"error","spans":[{"file_name":"src/main.rs","byte_start":100,"byte_end":101,"line_start":12,"line_end":12,"column_start":5,"column_end":6,"is_primary":true}]}}"#;
        let diag = parse_cargo_json_line(json).expect("should parse");
        assert_eq!(diag.level, DiagnosticLevel::Error);
        assert_eq!(diag.message, "cannot find value `x` in this scope");
        assert_eq!(diag.file.as_deref(), Some("src/main.rs"));
        assert_eq!(diag.line, Some(12));
        assert_eq!(diag.column, Some(5));
    }

    #[test]
    fn non_json_line_ignored() {
        assert!(parse_cargo_json_line("Compiling my-game v0.1.0").is_none());
        assert!(parse_cargo_json_line("").is_none());
    }
}
