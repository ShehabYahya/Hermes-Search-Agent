use std::{env, path::{Path, PathBuf}};

use crate::error::SearchError;

pub const APP_NAME: &str = "hermes-search-agent";
pub const DEFAULT_HERMES_PROFILE: &str = "hsa-research";
pub const DEFAULT_DSH_PROFILE: &str = "web";
pub const SERVICE_NAME: &str = "hermes-search-agent.service";
pub const OWNER_MARKER: &str = ".hermes-search-agent-owned";

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub home: PathBuf,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub binary_path: PathBuf,
    pub config_file: PathBuf,
    pub secrets_file: PathBuf,
    pub manifest_file: PathBuf,
    pub backups_dir: PathBuf,
    pub systemd_user_dir: PathBuf,
    pub service_file: PathBuf,
    pub hermes_home: PathBuf,
    pub dsh_home: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self, SearchError> {
        let home = env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| SearchError::Config("HOME is not set".to_string()))?;

        let config_root = env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let state_root = env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/state"));

        let config_dir = config_root.join(APP_NAME);
        let state_dir = state_root.join(APP_NAME);
        let systemd_user_dir = config_root.join("systemd/user");
        let hermes_home = env_path("HERMES_HOME", &home, home.join(".hermes"));
        let dsh_home = env_path("DSH_HOME", &home, home.join(".dsh"));

        Ok(Self {
            binary_path: home.join(".local/bin").join(APP_NAME),
            config_file: config_dir.join("config.toml"),
            secrets_file: config_dir.join("secrets.env"),
            manifest_file: state_dir.join("install.json"),
            backups_dir: state_dir.join("backups"),
            service_file: systemd_user_dir.join(SERVICE_NAME),
            home,
            config_dir,
            state_dir,
            systemd_user_dir,
            hermes_home,
            dsh_home,
        })
    }

    pub fn hermes_profile_dir(&self, profile: &str) -> PathBuf {
        self.hermes_home.join("profiles").join(profile)
    }

    pub fn hermes_profile_marker(&self, profile: &str) -> PathBuf {
        self.hermes_profile_dir(profile).join(OWNER_MARKER)
    }

    pub fn dsh_profile_dir(&self, profile: &str) -> PathBuf {
        self.dsh_home.join("profiles").join(profile)
    }

    pub fn dsh_patch_file(&self, profile: &str) -> PathBuf {
        self.dsh_profile_dir(profile).join("cordis.patch.yml")
    }

    pub fn dsh_settings_file(&self) -> PathBuf {
        self.dsh_home.join("settings.yaml")
    }

    pub fn dsh_user_presets_dir(&self) -> PathBuf {
        self.dsh_home.join(".agent-presets")
    }

    pub fn dsh_user_preset_dir(&self, preset: &str) -> PathBuf {
        self.dsh_user_presets_dir().join(preset)
    }
}

fn env_path(name: &str, home: &Path, default: PathBuf) -> PathBuf {
    match env::var(name) {
        Ok(raw) if !raw.trim().is_empty() => expand_home(&raw, home),
        _ => default,
    }
}

fn expand_home(raw: &str, home: &Path) -> PathBuf {
    if raw == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    PathBuf::from(raw)
}
