pub mod binary;
pub mod dsh;
pub mod hermes;
pub mod manifest;
pub mod preflight;
pub mod secrets;
pub mod systemd;
pub mod transaction;

use std::fs;

use uuid::Uuid;

use crate::{
    config::FileConfig,
    doctor,
    error::SearchError,
    fsutil::{remove_file_if_exists},
    paths::AppPaths,
    process::resolve_command,
};

use manifest::InstallManifest;

#[derive(Clone, Debug)]
pub struct InstallOptions {
    pub dry_run: bool,
    pub non_interactive: bool,
    pub dsh_profile: String,
}

pub async fn install(options: InstallOptions) -> Result<(), SearchError> {
    let paths = AppPaths::discover()?;
    let preflight = preflight::run(&paths, &options.dsh_profile)?;
    println!("DSH: {} ({})", preflight.dsh.display(), preflight.dsh_version);
    println!("Hermes: {}", preflight.hermes.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "not installed (will install tested Hermes)".into()));
    println!("Hermes profile: hsa-research");
    println!("MCP endpoint: http://127.0.0.1:8932/mcp");
    println!("DSH profile: {}", options.dsh_profile);
    if options.dry_run {
        println!("Dry run complete; no files were changed.");
        return Ok(());
    }

    fs::create_dir_all(&paths.config_dir)?;
    fs::create_dir_all(&paths.state_dir)?;
    fs::create_dir_all(&paths.backups_dir)?;
    fs::create_dir_all(&paths.systemd_user_dir)?;

    let mut config = FileConfig::load(&paths)?;
    config.dsh.profile = options.dsh_profile.clone();
    config.hermes.profile = "hsa-research".into();
    config.hermes.url = "http://127.0.0.1:8643".into();
    config.mcp.bind = "127.0.0.1:8932".into();
    config.dsh.server_name = "hermes_search".into();
    config.save(&paths)?;

    let secret = secrets::load_or_generate(&paths)?;
    secrets::write(&paths, &secret)?;
    binary::install_current(&paths)?;

    let install_id = preflight.existing.as_ref().map(|m| m.install_id.clone()).unwrap_or_else(|| Uuid::new_v4().to_string());
    let mut manifest = preflight.existing.unwrap_or_else(|| InstallManifest::new(&paths, install_id, config.hermes.profile.clone(), config.dsh.profile.clone(), config.dsh.server_name.clone()));
    manifest.dsh_profile = config.dsh.profile.clone();
    manifest.server_name = config.dsh.server_name.clone();

    let (hermes_bin, installed_hermes) = hermes::ensure_hermes(&paths).await?;
    manifest.installed_hermes |= installed_hermes;
    hermes::ensure_profile(&paths, &hermes_bin, &mut manifest, &secret, options.non_interactive)?;
    systemd::install(&paths, &manifest.hermes_profile)?;
    dsh::integrate(&paths, &preflight.dsh, &mut manifest)?;
    manifest.save(&paths)?;

    println!("Installation written. Running compatibility check...");
    doctor::run(true).await?;
    println!("Ready. DSH will expose mcp__hermes_search__light_search, mcp__hermes_search__medium_research, and mcp__hermes_search__deep_research.");
    Ok(())
}

pub async fn repair(non_interactive: bool) -> Result<(), SearchError> {
    let paths = AppPaths::discover()?;
    let mut manifest = InstallManifest::load(&paths)?.ok_or_else(|| SearchError::Install("no installation manifest found; run install first".into()))?;
    let config = FileConfig::load(&paths)?;
    let secret = secrets::load_or_generate(&paths)?;
    secrets::write(&paths, &secret)?;
    binary::install_current(&paths)?;
    let (hermes_bin, installed) = hermes::ensure_hermes(&paths).await?;
    manifest.installed_hermes |= installed;
    hermes::ensure_profile(&paths, &hermes_bin, &mut manifest, &secret, non_interactive)?;
    systemd::install(&paths, &manifest.hermes_profile)?;
    let dsh_bin = resolve_command("dsh", &paths).ok_or_else(|| SearchError::Install("dsh not found".into()))?;
    manifest.dsh_profile = config.dsh.profile;
    manifest.server_name = config.dsh.server_name;
    dsh::integrate(&paths, &dsh_bin, &mut manifest)?;
    manifest.save(&paths)?;
    doctor::run(true).await?;
    println!("Repair complete.");
    Ok(())
}

pub fn uninstall(purge: bool) -> Result<(), SearchError> {
    let paths = AppPaths::discover()?;
    let manifest = InstallManifest::load(&paths)?.ok_or_else(|| SearchError::Install("no installation manifest found".into()))?;
    dsh::remove(&paths, &manifest.dsh_profile)?;
    systemd::uninstall(&paths)?;
    if let Some(hermes_bin) = resolve_command("hermes", &paths) {
        let _ = hermes::stop_gateway(&hermes_bin, &manifest.hermes_profile);
        if purge && manifest.profile_owned {
            hermes::delete_owned_profile(&paths, &hermes_bin, &manifest)?;
        }
    }
    remove_file_if_exists(&paths.binary_path)?;
    if purge {
        if paths.config_dir.exists() { fs::remove_dir_all(&paths.config_dir)?; }
        if paths.state_dir.exists() { fs::remove_dir_all(&paths.state_dir)?; }
        println!("Purged Hermes Search Agent managed state.");
    } else {
        println!("Uninstalled services and DSH integration. Hermes research profile and application state were preserved.");
    }
    Ok(())
}
