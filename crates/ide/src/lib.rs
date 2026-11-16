use hir::{FileId, SourceDatabase};
use syntax::{syntax_kind::SyntaxKind, TextRange};

pub struct HoverResult {
    pub content: String,
}

pub fn hover(db: &dyn SourceDatabase, file_id: FileId, offset: u32) -> Option<HoverResult> {
    let syntax_node = db.syntax_tree(file_id);

    // Hover positions refer to the character under the pointer, so prefer the token that
    // starts at `offset` over the one that ends there.
    let token = syntax_node.token_at_offset(offset.into()).right_biased()?;

    match token.kind() {
        SyntaxKind::CLASS_KW => Some(HoverResult {
            content: "Keyword `class` declares a new class".to_string(),
        }),
        SyntaxKind::IDENT => Some(HoverResult {
            content: format!("Identifier `{}`", token.text()),
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: TextRange,
    pub severity: Severity,
    pub message: String,
}

/// Diagnostics for a file: currently its lexical and syntax errors, ordered by position.
pub fn diagnostics(db: &dyn SourceDatabase, file_id: FileId) -> Vec<Diagnostic> {
    db.syntax_errors(file_id)
        .iter()
        .map(|error| Diagnostic {
            range: error.range,
            severity: Severity::Error,
            message: error.message.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hir::RootDatabase;
    use std::sync::Arc;

    fn db_with(text: &str) -> (RootDatabase, FileId) {
        let mut db = RootDatabase::default();
        let file_id = FileId(1);
        db.set_file_text(file_id, Arc::new(text.to_string()));
        (db, file_id)
    }

    #[test]
    fn test_hover_class_keyword() {
        let (db, file_id) = db_with("class Main { }");

        // Offset 1 is inside 'class'
        let hover = hover(&db, file_id, 1).expect("Hover should return result");
        assert_eq!(hover.content, "Keyword `class` declares a new class");
    }

    #[test]
    fn test_hover_token_boundary_prefers_following_token() {
        let (db, file_id) = db_with("class Main { }");

        // Offset 5 is the whitespace right after 'class'.
        assert!(hover(&db, file_id, 5).is_none());
    }

    #[test]
    fn test_hover_ident() {
        let (db, file_id) = db_with("class Main { }");

        // Offset 7 is inside 'Main'
        let hover = hover(&db, file_id, 7).expect("Hover should return result");
        assert_eq!(hover.content, "Identifier `Main`");
    }

    #[test]
    fn no_diagnostics_for_valid_code() {
        let (db, file_id) = db_with("package a;\nimport b.C;\nclass Main { int x; }\n");
        assert_eq!(diagnostics(&db, file_id), vec![]);
    }

    #[test]
    fn syntax_errors_become_diagnostics() {
        let (db, file_id) = db_with("class Main {\n    int x\n}\n");

        // `int x` is missing its semicolon, reported just after `x` (which is at offset 21).
        let diagnostics = diagnostics(&db, file_id);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "expected ';'");
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].range, TextRange::empty(22.into()));
    }

    #[test]
    fn lexical_errors_become_diagnostics() {
        let (db, file_id) = db_with("class A { String s = \"oops; }");
        let messages: Vec<_> = diagnostics(&db, file_id)
            .into_iter()
            .map(|d| d.message)
            .collect();
        assert!(
            messages.contains(&"unterminated string literal".to_string()),
            "{messages:?}"
        );
    }
}
