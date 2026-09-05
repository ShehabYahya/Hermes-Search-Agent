use std::{fs, path::{Path, PathBuf}};

use serde_yaml::{Mapping, Value};
use uuid::Uuid;

use crate::{
    config::DshNativeWebConfig,
    error::SearchError,
    fsutil::atomic_write,
    install::manifest::InstallManifest,
    paths::{AppPaths, OWNER_MARKER},
    process::run_capture,
};

pub const ROUTING_MARKER: &str = "HERMES SEARCH AGENT ROUTING";

/// Reconcile the user-selected DSH native-web policy.
///
/// DSH's shipped presets are never edited. When either native tool is disabled,
/// HSA creates an owned user preset copied from the current default, changes only
/// that copy, and points DSH's user setting at it.
pub fn apply(
    paths: &AppPaths,
    dsh: &Path,
    dsh_profile: &str,
    policy: &DshNativeWebConfig,
    manifest: &mut InstallManifest,
) -> Result<(), SearchError> {
    manifest.native_web.choice_recorded = true;

    if policy.search && policy.fetch {
        restore(paths, manifest)?;
        return Ok(());
    }

    initialize_profile(dsh, dsh_profile)?;

    let source = if let Some(source) = manifest.native_web.source_preset_id.clone() {
        source
    } else {
        let source = detect_default_preset(paths, dsh_profile)?;
        manifest.native_web.original_settings_default = read_settings_default(paths)?;
        manifest.native_web.source_preset_id = Some(source.clone());
        source
    };

    let managed_id = format!("hsa-{source}");
    let source_dir = locate_source_preset(paths, dsh, dsh_profile, &source)?;
    let target_dir = paths.dsh_user_preset_dir(&managed_id);
    let temp_dir = paths.dsh_user_presets_dir().join(format!(".{managed_id}.tmp-{}", Uuid::new_v4()));

    fs::create_dir_all(paths.dsh_user_presets_dir())?;
    if target_dir.exists() {
        ensure_owned(&target_dir, &manifest.install_id)?;
    }
    if temp_dir.exists() { fs::remove_dir_all(&temp_dir)?; }

    let build = (|| -> Result<(), SearchError> {
        copy_dir_recursive(&source_dir, &temp_dir)?;
        modify_managed_preset(&temp_dir, &source, policy)?;
        atomic_write(&temp_dir.join(OWNER_MARKER), manifest.install_id.as_bytes(), Some(0o600))?;
        Ok(())
    })();
    if let Err(error) = build {
        let _ = fs::remove_dir_all(&temp_dir);
        return Err(error);
    }

    if target_dir.exists() { fs::remove_dir_all(&target_dir)?; }
    fs::rename(&temp_dir, &target_dir)?;
    write_settings_default(paths, Some(&managed_id))?;
    manifest.native_web.managed_preset_id = Some(managed_id);
    Ok(())
}

/// Restore the DSH preset choice that existed before HSA started managing it.
pub fn restore(paths: &AppPaths, manifest: &mut InstallManifest) -> Result<(), SearchError> {
    let Some(managed_id) = manifest.native_web.managed_preset_id.clone() else {
        manifest.native_web.source_preset_id = None;
        manifest.native_web.original_settings_default = None;
        return Ok(());
    };

    let current = read_settings_default(paths)?;
    if current.as_deref() == Some(managed_id.as_str()) {
        write_settings_default(paths, manifest.native_web.original_settings_default.as_deref())?;
    }

    remove_owned_preset(paths, &managed_id, &manifest.install_id)?;
    manifest.native_web.managed_preset_id = None;
    manifest.native_web.source_preset_id = None;
    manifest.native_web.original_settings_default = None;
    Ok(())
}

/// Best-effort cleanup used by transaction rollback before snapshots are restored.
pub fn remove_current_owned_preset(paths: &AppPaths, manifest: &InstallManifest) {
    if let Some(id) = manifest.native_web.managed_preset_id.as_deref() {
        let _ = remove_owned_preset(paths, id, &manifest.install_id);
    }
}

