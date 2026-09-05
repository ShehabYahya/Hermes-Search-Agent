use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use uuid::Uuid;

use crate::error::SearchError;

pub fn ensure_parent(path: &Path) -> Result<(), SearchError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8], mode: Option<u32>) -> Result<(), SearchError> {
    ensure_parent(path)?;
    let parent = path.parent().ok_or_else(|| SearchError::Install(format!("path has no parent: {}", path.display())))?;
    let tmp = parent.join(format!(".{}.{}.tmp", path.file_name().and_then(|v| v.to_str()).unwrap_or("write"), Uuid::new_v4().simple()));
    let mut file = OpenOptions::new().create_new(true).write(true).open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    if let Some(mode) = mode {
        set_mode(&tmp, mode)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn atomic_copy(source: &Path, destination: &Path, mode: Option<u32>) -> Result<(), SearchError> {
    let bytes = fs::read(source)?;
    atomic_write(destination, &bytes, mode)
}

pub fn backup_file(path: &Path, backup_dir: &Path, label: &str) -> Result<Option<PathBuf>, SearchError> {
    if !path.exists() {
        return Ok(None);
    }
    fs::create_dir_all(backup_dir)?;
    let backup = backup_dir.join(format!("{label}.{}.bak", now_unix()));
    fs::copy(path, &backup)?;
    Ok(Some(backup))
}

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

pub fn remove_file_if_exists(path: &Path) -> Result<(), SearchError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(unix)]
pub fn set_mode(path: &Path, mode: u32) -> Result<(), SearchError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
pub fn set_mode(_path: &Path, _mode: u32) -> Result<(), SearchError> {
    Ok(())
}
