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

    pub fn existed(&self) -> bool { self.content.is_some() }
    pub fn path(&self) -> &Path { &self.path }
}

#[derive(Debug)]
pub struct DirectorySnapshot {
    path: PathBuf,
    files: Option<Vec<(PathBuf, Vec<u8>)>>,
}

impl DirectorySnapshot {
    pub fn capture(path: impl Into<PathBuf>) -> Result<Self, SearchError> {
        let path = path.into();
        if !path.exists() {
            return Ok(Self { path, files: None });
        }
        if !path.is_dir() {
            return Err(SearchError::Install(format!("snapshot path is not a directory: {}", path.display())));
        }
        let mut files = Vec::new();
        collect_files(&path, &path, &mut files)?;
        Ok(Self { path, files: Some(files) })
    }

    pub fn restore(&self) -> Result<(), SearchError> {
        if self.path.exists() {
            fs::remove_dir_all(&self.path)?;
        }
        let Some(files) = &self.files else { return Ok(()); };
        fs::create_dir_all(&self.path)?;
        for (relative, bytes) in files {
            let target = self.path.join(relative);
            if let Some(parent) = target.parent() { fs::create_dir_all(parent)?; }
            fs::write(target, bytes)?;
        }
        Ok(())
    }

    pub fn path(&self) -> &Path { &self.path }
}

fn collect_files(root: &Path, current: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) -> Result<(), SearchError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::metadata(&path)?;
        if metadata.is_dir() {
            collect_files(root, &path, out)?;
        } else if metadata.is_file() {
            let relative = path.strip_prefix(root).map_err(|e| SearchError::Install(format!("cannot snapshot {}: {e}", path.display())))?;
            out.push((relative.to_path_buf(), fs::read(&path)?));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("value");
        fs::write(&path, b"before").unwrap();
        let snap = FileSnapshot::capture(&path).unwrap();
        fs::write(&path, b"after").unwrap();
        snap.restore().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"before");
    }

    #[test]
    fn removes_file_that_did_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("value");
        let snap = FileSnapshot::capture(&path).unwrap();
        fs::write(&path, b"created").unwrap();
        snap.restore().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn restores_directory_tree() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preset");
        fs::create_dir_all(path.join("skills/a")).unwrap();
        fs::write(path.join("agent.cordis.yml"), b"before").unwrap();
        fs::write(path.join("skills/a/SKILL.md"), b"skill").unwrap();
        let snap = DirectorySnapshot::capture(&path).unwrap();
        fs::remove_dir_all(&path).unwrap();
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("agent.cordis.yml"), b"after").unwrap();
        snap.restore().unwrap();
        assert_eq!(fs::read(path.join("agent.cordis.yml")).unwrap(), b"before");
        assert_eq!(fs::read(path.join("skills/a/SKILL.md")).unwrap(), b"skill");
    }
}
