use std::sync::Arc;

use syntax::{Parse, SyntaxError, SyntaxNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

#[salsa::query_group(SourceDatabaseStorage)]
pub trait SourceDatabase: salsa::Database {
    #[salsa::input]
    fn file_text(&self, file_id: FileId) -> Arc<String>;

    /// The parse result of a file. Cheap to clone; recomputed only when the file text changes.
    fn parse(&self, file_id: FileId) -> Parse;

    /// Root of the syntax tree of a file.
    fn syntax_tree(&self, file_id: FileId) -> SyntaxNode;

    /// Lexical and syntax errors of a file, ordered by position.
    fn syntax_errors(&self, file_id: FileId) -> Arc<Vec<SyntaxError>>;
}

fn parse(db: &dyn SourceDatabase, file_id: FileId) -> Parse {
    syntax::parse(&db.file_text(file_id))
}

fn syntax_tree(db: &dyn SourceDatabase, file_id: FileId) -> SyntaxNode {
    db.parse(file_id).syntax_node()
}

fn syntax_errors(db: &dyn SourceDatabase, file_id: FileId) -> Arc<Vec<SyntaxError>> {
    Arc::new(db.parse(file_id).errors().to_vec())
}

#[salsa::database(SourceDatabaseStorage)]
#[derive(Default)]
pub struct RootDatabase {
    storage: salsa::Storage<Self>,
}

impl salsa::Database for RootDatabase {}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::syntax_kind::SyntaxKind;

    fn db_with(text: &str) -> (RootDatabase, FileId) {
        let mut db = RootDatabase::default();
        let file_id = FileId(1);
        db.set_file_text(file_id, Arc::new(text.to_string()));
        (db, file_id)
    }

    #[test]
    fn syntax_tree_of_a_file() {
        let (db, file_id) = db_with("class Main {}");
        assert_eq!(db.syntax_tree(file_id).kind(), SyntaxKind::SOURCE_FILE);
        assert!(db.syntax_errors(file_id).is_empty());
    }

    #[test]
    fn syntax_errors_of_a_broken_file() {
        let (db, file_id) = db_with("class Main {");
        let errors = db.syntax_errors(file_id);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "expected '}'");
    }

    #[test]
    fn results_follow_text_changes() {
        let (mut db, file_id) = db_with("class Main {");
        assert_eq!(db.syntax_errors(file_id).len(), 1);

        db.set_file_text(file_id, Arc::new("class Main {}".to_string()));
        assert!(db.syntax_errors(file_id).is_empty());
    }
}

