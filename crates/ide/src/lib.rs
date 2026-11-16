use hir::{FileId, SourceDatabase};
use syntax::syntax_kind::SyntaxKind;

pub struct HoverResult {
    pub content: String,
}

pub fn hover(db: &dyn SourceDatabase, file_id: FileId, offset: u32) -> Option<HoverResult> {
    let syntax_node = db.parse(file_id);

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

#[cfg(test)]
mod tests {
    use super::*;
    use hir::RootDatabase;
    use std::sync::Arc;

    #[test]
    fn test_hover_class_keyword() {
        let mut db = RootDatabase::default();
        let file_id = FileId(1);
        db.set_file_text(file_id, Arc::new("class Main { }".to_string()));

        // Offset 1 is inside 'class'
        let hover = hover(&db, file_id, 1).expect("Hover should return result");
        assert_eq!(hover.content, "Keyword `class` declares a new class");
    }

    #[test]
    fn test_hover_token_boundary_prefers_following_token() {
        let mut db = RootDatabase::default();
        let file_id = FileId(1);
        db.set_file_text(file_id, Arc::new("class Main { }".to_string()));

        // Offset 5 is the whitespace right after 'class'.
        assert!(hover(&db, file_id, 5).is_none());
    }

    #[test]
    fn test_hover_ident() {
        let mut db = RootDatabase::default();
        let file_id = FileId(1);
        db.set_file_text(file_id, Arc::new("class Main { }".to_string()));

        // Offset 7 is inside 'Main'
        let hover = hover(&db, file_id, 7).expect("Hover should return result");
        assert_eq!(hover.content, "Identifier `Main`");
    }
}
