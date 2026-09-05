use std::path::Path;

use crate::{error::SearchError, fsutil::{atomic_write, remove_file_if_exists}, paths::{AppPaths, SERVICE_NAME}, process::{resolve_command, run_capture, run_checked}};

pub fn install(paths: &AppPaths, hermes_profile: &str) -> Result<(), SearchError> {
    let unit = unit_text(paths, hermes_profile);
    atomic_write(&paths.service_file, unit.as_bytes(), Some(0o644))?;
    let systemctl = resolve_command("systemctl", paths).ok_or_else(|| SearchError::Install("systemctl not found".into()))?;
    run_checked(&systemctl, ["--user", "daemon-reload"])?;
    run_checked(&systemctl, ["--user", "enable", "--now", SERVICE_NAME])?;
    Ok(())
}

pub fn restart(paths: &AppPaths) -> Result<(), SearchError> {
    let systemctl = resolve_command("systemctl", paths).ok_or_else(|| SearchError::Install("systemctl not found".into()))?;
    run_checked(&systemctl, ["--user", "daemon-reload"])?;
    run_checked(&systemctl, ["--user", "restart", SERVICE_NAME])?;
    Ok(())
}

pub fn uninstall(paths: &AppPaths) -> Result<(), SearchError> {
    if let Some(systemctl) = resolve_command("systemctl", paths) {
        let _ = run_capture(&systemctl, ["--user", "disable", "--now", SERVICE_NAME]);
        remove_file_if_exists(&paths.service_file)?;
        let _ = run_capture(&systemctl, ["--user", "daemon-reload"]);
    } else {
        remove_file_if_exists(&paths.service_file)?;
    }
    Ok(())
}

pub fn is_active(paths: &AppPaths, service: &str) -> bool {
    let Some(systemctl) = resolve_command("systemctl", paths) else { return false; };
    run_capture(&systemctl, ["--user", "is-active", "--quiet", service]).map(|o| o.success()).unwrap_or(false)
}

fn unit_text(paths: &AppPaths, hermes_profile: &str) -> String {
    let gateway = format!("hermes-gateway-{hermes_profile}.service");
    format!("[Unit]\nDescription=Hermes Search Agent MCP\nAfter=network-online.target {gateway}\nWants=network-online.target\n\n[Service]\nType=simple\nEnvironmentFile={}\nExecStart={} serve\nRestart=on-failure\nRestartSec=2\nNoNewPrivileges=true\nPrivateTmp=true\n\n[Install]\nWantedBy=default.target\n", escape(&paths.secrets_file), escape(&paths.binary_path))
}

fn escape(path: &Path) -> String {
    path.to_string_lossy().replace(' ', "\\x20")
}
