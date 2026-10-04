use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

#[salsa::query_group(SourceDatabaseStorage)]
pub trait SourceDatabase: salsa::Database {
    #[salsa::input]
    fn file_text(&self, file_id: FileId) -> Arc<String>;
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

    #[test]
    fn test_database_basic() {
        let mut db = RootDatabase::default();
        let file_id = FileId(1);
        let text = Arc::new("class Main {}".to_string());

        db.set_file_text(file_id, text.clone());

        assert_eq!(db.file_text(file_id), text);
    }
}
