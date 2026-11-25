use hir::{FileId, SourceDatabase, WorkspaceLayout};
use syntax::TextRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedInformation {
    pub file_id: FileId,
    pub range: TextRange,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub range: TextRange,
    pub severity: Severity,
    pub message: String,
    pub related: Vec<RelatedInformation>,
    pub help: Option<String>,
}

/// Classifies a raw lexer/parser/language-level message into `(code, message, help)`.
///
/// Primary messages use familiar `javac`-like phrasing without internal `SyntaxKind` names
/// or trailing periods on single-sentence messages.
pub fn classify_syntax_error(raw_message: &str) -> (&'static str, String, Option<String>) {
    let trimmed = raw_message.trim();

    if let Some(prefix) = trimmed.strip_suffix("; enable preview") {
        let msg = prefix.replace(" in java ", " in Java ");
        return (
            "syntax/preview-required",
            msg,
            Some("pass `--enable-preview` to enable preview language features".to_string()),
        );
    }

    if trimmed.contains("with preview enabled") {
        return (
            "syntax/preview-required",
            trimmed.to_string(),
            Some("upgrade the project language level and pass `--enable-preview`".to_string()),
        );
    }

    if trimmed.contains("require language level") {
        let help = trimmed
            .rsplit_once(' ')
            .and_then(|(_, ver)| ver.parse::<u16>().ok())
            .map_or_else(
                || "upgrade the project language level".to_string(),
                |ver| format!("set the project language level to Java {ver} or newer"),
            );
        return ("syntax/feature-not-supported", trimmed.to_string(), Some(help));
    }

    if trimmed.contains("were removed in language level") {
        return (
            "syntax/feature-not-supported",
            trimmed.to_string(),
            Some("replace the removed language feature".to_string()),
        );
    }

    if trimmed.contains("are not available at language level") {
        return (
            "syntax/feature-not-supported",
            trimmed.to_string(),
            Some("adjust the project language level or replace the unsupported construct".to_string()),
        );
    }

    match trimmed {
        "unterminated block comment" => (
            "syntax/unclosed-comment",
            "unclosed comment".to_string(),
            Some("close the block comment with `*/`".to_string()),
        ),
        "unterminated string literal" => (
            "syntax/unclosed-literal",
            "unclosed string literal".to_string(),
            Some("add a closing `\"` before the end of the line".to_string()),
        ),
        "unterminated character literal" => (
            "syntax/unclosed-literal",
            "unclosed character literal".to_string(),
            Some("add a closing `'` on the same line".to_string()),
        ),
        "unterminated text block" => (
            "syntax/unclosed-literal",
            "unclosed text block".to_string(),
            Some("add a closing `\"\"\"` delimiter".to_string()),
        ),
        "expected line terminator after '\"\"\"'" => (
            "syntax/unclosed-literal",
            "illegal text block open delimiter, missing line terminator".to_string(),
            Some("start text block content on a new line after `\"\"\"`".to_string()),
        ),
        "expected an expression" => (
            "syntax/illegal-start",
            "illegal start of expression".to_string(),
            Some("provide a valid expression here".to_string()),
        ),
        "expected a type" => (
            "syntax/illegal-start",
            "illegal start of type".to_string(),
            Some("provide a valid type here".to_string()),
        ),
        "expected a statement" => (
            "syntax/illegal-start",
            "illegal start of statement".to_string(),
            Some("provide a valid statement or block here".to_string()),
        ),
        "unmatched '}'" => (
            "syntax/illegal-start",
            "unmatched '}'".to_string(),
            Some("remove the extra `}` or add a matching `{`".to_string()),
        ),
        "unexpected token" | "unexpected character" => {
            ("syntax/illegal-start", trimmed.to_string(), None)
        }
        _ if trimmed.starts_with("unexpected '") => (
            "syntax/illegal-start",
            trimmed.to_string(),
            Some("remove or move this unexpected token".to_string()),
        ),
        "expected ';'" => (
            "syntax/expected-token",
            "';' expected".to_string(),
            Some("add a semicolon at the end of the statement".to_string()),
        ),
        "expected ')'" => (
            "syntax/expected-token",
            "')' expected".to_string(),
            Some("add a closing `)` to match the opening `(`".to_string()),
        ),
        "expected '('" => (
            "syntax/expected-token",
            "'(' expected".to_string(),
            Some("add an opening `(`".to_string()),
        ),
        "expected '}'" => (
            "syntax/expected-token",
            "'}' expected".to_string(),
            Some("add a closing `}` to close the block or declaration".to_string()),
        ),
        "expected '{'" => (
            "syntax/expected-token",
            "'{' expected".to_string(),
            Some("add an opening `{`".to_string()),
        ),
        "expected ']'" => (
            "syntax/expected-token",
            "']' expected".to_string(),
            Some("add a closing `]` to match the opening `[`".to_string()),
        ),
        "expected '>'" => (
            "syntax/expected-token",
            "'>' expected".to_string(),
            Some("add a closing `>` for the type argument list".to_string()),
        ),
        "expected a name" => ("syntax/expected-token", "identifier expected".to_string(), None),
        "expected an annotation name" => (
            "syntax/expected-token",
            "annotation type expected".to_string(),
            None,
        ),
        "expected a parameter" => (
            "syntax/expected-token",
            "parameter declaration expected".to_string(),
            None,
        ),
        "expected a class, interface, enum or record declaration" => (
            "syntax/expected-token",
            "class, interface, enum, or record expected".to_string(),
            None,
        ),
        "expected a member declaration" => (
            "syntax/expected-token",
            "member declaration expected".to_string(),
            None,
        ),
        "expected a module directive" => (
            "syntax/expected-token",
            "module directive expected".to_string(),
            None,
        ),
        _ => {
            if let Some(rest) = trimmed.strip_prefix("expected ")
                && rest.starts_with('\'')
            {
                return ("syntax/expected-token", format!("{rest} expected"), None);
            }
            let sanitized = trimmed.strip_suffix('.').unwrap_or(trimmed).trim();
            let msg = if sanitized.is_empty() {
                "syntax error".to_string()
            } else {
                sanitized.to_string()
            };
            ("syntax/parse-error", msg, None)
        }
    }
}

