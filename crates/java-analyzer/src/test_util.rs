use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) struct TestDir(tempfile::TempDir);

impl TestDir {
    pub(crate) fn new() -> Self {
        Self(tempfile::TempDir::new().unwrap())
    }

    pub(crate) fn path(&self) -> &Path {
        self.0.path()
    }

    pub(crate) fn write(&self, rel: &str, content: &str) -> PathBuf {
        let full = self.0.path().join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full, content).unwrap();
        full
    }
}
