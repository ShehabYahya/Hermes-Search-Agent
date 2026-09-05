pub mod binary;
pub mod dsh;
pub mod hermes;
pub mod manifest;
pub mod preflight;
pub mod secrets;
pub mod systemd;
pub mod transaction;

use std::{fs, path::Path};

use uuid::Uuid;

use crate::{
    config::FileConfig,
    doctor,
    error::SearchError,
    fsutil::remove_file_if_exists,
    paths::AppPaths,
    process::{resolve_command, run_capture},
};

use manifest::InstallManifest;
use transaction::FileSnapshot;

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
    println!("Hermes: {}", preflight.hermes.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| format!("not installed (will install tested Hermes {})", hermes::TESTED_HERMES_VERSION)));
    println!("Hermes profile: hsa-research");
    println!("MCP endpoint: http://127.0.0.1:8932/mcp");
    println!("DSH profile: {}", options.dsh_profile);
    if options.dry_run {
        println!("Dry run complete; no files were changed.");
        return Ok(());
    }

    let profile = "hsa-research".to_string();
    let profile_dir = paths.hermes_profile_dir(&profile);
    let profile_existed = profile_dir.exists();
    let service_was_active = systemd::is_active(&paths, "hermes-search-agent.service");
    let snapshots = InstallSnapshots::capture(&paths, &profile, &options.dsh_profile)?;

    let install_id = existing_install_id(&paths, preflight.existing.as_ref())?;
    let mut manifest = preflight.existing.unwrap_or_else(|| InstallManifest::new(
        &paths,
        install_id,
        profile.clone(),
        options.dsh_profile.clone(),
        "hermes_search".into(),
    ));

    let result = apply_install(&paths, &preflight.dsh, &options, &mut manifest).await;
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            eprintln!("Installation failed; rolling back managed changes...");
            rollback_install(&paths, &manifest, &snapshots, profile_existed, service_was_active);
            Err(error)
        }
    }
}

async fn apply_install(paths: &AppPaths, dsh_bin: &Path, options: &InstallOptions, manifest: &mut InstallManifest) -> Result<(), SearchError> {
    fs::create_dir_all(&paths.config_dir)?;
    fs::create_dir_all(&paths.state_dir)?;
    fs::create_dir_all(&paths.backups_dir)?;
    fs::create_dir_all(&paths.systemd_user_dir)?;

    let mut config = FileConfig::load(paths)?;
    config.dsh.profile = options.dsh_profile.clone();
    config.hermes.profile = manifest.hermes_profile.clone();
    config.hermes.url = "http://127.0.0.1:8643".into();
    config.mcp.bind = "127.0.0.1:8932".into();
    config.dsh.server_name = "hermes_search".into();
    config.save(paths)?;

    let secret = secrets::load_or_generate(paths)?;
    secrets::write(paths, &secret)?;
    binary::install_current(paths)?;

    manifest.dsh_profile = config.dsh.profile.clone();
    manifest.server_name = config.dsh.server_name.clone();
    let (hermes_bin, installed_hermes) = hermes::ensure_hermes(paths).await?;
    manifest.installed_hermes |= installed_hermes;
    hermes::ensure_profile(paths, &hermes_bin, manifest, &secret, options.non_interactive)?;
    systemd::install(paths, &manifest.hermes_profile)?;
    dsh::integrate(paths, dsh_bin, manifest)?;
    manifest.save(paths)?;

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

fn existing_install_id(paths: &AppPaths, manifest: Option<&InstallManifest>) -> Result<String, SearchError> {
    if let Some(manifest) = manifest { return Ok(manifest.install_id.clone()); }
    let marker = paths.hermes_profile_marker("hsa-research");
    if marker.exists() {
        let id = fs::read_to_string(&marker)?;
        if !id.trim().is_empty() { return Ok(id.trim().to_string()); }
    }
    Ok(Uuid::new_v4().to_string())
}

struct InstallSnapshots {
    config: FileSnapshot,
    secrets: FileSnapshot,
    manifest: FileSnapshot,
    binary: FileSnapshot,
    service: FileSnapshot,
    dsh_patch: FileSnapshot,
    profile_config: FileSnapshot,
    profile_env: FileSnapshot,
    profile_marker: FileSnapshot,
}

impl InstallSnapshots {
    fn capture(paths: &AppPaths, profile: &str, dsh_profile: &str) -> Result<Self, SearchError> {
        let profile_dir = paths.hermes_profile_dir(profile);
        Ok(Self {
            config: FileSnapshot::capture(&paths.config_file)?,
            secrets: FileSnapshot::capture(&paths.secrets_file)?,
            manifest: FileSnapshot::capture(&paths.manifest_file)?,
            binary: FileSnapshot::capture(&paths.binary_path)?,
            service: FileSnapshot::capture(&paths.service_file)?,
            dsh_patch: FileSnapshot::capture(paths.dsh_patch_file(dsh_profile))?,
            profile_config: FileSnapshot::capture(profile_dir.join("config.yaml"))?,
            profile_env: FileSnapshot::capture(profile_dir.join(".env"))?,
            profile_marker: FileSnapshot::capture(profile_dir.join(".hermes-search-agent-owned"))?,
        })
    }

    fn restore_files(&self) {
        for snapshot in [
            &self.dsh_patch, &self.profile_marker, &self.profile_env, &self.profile_config,
            &self.service, &self.binary, &self.manifest, &self.secrets, &self.config,
        ] {
            if let Err(error) = snapshot.restore() {
                eprintln!("Rollback warning for {}: {error}", snapshot.path().display());
            }
        }
    }
}

fn rollback_install(paths: &AppPaths, manifest: &InstallManifest, snapshots: &InstallSnapshots, profile_existed: bool, service_was_active: bool) {
    let _ = systemd::uninstall(paths);
    if !profile_existed {
        if let Some(hermes_bin) = resolve_command("hermes", paths) {
            if paths.hermes_profile_marker(&manifest.hermes_profile).exists() {
                let _ = hermes::delete_owned_profile(paths, &hermes_bin, manifest);
            }
        }
    }
    snapshots.restore_files();
    if snapshots.service.existed() {
        if let Some(systemctl) = resolve_command("systemctl", paths) {
            let _ = run_capture(&systemctl, ["--user", "daemon-reload"]);
            if service_was_active {
                let _ = run_capture(&systemctl, ["--user", "enable", "--now", "hermes-search-agent.service"]);
            }
        }
    }
}
