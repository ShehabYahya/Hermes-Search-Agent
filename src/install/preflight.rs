use std::{net::{SocketAddr, TcpListener}, path::PathBuf};

use crate::{
    error::SearchError,
    install::manifest::InstallManifest,
    paths::AppPaths,
    process::{resolve_command, run_capture},
};

#[derive(Debug)]
pub struct Preflight {
    pub dsh: PathBuf,
    pub dsh_version: String,
    pub hermes: Option<PathBuf>,
    pub existing: Option<InstallManifest>,
}

pub fn run(paths: &AppPaths, dsh_profile: &str) -> Result<Preflight, SearchError> {
    if !cfg!(target_os = "linux") {
        return Err(SearchError::Install("the managed installer currently supports Linux only".into()));
    }
    let systemctl = resolve_command("systemctl", paths).ok_or_else(|| SearchError::Install("systemctl was not found".into()))?;
    let systemd = run_capture(&systemctl, ["--user", "show-environment"])?;
    if !systemd.success() {
        return Err(SearchError::Install(format!("systemd user manager is unavailable: {}", systemd.stderr)));
    }

    let dsh = resolve_command("dsh", paths).ok_or_else(|| SearchError::Install("dsh was not found in PATH or ~/.local/bin".into()))?;
    let version = run_capture(&dsh, ["--version"])?;
    if !version.success() {
        return Err(SearchError::Install(format!("dsh --version failed: {}", version.stderr)));
    }

    let existing = InstallManifest::load(paths)?;
    let hermes = resolve_command("hermes", paths);
    if hermes.is_none() {
        for command in ["bash", "curl", "git"] {
            if resolve_command(command, paths).is_none() {
                return Err(SearchError::Install(format!("{command} is required to install Hermes")));
            }
        }
    }

    if existing.is_none() {
        ensure_port_free("127.0.0.1:8643".parse().unwrap(), "Hermes research API")?;
        ensure_port_free("127.0.0.1:8932".parse().unwrap(), "research MCP")?;
    }

    let profile_dir = paths.hermes_profile_dir("hsa-research");
    if profile_dir.exists() && existing.is_none() && !paths.hermes_profile_marker("hsa-research").exists() {
        return Err(SearchError::Install(format!("Hermes profile '{}' already exists but is not owned by this application", profile_dir.display())));
    }

    let dsh_profile_dir = paths.dsh_profile_dir(dsh_profile);
    if dsh_profile_dir.exists() && !dsh_profile_dir.is_dir() {
        return Err(SearchError::Install(format!("DSH profile path is not a directory: {}", dsh_profile_dir.display())));
    }

    Ok(Preflight { dsh, dsh_version: version.stdout, hermes, existing })
}

fn ensure_port_free(addr: SocketAddr, label: &str) -> Result<(), SearchError> {
    TcpListener::bind(addr).map(|listener| drop(listener)).map_err(|e| SearchError::Install(format!("{label} port {addr} is already in use: {e}")))
}
