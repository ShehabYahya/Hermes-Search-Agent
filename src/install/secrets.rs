use std::fs;

use uuid::Uuid;

use crate::{config::read_env_value, error::SearchError, fsutil::atomic_write, paths::AppPaths};

pub fn load_or_generate(paths: &AppPaths) -> Result<String, SearchError> {
    if let Some(value) = read_env_value(&paths.secrets_file, "HERMES_RESEARCH_API_KEY")? {
        if !value.trim().is_empty() { return Ok(value); }
    }
    Ok(format!("hsa_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()))
}

pub fn write(paths: &AppPaths, secret: &str) -> Result<(), SearchError> {
    let body = format!("# Managed by hermes-search-agent. Owner read/write only.\nHERMES_RESEARCH_API_KEY={secret}\n");
    atomic_write(&paths.secrets_file, body.as_bytes(), Some(0o600))
}

pub fn profile_env_merge(path: &std::path::Path, updates: &[(&str, &str)]) -> Result<(), SearchError> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();
    for (key, value) in updates {
        let prefix = format!("{key}=");
        if let Some(index) = lines.iter().position(|line| line.trim_start().starts_with(&prefix)) {
            lines[index] = format!("{key}={value}");
        } else {
            lines.push(format!("{key}={value}"));
        }
    }
    let mut body = lines.join("\n");
    body.push('\n');
    atomic_write(path, body.as_bytes(), Some(0o600))
}
