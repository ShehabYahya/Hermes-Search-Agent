use crate::{config::FileConfig, error::SearchError, install::{dsh, manifest::InstallManifest, systemd}, paths::AppPaths, process::{resolve_command, run_capture}};

pub fn run() -> Result<(), SearchError> {
    let paths = AppPaths::discover()?;
    let config = FileConfig::load(&paths)?;
    let manifest = InstallManifest::load(&paths)?;
    println!("Hermes Search Agent {}", env!("CARGO_PKG_VERSION"));
    println!("Config: {} {}", paths.config_file.display(), mark(paths.config_file.exists()));
    println!("Manifest: {}", mark(manifest.is_some()));
    println!("Binary: {} {}", paths.binary_path.display(), mark(paths.binary_path.exists()));
    println!("Hermes profile: {} {}", config.hermes.profile, mark(paths.hermes_profile_dir(&config.hermes.profile).exists()));
    println!("Hermes API: {}", config.hermes.url);
    println!("MCP: http://{}/mcp", config.mcp.bind);
    println!("MCP service: {}", active(systemd::is_active(&paths, "hermes-search-agent.service")));
    println!("Hermes gateway: {}", active(systemd::is_active(&paths, &format!("hermes-gateway-{}.service", config.hermes.profile))));
    println!("DSH profile: {}", config.dsh.profile);
    println!("DSH integration: {}", mark(dsh::is_integrated(&paths, &config.dsh.profile)));
    println!("DSH native web_search: {}", state(config.dsh.native_web.search));
    println!("DSH native web_fetch: {}", state(config.dsh.native_web.fetch));
    if let Some(manifest) = manifest.as_ref() {
        if let Some(preset) = manifest.native_web.managed_preset_id.as_deref() {
            println!("DSH managed preset: {preset}");
        }
    }
    if let Some(dsh_bin) = resolve_command("dsh", &paths) {
        if let Ok(out) = run_capture(&dsh_bin, ["--version"]) { println!("DSH version: {}", out.stdout); }
    }
    if let Some(hermes_bin) = resolve_command("hermes", &paths) {
        if let Ok(out) = run_capture(&hermes_bin, ["--version"]) { println!("Hermes version: {}", out.stdout); }
    }
    Ok(())
}

fn mark(ok: bool) -> &'static str { if ok { "OK" } else { "MISSING" } }
fn active(ok: bool) -> &'static str { if ok { "running" } else { "stopped" } }
fn state(ok: bool) -> &'static str { if ok { "enabled" } else { "disabled" } }
