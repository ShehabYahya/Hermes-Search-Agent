use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{error::SearchError, fsutil::{atomic_write, now_unix}, paths::AppPaths};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstallManifest {
    pub install_id: String,
    pub app_version: String,
    pub hermes_profile: String,
    pub dsh_profile: String,
    pub server_name: String,
    pub binary_path: PathBuf,
    pub service_file: PathBuf,
    pub dsh_patch_file: PathBuf,
    pub dsh_backup: Option<PathBuf>,
    pub profile_owned: bool,
    pub installed_hermes: bool,
    pub installed_at: u64,
    pub updated_at: u64,
}

impl InstallManifest {
    pub fn new(paths: &AppPaths, install_id: String, hermes_profile: String, dsh_profile: String, server_name: String) -> Self {
        let now = now_unix();
        Self {
            install_id,
            app_version: env!("CARGO_PKG_VERSION").into(),
            dsh_patch_file: paths.dsh_patch_file(&dsh_profile),
            binary_path: paths.binary_path.clone(),
            service_file: paths.service_file.clone(),
            hermes_profile,
            dsh_profile,
            server_name,
            dsh_backup: None,
            profile_owned: false,
            installed_hermes: false,
            installed_at: now,
            updated_at: now,
        }
    }

    pub fn load(paths: &AppPaths) -> Result<Option<Self>, SearchError> {
        if !paths.manifest_file.exists() { return Ok(None); }
        let raw = fs::read_to_string(&paths.manifest_file)?;
        let value = serde_json::from_str(&raw).map_err(|e| SearchError::Install(format!("invalid install manifest {}: {e}", paths.manifest_file.display())))?;
        Ok(Some(value))
    }

    pub fn save(&mut self, paths: &AppPaths) -> Result<(), SearchError> {
        self.app_version = env!("CARGO_PKG_VERSION").into();
        self.updated_at = now_unix();
        let raw = serde_json::to_vec_pretty(self).map_err(|e| SearchError::Install(format!("cannot serialize install manifest: {e}")))?;
        atomic_write(&paths.manifest_file, &raw, Some(0o600))
    }
}
