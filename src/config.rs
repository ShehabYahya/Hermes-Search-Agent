use std::{env, fs, net::SocketAddr, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{error::SearchError, fsutil::atomic_write, paths::{AppPaths, DEFAULT_DSH_PROFILE, DEFAULT_HERMES_PROFILE}};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    pub mcp: McpConfig,
    pub hermes: HermesConfig,
    pub timeouts: TimeoutConfig,
    pub dsh: DshConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    pub bind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct HermesConfig {
    pub profile: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeoutConfig {
    pub light: u64,
    pub medium: u64,
    pub deep: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DshConfig {
    pub profile: String,
    pub server_name: String,
}

impl Default for FileConfig {
    fn default() -> Self {
        Self { mcp: McpConfig::default(), hermes: HermesConfig::default(), timeouts: TimeoutConfig::default(), dsh: DshConfig::default() }
    }
}
impl Default for McpConfig { fn default() -> Self { Self { bind: "127.0.0.1:8932".into() } } }
impl Default for HermesConfig { fn default() -> Self { Self { profile: DEFAULT_HERMES_PROFILE.into(), url: "http://127.0.0.1:8643".into() } } }
impl Default for TimeoutConfig { fn default() -> Self { Self { light: 30, medium: 120, deep: 360 } } }
impl Default for DshConfig { fn default() -> Self { Self { profile: DEFAULT_DSH_PROFILE.into(), server_name: "hermes_search".into() } } }

impl FileConfig {
    pub fn load(paths: &AppPaths) -> Result<Self, SearchError> {
        if !paths.config_file.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&paths.config_file)?;
        toml::from_str(&raw).map_err(|e| SearchError::Config(format!("cannot parse {}: {e}", paths.config_file.display())))
    }

    pub fn save(&self, paths: &AppPaths) -> Result<(), SearchError> {
        let raw = toml::to_string_pretty(self).map_err(|e| SearchError::Config(format!("cannot serialize config: {e}")))?;
        atomic_write(&paths.config_file, raw.as_bytes(), Some(0o600))
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub bind: SocketAddr,
    pub hermes_url: String,
    pub hermes_api_key: String,
    pub hermes_profile: String,
    pub dsh_profile: String,
    pub server_name: String,
    pub light_timeout: Duration,
    pub medium_timeout: Duration,
    pub deep_timeout: Duration,
}

impl Settings {
    pub fn load(paths: &AppPaths) -> Result<Self, SearchError> {
        let file = FileConfig::load(paths)?;
        let bind = env::var("HERMES_SEARCH_MCP_BIND").unwrap_or(file.mcp.bind)
            .parse::<SocketAddr>().map_err(|e| SearchError::Config(format!("invalid MCP bind address: {e}")))?;
        let hermes_url = env::var("HERMES_RESEARCH_URL").unwrap_or(file.hermes.url);
        let hermes_api_key = env::var("HERMES_RESEARCH_API_KEY").ok()
            .or_else(|| read_env_value(&paths.secrets_file, "HERMES_RESEARCH_API_KEY").ok().flatten())
            .ok_or_else(|| SearchError::Config(format!("HERMES_RESEARCH_API_KEY is required (expected in {})", paths.secrets_file.display())))?;
        if hermes_api_key.trim().is_empty() {
            return Err(SearchError::Config("HERMES_RESEARCH_API_KEY cannot be empty".into()));
        }
        Ok(Self {
            bind,
            hermes_url,
            hermes_api_key,
            hermes_profile: file.hermes.profile,
            dsh_profile: file.dsh.profile,
            server_name: file.dsh.server_name,
            light_timeout: Duration::from_secs(timeout_env("HERMES_SEARCH_LIGHT_TIMEOUT_SECS", file.timeouts.light)?),
            medium_timeout: Duration::from_secs(timeout_env("HERMES_SEARCH_MEDIUM_TIMEOUT_SECS", file.timeouts.medium)?),
            deep_timeout: Duration::from_secs(timeout_env("HERMES_SEARCH_DEEP_TIMEOUT_SECS", file.timeouts.deep)?),
        })
    }
}

fn timeout_env(name: &str, default: u64) -> Result<u64, SearchError> {
    let value = match env::var(name) {
        Ok(raw) => raw.parse::<u64>().map_err(|e| SearchError::Config(format!("invalid {name}: {e}")))?,
        Err(_) => default,
    };
    if value == 0 { return Err(SearchError::Config(format!("{name} must be greater than zero"))); }
    Ok(value)
}

pub fn read_env_value(path: &std::path::Path, key: &str) -> Result<Option<String>, SearchError> {
    if !path.exists() { return Ok(None); }
    let content = fs::read_to_string(path)?;
    Ok(content.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { return None; }
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().trim_matches('"').to_string())
    }).next())
}