pub fn verify(paths: &AppPaths, policy: &DshNativeWebConfig, manifest: &InstallManifest) -> Result<(), SearchError> {
    if policy.search && policy.fetch {
        if manifest.native_web.managed_preset_id.is_some() {
            return Err(SearchError::Install("native web tools are configured to stay enabled, but a managed DSH preset is still recorded".into()));
        }
        return Ok(());
    }

    let managed_id = manifest.native_web.managed_preset_id.as_deref()
        .ok_or_else(|| SearchError::Install("native web tools are disabled in config but no managed DSH preset is recorded".into()))?;
    let dir = paths.dsh_user_preset_dir(managed_id);
    ensure_owned(&dir, &manifest.install_id)?;

    let actual = inspect_preset(&dir)?;
    if actual.search != policy.search || actual.fetch != policy.fetch {
        return Err(SearchError::Install(format!(
            "managed preset native web state does not match config (expected search={}, fetch={}; found search={}, fetch={})",
            policy.search, policy.fetch, actual.search, actual.fetch
        )));
    }
    if !actual.has_routing {
        return Err(SearchError::Install("managed DSH preset is missing Hermes research routing guidance".into()));
    }
    if read_settings_default(paths)?.as_deref() != Some(managed_id) {
        return Err(SearchError::Install(format!("DSH default preset is not the managed preset {managed_id}")));
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct PresetInspection {
    search: bool,
    fetch: bool,
    has_routing: bool,
}

fn inspect_preset(dir: &Path) -> Result<PresetInspection, SearchError> {
    let path = dir.join("agent.cordis.yml");
    let raw = fs::read_to_string(&path)?;
    let doc: Value = serde_yaml::from_str(&raw)
        .map_err(|e| SearchError::Install(format!("cannot parse managed preset {}: {e}", path.display())))?;
    let rows = doc.as_sequence().ok_or_else(|| SearchError::Install(format!("managed preset {} is not a YAML sequence", path.display())))?;

    let tool = find_row(rows, "tool-web").ok_or_else(|| SearchError::Install("managed DSH preset has no tool-web row".into()))?;
    let config = row_config(tool)?;
    let search = bool_field(config, "search").unwrap_or(true);
    let fetch = bool_field(config, "fetch").unwrap_or(true);

    let persona = find_row(rows, "persona").ok_or_else(|| SearchError::Install("managed DSH preset has no persona row".into()))?;
    let persona_config = row_config(persona)?;
    let has_routing = string_field(persona_config, "text").map(|v| v.contains(ROUTING_MARKER)).unwrap_or(false);
    Ok(PresetInspection { search, fetch, has_routing })
}

fn modify_managed_preset(dir: &Path, source: &str, policy: &DshNativeWebConfig) -> Result<(), SearchError> {
    let composition = dir.join("agent.cordis.yml");
    let raw = fs::read_to_string(&composition)?;
    let mut doc: Value = serde_yaml::from_str(&raw)
        .map_err(|e| SearchError::Install(format!("cannot parse source DSH preset {}: {e}", composition.display())))?;
    let rows = doc.as_sequence_mut().ok_or_else(|| SearchError::Install(format!("DSH preset {} is not a YAML sequence", composition.display())))?;

    let tool = find_row_mut(rows, "tool-web").ok_or_else(|| SearchError::Install(format!("DSH preset {source} does not expose the native tool-web row")))?;
    let tool_config = row_config_mut(tool)?;
    tool_config.insert(key("search"), Value::Bool(policy.search));
    tool_config.insert(key("fetch"), Value::Bool(policy.fetch));

    let persona = find_row_mut(rows, "persona").ok_or_else(|| SearchError::Install(format!("DSH preset {source} has no persona row for routing guidance")))?;
    let persona_config = row_config_mut(persona)?;
    let current = string_field(persona_config, "text").ok_or_else(|| SearchError::Install(format!("DSH preset {source} persona has no text")))?.to_string();
    persona_config.insert(key("text"), Value::String(format!("{}\n\n{}", current.trim_end(), routing_text(policy))));

    let rendered = serde_yaml::to_string(&doc).map_err(|e| SearchError::Install(format!("cannot serialize managed DSH preset: {e}")))?;
    atomic_write(&composition, rendered.as_bytes(), Some(0o600))?;
    rewrite_metadata(dir, source)?;
    Ok(())
}

fn rewrite_metadata(dir: &Path, source: &str) -> Result<(), SearchError> {
    let path = dir.join("preset.yml");
    if !path.exists() { return Ok(()); }
    let raw = fs::read_to_string(&path)?;
    let mut value: Value = serde_yaml::from_str(&raw)
        .map_err(|e| SearchError::Install(format!("cannot parse preset metadata {}: {e}", path.display())))?;
    let map = value.as_mapping_mut().ok_or_else(|| SearchError::Install(format!("preset metadata {} is not a mapping", path.display())))?;
    map.insert(key("name"), Value::String(format!("Hermes Search Agent ({source})")));
    map.insert(key("description"), Value::String(format!("HSA-managed copy of the {source} preset with user-selected native web tool policy.")));
    map.remove(&key("order"));
    let rendered = serde_yaml::to_string(&value).map_err(|e| SearchError::Install(format!("cannot serialize preset metadata: {e}")))?;
    atomic_write(&path, rendered.as_bytes(), Some(0o600))
}

pub fn routing_text(policy: &DshNativeWebConfig) -> String {
    let availability = match (policy.search, policy.fetch) {
        (false, true) => "Native DSH web_search is disabled. Native web_fetch remains available only for retrieving a known URL directly.",
        (false, false) => "Native DSH web_search and web_fetch are disabled. Use Hermes Search Agent for external web discovery and research.",
        (true, false) => "Native DSH web_search remains available. Native web_fetch is disabled; use Hermes Search Agent when a research agent should retrieve and synthesize external evidence.",
        (true, true) => "Native DSH web_search and web_fetch remain available.",
    };
    format!(
        "{ROUTING_MARKER}\n\nFor web discovery and external research, use the Hermes Search Agent tools when they are the appropriate research path.\n\n- light_search: one narrow factual lookup or current fact.\n- medium_research: the default for ordinary multi-source research, comparisons, and several related questions.\n- deep_research: only for difficult evidence-driven investigations involving conflicting evidence, competing hypotheses, root cause, hidden subquestions, or consequential decisions.\n\nPrefer the lowest research level sufficient for the task. Do not use deep_research merely because a topic is technical or because more detail would be useful.\n\n{availability}"
    )
}

fn detect_default_preset(paths: &AppPaths, profile: &str) -> Result<String, SearchError> {
    if let Some(value) = read_settings_default(paths)? {
        return Ok(value);
    }
    let patch = paths.dsh_patch_file(profile);
    if patch.exists() {
        let raw = fs::read_to_string(&patch)?;
        if let Ok(Value::Sequence(rows)) = serde_yaml::from_str::<Value>(&raw) {
            for row in &rows {
                if row_id(row) == Some("agent-presets") {
                    if let Ok(config) = row_config(row) {
                        if let Some(default) = string_field(config, "default") {
                            return Ok(default.to_string());
                        }
                    }
                }
            }
        }
    }
    Ok("standard".into())
}

pub fn read_settings_default(paths: &AppPaths) -> Result<Option<String>, SearchError> {
    let path = paths.dsh_settings_file();
    if !path.exists() { return Ok(None); }
    let raw = fs::read_to_string(&path)?;
    let root: Value = serde_yaml::from_str(&raw)
        .map_err(|e| SearchError::Install(format!("cannot parse DSH settings {}: {e}", path.display())))?;
    let Some(root_map) = root.as_mapping() else { return Ok(None); };
    let Some(namespace) = root_map.get(&key("agent-presets")).and_then(Value::as_mapping) else { return Ok(None); };
    Ok(string_field(namespace, "default").map(str::to_string))
}

fn write_settings_default(paths: &AppPaths, value: Option<&str>) -> Result<(), SearchError> {
    let path = paths.dsh_settings_file();
    if value.is_none() && !path.exists() { return Ok(()); }
    let mut root = if path.exists() {
        let raw = fs::read_to_string(&path)?;
        serde_yaml::from_str::<Value>(&raw)
            .map_err(|e| SearchError::Install(format!("cannot parse DSH settings {}: {e}", path.display())))?
    } else {
        Value::Mapping(Mapping::new())
    };
    let root_map = root.as_mapping_mut().ok_or_else(|| SearchError::Install(format!("DSH settings {} must be a YAML mapping", path.display())))?;
    let namespace_key = key("agent-presets");

    match value {
        Some(default) => {
            if !root_map.contains_key(&namespace_key) {
                root_map.insert(namespace_key.clone(), Value::Mapping(Mapping::new()));
            }
            let namespace = root_map.get_mut(&namespace_key).and_then(Value::as_mapping_mut)
                .ok_or_else(|| SearchError::Install("DSH settings agent-presets entry must be a mapping".into()))?;
            namespace.insert(key("default"), Value::String(default.to_string()));
        }
        None => {
            let mut remove_namespace = false;
            if let Some(namespace) = root_map.get_mut(&namespace_key).and_then(Value::as_mapping_mut) {
                namespace.remove(&key("default"));
                remove_namespace = namespace.is_empty();
            }
            if remove_namespace { root_map.remove(&namespace_key); }
        }
    }

    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let rendered = serde_yaml::to_string(&root).map_err(|e| SearchError::Install(format!("cannot serialize DSH settings: {e}")))?;
    atomic_write(&path, rendered.as_bytes(), Some(0o600))
}

fn initialize_profile(dsh: &Path, profile: &str) -> Result<(), SearchError> {
    let primary = run_capture(dsh, ["--profile", profile, "--dump-config"])?;
    if primary.success() { return Ok(()); }
    let fallback = run_capture(dsh, ["web", "--dump-config"])?;
    if fallback.success() { return Ok(()); }
    Err(SearchError::Install(format!("could not initialize DSH profile {profile}: {}; fallback: {}", primary.stderr, fallback.stderr)))
}

fn locate_source_preset(paths: &AppPaths, dsh: &Path, profile: &str, id: &str) -> Result<PathBuf, SearchError> {
    let mut node_roots = vec![paths.dsh_profile_dir(profile).join("node_modules")];
    if let Ok(canonical) = fs::canonicalize(dsh) {
        for ancestor in canonical.ancestors() {
            if ancestor.file_name().and_then(|v| v.to_str()) == Some("node_modules") {
                node_roots.push(ancestor.to_path_buf());
            }
            let nested = ancestor.join("node_modules");
            if nested.is_dir() { node_roots.push(nested); }
        }
    }

    node_roots.sort();
    node_roots.dedup();
    for root in &node_roots {
        let direct = root.join("@deepseek-ai/dsh-agent-presets/presets").join(id);
        if direct.is_dir() { return Ok(direct); }
        let pnpm = root.join(".pnpm");
        if pnpm.is_dir() {
            for entry in fs::read_dir(&pnpm)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.contains("dsh-agent-presets") { continue; }
                let candidate = entry.path().join("node_modules/@deepseek-ai/dsh-agent-presets/presets").join(id);
                if candidate.is_dir() { return Ok(candidate); }
            }
        }
    }

    let user = paths.dsh_user_preset_dir(id);
    if user.is_dir() { return Ok(user); }

    Err(SearchError::Install(format!(
        "could not locate DSH preset {id}; expected @deepseek-ai/dsh-agent-presets under profile {} or a user preset under {}",
        paths.dsh_profile_dir(profile).display(), paths.dsh_user_presets_dir().display()
    )))
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), SearchError> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = fs::metadata(&source_path)?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn ensure_owned(dir: &Path, install_id: &str) -> Result<(), SearchError> {
    if !dir.is_dir() {
        return Err(SearchError::Install(format!("managed DSH preset path is not a directory: {}", dir.display())));
    }
    let marker = dir.join(OWNER_MARKER);
    let owner = fs::read_to_string(&marker).map_err(|_| SearchError::Install(format!(
        "DSH preset {} already exists but is not owned by this Hermes Search Agent installation", dir.display()
    )))?;
    if owner.trim() != install_id {
        return Err(SearchError::Install(format!("DSH preset {} belongs to a different installation", dir.display())));
    }
    Ok(())
}

fn remove_owned_preset(paths: &AppPaths, id: &str, install_id: &str) -> Result<(), SearchError> {
    let dir = paths.dsh_user_preset_dir(id);
    if !dir.exists() { return Ok(()); }
    ensure_owned(&dir, install_id)?;
    fs::remove_dir_all(dir)?;
    Ok(())
}

fn find_row<'a>(rows: &'a [Value], id: &str) -> Option<&'a Value> {
    rows.iter().find(|row| row_id(row) == Some(id))
}

