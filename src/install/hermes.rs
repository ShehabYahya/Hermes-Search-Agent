use std::{fs, path::{Path, PathBuf}};

use serde_yaml::{Mapping, Value};

use crate::{
    error::SearchError,
    fsutil::{atomic_write, set_mode},
    install::{manifest::InstallManifest, secrets},
    paths::{AppPaths, OWNER_MARKER},
    process::{resolve_command, run_capture, run_checked, run_inherit},
};

pub const TESTED_HERMES_REF: &str = "v2026.8.31";
pub const TESTED_HERMES_VERSION: &str = "0.21.0";
const INSTALLER_URL: &str = "https://raw.githubusercontent.com/NousResearch/hermes-agent/v2026.8.31/scripts/install.sh";

pub async fn ensure_hermes(paths: &AppPaths) -> Result<(PathBuf, bool), SearchError> {
    if let Some(path) = resolve_command("hermes", paths) {
        return Ok((path, false));
    }
    fs::create_dir_all(&paths.state_dir)?;
    let installer = paths.state_dir.join("hermes-install.sh");
    let response = reqwest::Client::new().get(INSTALLER_URL).send().await?.error_for_status()?;
    let bytes = response.bytes().await?;
    atomic_write(&installer, &bytes, Some(0o700))?;
    set_mode(&installer, 0o700)?;
    let bash = resolve_command("bash", paths).ok_or_else(|| SearchError::Install("bash disappeared during install".into()))?;
    run_inherit(&bash, [installer.as_os_str(), "--skip-setup".as_ref(), "--non-interactive".as_ref(), "--branch".as_ref(), TESTED_HERMES_REF.as_ref()])?;
    let hermes = resolve_command("hermes", paths).ok_or_else(|| SearchError::Install("Hermes installer completed but the hermes command was not found".into()))?;
    Ok((hermes, true))
}

pub fn ensure_profile(
    paths: &AppPaths,
    hermes: &Path,
    manifest: &mut InstallManifest,
    secret: &str,
    non_interactive: bool,
) -> Result<(), SearchError> {
    let profile = manifest.hermes_profile.clone();
    let profile_dir = paths.hermes_profile_dir(&profile);
    let marker = profile_dir.join(OWNER_MARKER);

    if profile_dir.exists() {
        verify_ownership(&marker, &manifest.install_id)?;
    } else {
        let clone = run_capture(hermes, [
            "profile", "create", &profile,
            "--clone-from", "default", "--clone", "--no-alias",
            "--description", "Dedicated evidence-driven web research agent managed by Hermes Search Agent.",
        ])?;
        if !clone.success() {
            run_checked(hermes, [
                "profile", "create", &profile, "--no-alias", "--no-skills",
                "--description", "Dedicated evidence-driven web research agent managed by Hermes Search Agent.",
            ])?;
        }
        manifest.profile_owned = true;
    }

    configure_profile(paths, &profile, secret)?;
    atomic_write(&marker, format!("{}\n", manifest.install_id).as_bytes(), Some(0o600))?;
    manifest.profile_owned = true;

    let _ = run_capture(hermes, ["-p", &profile, "skills", "opt-out"]);

    if !profile_has_model(&profile_dir.join("config.yaml"))? {
        if non_interactive {
            return Err(SearchError::Install(format!("Hermes profile {profile} has no configured model; run `hermes -p {profile} setup` and then `hermes-search-agent repair`")));
        }
        eprintln!("Hermes research profile needs a model/provider. Opening the Hermes setup wizard now.");
        run_inherit(hermes, ["-p", &profile, "setup"])?;
        configure_profile(paths, &profile, secret)?;
    }

    run_checked(hermes, ["-p", &profile, "gateway", "install", "--force", "--start-now", "--start-on-login"])?;
    Ok(())
}

pub fn restart_gateway(hermes: &Path, profile: &str) -> Result<(), SearchError> {
    run_checked(hermes, ["-p", profile, "gateway", "restart"])?;
    Ok(())
}

pub fn stop_gateway(hermes: &Path, profile: &str) -> Result<(), SearchError> {
    let _ = run_capture(hermes, ["-p", profile, "gateway", "stop"])?;
    Ok(())
}