/// Diagnostics for a file: lexical/syntax errors plus optional package-vs-layout mismatch errors.
pub fn diagnostics_with_layout(
    db: &dyn SourceDatabase,
    file_id: FileId,
    layout: Option<(&WorkspaceLayout, &str)>,
) -> Vec<Diagnostic> {
    let mut diags: Vec<Diagnostic> = db
        .syntax_errors(file_id)
        .iter()
        .map(|error| {
            let (code, message, help) = classify_syntax_error(&error.message);
            Diagnostic {
                code,
                range: error.range,
                severity: Severity::Error,
                message,
                related: Vec::new(),
                help,
            }
        })
        .collect();

    if let Some((layout, rel_path)) = layout {
        let syntax_tree = db.syntax_tree(file_id);
        if let Some(pkg_diag) = layout.check_file_package(file_id, rel_path, &syntax_tree) {
            let help = if pkg_diag.expected.is_empty() {
                "remove the `package` declaration or move the file into a matching directory"
                    .to_string()
            } else {
                format!(
                    "change the package declaration to `package {};` or move the file",
                    pkg_diag.expected
                )
            };
            diags.push(Diagnostic {
                code: "workspace/package-mismatch",
                range: pkg_diag.range,
                severity: Severity::Error,
                message: pkg_diag.message(),
                related: Vec::new(),
                help: Some(help),
            });
        }
    }

    diags
}

/// Diagnostics for a file: currently its lexical and syntax errors, ordered by position.
pub fn diagnostics(db: &dyn SourceDatabase, file_id: FileId) -> Vec<Diagnostic> {
    diagnostics_with_layout(db, file_id, None)
}