fn find_row_mut<'a>(rows: &'a mut [Value], id: &str) -> Option<&'a mut Value> {
    rows.iter_mut().find(|row| row_id(row) == Some(id))
}

fn row_id(row: &Value) -> Option<&str> {
    row.as_mapping()?.get(&key("id"))?.as_str()
}

fn row_config(row: &Value) -> Result<&Mapping, SearchError> {
    row.as_mapping().and_then(|m| m.get(&key("config"))).and_then(Value::as_mapping)
        .ok_or_else(|| SearchError::Install("DSH preset row has no mapping config".into()))
}

fn row_config_mut(row: &mut Value) -> Result<&mut Mapping, SearchError> {
    let mapping = row.as_mapping_mut().ok_or_else(|| SearchError::Install("DSH preset row is not a mapping".into()))?;
    let config_key = key("config");
    if !mapping.contains_key(&config_key) {
        mapping.insert(config_key.clone(), Value::Mapping(Mapping::new()));
    }
    mapping.get_mut(&config_key).and_then(Value::as_mapping_mut)
        .ok_or_else(|| SearchError::Install("DSH preset row config is not a mapping".into()))
}

fn string_field<'a>(map: &'a Mapping, name: &str) -> Option<&'a str> {
    map.get(&key(name))?.as_str()
}