pub fn delete_owned_profile(paths: &AppPaths, hermes: &Path, manifest: &InstallManifest) -> Result<(), SearchError> {
    let marker = paths.hermes_profile_marker(&manifest.hermes_profile);
    verify_ownership(&marker, &manifest.install_id)?;
    run_checked(hermes, ["profile", "delete", &manifest.hermes_profile, "--yes"])?;
    Ok(())
}

fn configure_profile(paths: &AppPaths, profile: &str, secret: &str) -> Result<(), SearchError> {
    let dir = paths.hermes_profile_dir(profile);
    fs::create_dir_all(&dir)?;
    let config_path = dir.join("config.yaml");
    let mut root = if config_path.exists() {
        let raw = fs::read_to_string(&config_path)?;
        serde_yaml::from_str::<Value>(&raw).unwrap_or_else(|_| Value::Mapping(Mapping::new()))
    } else {
        Value::Mapping(Mapping::new())
    };
    let map = root.as_mapping_mut().ok_or_else(|| SearchError::Install(format!("Hermes profile config is not a mapping: {}", config_path.display())))?;

    let mut toolsets = Mapping::new();
    toolsets.insert(Value::String("api_server".into()), Value::Sequence(vec![Value::String("web".into()), Value::String("browser".into())]));
    map.insert(Value::String("platform_toolsets".into()), Value::Mapping(toolsets));

    let web = mapping_entry(map, "web");
    web.insert(Value::String("search_backend".into()), Value::String("ddgs".into()));
    web.insert(Value::String("extract_backend".into()), Value::String("firecrawl".into()));

    let gateway = mapping_entry(map, "gateway");
    gateway.insert(Value::String("multiplex_profiles".into()), Value::Bool(false));

    let platforms = mapping_entry(map, "platforms");
    for name in [
        "telegram", "discord", "whatsapp", "whatsapp_cloud", "slack", "signal", "mattermost", "matrix",
        "homeassistant", "email", "sms", "dingtalk", "webhook", "msgraph_webhook", "feishu", "wecom",
        "wecom_callback", "weixin", "bluebubbles", "qqbot", "yuanbao", "relay",
    ] {
        let mut p = Mapping::new();
        p.insert(Value::String("enabled".into()), Value::Bool(false));
        platforms.insert(Value::String(name.into()), Value::Mapping(p));
    }
    let mut api = Mapping::new();
    api.insert(Value::String("enabled".into()), Value::Bool(true));
    platforms.insert(Value::String("api_server".into()), Value::Mapping(api));

    let raw = serde_yaml::to_string(&root).map_err(|e| SearchError::Install(format!("cannot serialize Hermes profile config: {e}")))?;
    atomic_write(&config_path, raw.as_bytes(), Some(0o600))?;

    secrets::profile_env_merge(&dir.join(".env"), &[
        ("API_SERVER_ENABLED", "true"),
        ("API_SERVER_HOST", "127.0.0.1"),
        ("API_SERVER_PORT", "8643"),
        ("API_SERVER_KEY", secret),
        ("GATEWAY_MULTIPLEX_PROFILES", "false"),
    ])?;
    Ok(())
}

fn mapping_entry<'a>(root: &'a mut Mapping, key: &str) -> &'a mut Mapping {
    let value = root.entry(Value::String(key.into())).or_insert_with(|| Value::Mapping(Mapping::new()));
    if !value.is_mapping() { *value = Value::Mapping(Mapping::new()); }
    value.as_mapping_mut().expect("mapping inserted")
}

fn profile_has_model(path: &Path) -> Result<bool, SearchError> {
    if !path.exists() { return Ok(false); }
    let raw = fs::read_to_string(path)?;
    let value: Value = serde_yaml::from_str(&raw).map_err(|e| SearchError::Install(format!("invalid Hermes profile config: {e}")))?;
    let model = value.get("model").and_then(Value::as_mapping);
    Ok(model.is_some_and(|m| ["default", "model"].iter().any(|k| m.get(Value::String((*k).into())).and_then(Value::as_str).is_some_and(|s| !s.trim().is_empty()))))
}

fn verify_ownership(marker: &Path, install_id: &str) -> Result<(), SearchError> {
    let actual = fs::read_to_string(marker).map_err(|_| SearchError::Install(format!("refusing to manage existing Hermes profile without ownership marker: {}", marker.display())))?;
    if actual.trim() != install_id {
        return Err(SearchError::Install(format!("Hermes profile ownership marker does not match this installation: {}", marker.display())));
    }
    Ok(())
}
