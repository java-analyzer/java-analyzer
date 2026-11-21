use std::sync::Arc;

use crate::{FileId, LanguageLevel, RootDatabase, TextRange};

pub fn db_with(text: &str) -> (RootDatabase, FileId) {
    let mut db = RootDatabase::default();
    let file_id = FileId(1);
    db.set_file_text(file_id, Arc::new(text.to_string()));
    db.set_file_language_level(file_id, LanguageLevel::latest());
    db.set_workspace_files(Arc::new(vec![file_id]));
    (db, file_id)
}

pub fn db_with_files(files: &[(u32, &str)]) -> RootDatabase {
    let mut db = RootDatabase::default();
    let mut ids = Vec::with_capacity(files.len());
    for &(id, text) in files {
        let file_id = FileId(id);
        db.set_file_text(file_id, Arc::new(text.to_string()));
        db.set_file_language_level(file_id, LanguageLevel::latest());
        ids.push(file_id);
    }
    db.set_workspace_files(Arc::new(ids));
    db
}

pub fn cursor(text: &str, marker: &str) -> u32 {
    let idx = text
        .find(marker)
        .unwrap_or_else(|| panic!("marker not found: {marker:?}"));
    (idx + marker.len()) as u32
}

pub fn slice(text: &str, range: TextRange) -> &str {
    &text[usize::from(range.start())..usize::from(range.end())]
}