fn bool_field(map: &Mapping, name: &str) -> Option<bool> {
    map.get(&key(name))?.as_bool()
}

fn key(value: &str) -> Value { Value::String(value.to_string()) }

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
- id: persona
  name: '@deepseek-ai/dsh-persona'
  config:
    text: You are a coding agent.
- id: tool-web
  name: '@deepseek-ai/dsh-tool-web'
  config:
    fetch: true
    searchTimeoutMs: 60000
"#;

    #[test]
    fn routing_explains_lowest_sufficient_level() {
        let text = routing_text(&DshNativeWebConfig { search: false, fetch: true });
        assert!(text.contains("Prefer the lowest research level sufficient"));
        assert!(text.contains("web_search is disabled"));
        assert!(text.contains("web_fetch remains available"));
    }

    #[test]
    fn managed_document_disables_search_but_keeps_fetch() {
        let mut doc: Value = serde_yaml::from_str(SAMPLE).unwrap();
        let rows = doc.as_sequence_mut().unwrap();
        let tool = find_row_mut(rows, "tool-web").unwrap();
        let config = row_config_mut(tool).unwrap();
        config.insert(key("search"), Value::Bool(false));
        config.insert(key("fetch"), Value::Bool(true));
        let persona = find_row_mut(rows, "persona").unwrap();
        let persona_config = row_config_mut(persona).unwrap();
        let current = string_field(persona_config, "text").unwrap().to_string();
        persona_config.insert(key("text"), Value::String(format!("{current}\n\n{}", routing_text(&DshNativeWebConfig { search: false, fetch: true }))));
        let rendered = serde_yaml::to_string(&doc).unwrap();
        let parsed: Value = serde_yaml::from_str(&rendered).unwrap();
        let rows = parsed.as_sequence().unwrap();
        let tool = row_config(find_row(rows, "tool-web").unwrap()).unwrap();
        assert_eq!(bool_field(tool, "search"), Some(false));
        assert_eq!(bool_field(tool, "fetch"), Some(true));
        let persona = row_config(find_row(rows, "persona").unwrap()).unwrap();
        assert!(string_field(persona, "text").unwrap().contains(ROUTING_MARKER));
    }
}
