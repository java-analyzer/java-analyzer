use std::sync::Arc;
use syntax::{SyntaxNode, parser::parse as parse_syntax};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

#[salsa::query_group(SourceDatabaseStorage)]
pub trait SourceDatabase: salsa::Database {
    #[salsa::input]
    fn file_text(&self, file_id: FileId) -> Arc<String>;

    fn parse(&self, file_id: FileId) -> SyntaxNode;
}

fn parse(db: &dyn SourceDatabase, file_id: FileId) -> SyntaxNode {
    let text = db.file_text(file_id);
    let green = parse_syntax(&text);
    SyntaxNode::new_root(green)
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

    #[test]
    fn test_database_parse() {
        let mut db = RootDatabase::default();
        let file_id = FileId(1);
        let text = Arc::new("class Main {}".to_string());

        db.set_file_text(file_id, text);

        let syntax_node = db.parse(file_id);
        assert_eq!(syntax_node.kind(), SyntaxKind::SOURCE_FILE);
    }
}
