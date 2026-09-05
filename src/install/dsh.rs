use std::{fs, path::{Path, PathBuf}};

use serde_yaml::Value;

use crate::{
    error::SearchError,
    fsutil::{atomic_write, backup_file},
    install::manifest::InstallManifest,
    paths::AppPaths,
    process::{run_capture, run_checked},
};

const BEGIN: &str = "# BEGIN hermes-search-agent managed MCP";
const END: &str = "# END hermes-search-agent managed MCP";

pub fn integrate(paths: &AppPaths, dsh: &Path, manifest: &mut InstallManifest) -> Result<(), SearchError> {
    initialize_profile(dsh, &manifest.dsh_profile)?;
    let patch = paths.dsh_patch_file(&manifest.dsh_profile);
    let existing = fs::read_to_string(&patch).unwrap_or_else(|_| "[]\n".into());
    validate_patch(&existing, &patch)?;
    if manifest.dsh_backup.is_none() {
        manifest.dsh_backup = backup_file(&patch, &paths.backups_dir, &format!("dsh-{}-cordis.patch.yml", manifest.dsh_profile))?;
    }
    let block = managed_block(&manifest.server_name);
    let updated = upsert_block(&existing, &block)?;
    atomic_write(&patch, updated.as_bytes(), Some(0o600))?;

    if let Err(error) = validate_dsh_config(dsh, &manifest.dsh_profile) {
        if let Some(backup) = &manifest.dsh_backup {
            if backup.exists() { fs::copy(backup, &patch)?; }
        }
        return Err(error);
    }
    Ok(())
}

pub fn remove(paths: &AppPaths, profile: &str) -> Result<(), SearchError> {
    let patch = paths.dsh_patch_file(profile);
    if !patch.exists() { return Ok(()); }
    let existing = fs::read_to_string(&patch)?;
    let updated = remove_block(&existing)?;
    atomic_write(&patch, updated.as_bytes(), Some(0o600))
}

pub fn is_integrated(paths: &AppPaths, profile: &str) -> bool {
    fs::read_to_string(paths.dsh_patch_file(profile)).map(|v| v.contains(BEGIN) && v.contains(END)).unwrap_or(false)
}

pub fn managed_block(server_name: &str) -> String {
    format!("{BEGIN}\n- insert:\n    - id: mcp-hermes-search\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        serverName: {server_name}\n        transport: streamable-http\n        url: http://127.0.0.1:8932/mcp\n        headers: {{}}\n        toolCallTimeoutMs: 390000\n        failOnStartupError: false\n{END}")
}

fn initialize_profile(dsh: &Path, profile: &str) -> Result<(), SearchError> {
    let primary = run_capture(dsh, ["--profile", profile, "--dump-config"])?;
    if primary.success() { return Ok(()); }
    let fallback = run_capture(dsh, ["web", "--dump-config"])?;
    if fallback.success() { return Ok(()); }
    Err(SearchError::Install(format!("could not initialize DSH profile {profile}: {}; fallback: {}", primary.stderr, fallback.stderr)))
}

fn validate_dsh_config(dsh: &Path, profile: &str) -> Result<(), SearchError> {
    let result = run_capture(dsh, ["--profile", profile, "--dump-config"])?;
    if result.success() { return Ok(()); }
    let fallback = run_capture(dsh, ["web", "--dump-config"])?;
    if fallback.success() { return Ok(()); }
    Err(SearchError::Install(format!("DSH rejected the managed MCP patch: {}; fallback: {}", result.stderr, fallback.stderr)))
}

fn validate_patch(raw: &str, path: &Path) -> Result<(), SearchError> {
    if raw.trim().is_empty() { return Ok(()); }
    let value: Value = serde_yaml::from_str(raw).map_err(|e| SearchError::Install(format!("cannot safely edit {}: invalid YAML: {e}", path.display())))?;
    if !value.is_sequence() {
        return Err(SearchError::Install(format!("cannot safely edit {}: expected a top-level YAML sequence", path.display())));
    }
    Ok(())
}

fn upsert_block(existing: &str, block: &str) -> Result<String, SearchError> {
    if existing.contains(BEGIN) || existing.contains(END) {
        return replace_block(existing, block);
    }
    let trimmed = existing.trim();
    if trimmed.is_empty() || trimmed == "[]" {
        return Ok(format!("{block}\n"));
    }
    Ok(format!("{}\n\n{block}\n", existing.trim_end()))
}

fn replace_block(existing: &str, block: &str) -> Result<String, SearchError> {
    let start = existing.find(BEGIN).ok_or_else(|| SearchError::Install("managed DSH block is malformed: missing BEGIN marker".into()))?;
    let end_start = existing[start..].find(END).map(|v| start + v).ok_or_else(|| SearchError::Install("managed DSH block is malformed: missing END marker".into()))?;
    let end = end_start + END.len();
    Ok(format!("{}{}{}", &existing[..start], block, &existing[end..]))
}

fn remove_block(existing: &str) -> Result<String, SearchError> {
    if !existing.contains(BEGIN) && !existing.contains(END) { return Ok(existing.to_string()); }
    let start = existing.find(BEGIN).ok_or_else(|| SearchError::Install("managed DSH block is malformed: missing BEGIN marker".into()))?;
    let end_start = existing[start..].find(END).map(|v| start + v).ok_or_else(|| SearchError::Install("managed DSH block is malformed: missing END marker".into()))?;
    let end = end_start + END.len();
    let remainder = format!("{}{}", &existing[..start], &existing[end..]).trim().to_string();
    if remainder.is_empty() { Ok("[]\n".into()) } else { Ok(format!("{remainder}\n")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_empty_sequence() {
        let out = upsert_block("[]\n", &managed_block("hermes_search")).unwrap();
        assert!(out.starts_with(BEGIN));
        assert!(!out.contains("[]"));
    }

    #[test]
    fn preserves_other_entries() {
        let original = "- id: existing\n  disabled: true\n";
        let out = upsert_block(original, &managed_block("hermes_search")).unwrap();
        assert!(out.contains("id: existing"));
        assert!(out.contains("mcp-hermes-search"));
        let removed = remove_block(&out).unwrap();
        assert!(removed.contains("id: existing"));
        assert!(!removed.contains("mcp-hermes-search"));
    }
}
