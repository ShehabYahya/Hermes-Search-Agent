use std::{fs, path::{Path, PathBuf}};

use crate::error::SearchError;

#[derive(Debug)]
pub struct FileSnapshot {
    path: PathBuf,
    content: Option<Vec<u8>>,
}

impl FileSnapshot {
    pub fn capture(path: impl Into<PathBuf>) -> Result<Self, SearchError> {
        let path = path.into();
        let content = if path.exists() { Some(fs::read(&path)?) } else { None };
        Ok(Self { path, content })
    }

    pub fn restore(&self) -> Result<(), SearchError> {
        match &self.content {
            Some(bytes) => {
                if let Some(parent) = self.path.parent() { fs::create_dir_all(parent)?; }
                fs::write(&self.path, bytes)?;
            }
            None => {
                if self.path.exists() { fs::remove_file(&self.path)?; }
            }
        }
        Ok(())
    }

    pub fn path(&self) -> &Path { &self.path }
}
