use crate::{
    config::Settings,
    error::SearchError,
    hermes::HermesClient,
    install::{dsh, manifest::InstallManifest, systemd},
    paths::AppPaths,
    process::resolve_command,
    research::ResearchRunner,
    tools::LightSearchArgs,
};

pub async fn run(quick: bool) -> Result<(), SearchError> {
    let paths = AppPaths::discover()?;
    let manifest = InstallManifest::load(&paths)?;
    let settings = Settings::load(&paths)?;
    let mut failed = false;

    check("configuration", paths.config_file.exists(), &mut failed);
    check("secret file", paths.secrets_file.exists(), &mut failed);
    check("installed binary", paths.binary_path.exists(), &mut failed);
    check("Hermes command", resolve_command("hermes", &paths).is_some(), &mut failed);
    check("DSH command", resolve_command("dsh", &paths).is_some(), &mut failed);
    check("Hermes research profile", paths.hermes_profile_dir(&settings.hermes_profile).is_dir(), &mut failed);
    check("profile ownership marker", paths.hermes_profile_marker(&settings.hermes_profile).is_file(), &mut failed);
    check("DSH MCP patch", dsh::is_integrated(&paths, &settings.dsh_profile), &mut failed);
    check("MCP systemd service", systemd::is_active(&paths, "hermes-search-agent.service"), &mut failed);
    check("Hermes gateway service", systemd::is_active(&paths, &format!("hermes-gateway-{}.service", settings.hermes_profile)), &mut failed);

    let hermes = HermesClient::new(&settings.hermes_url, &settings.hermes_api_key);
    match hermes.require_research_capabilities().await {
        Ok(_) => println!("[ok] Hermes run API capabilities"),
        Err(error) => { println!("[FAIL] Hermes run API capabilities: {error}"); failed = true; }
    }

    if !quick && !failed {
        println!("Running a real light-search end-to-end probe...");
        let runner = ResearchRunner::new(hermes, &settings);
        match runner.light_search(LightSearchArgs {
            question: "What is the official homepage of the Rust programming language?".into(),
            context: Some("This is a connectivity test; return the authoritative official site.".into()),
            time_scope: None,
            source_constraints: Some("Use the official Rust project source.".into()),
        }).await {
            Ok(answer) if !answer.trim().is_empty() => println!("[ok] end-to-end research run"),
            Ok(_) => { println!("[FAIL] end-to-end research returned an empty response"); failed = true; }
            Err(error) => { println!("[FAIL] end-to-end research: {error}"); failed = true; }
        }
    }

    if manifest.is_none() { println!("[warn] install manifest missing"); }
    if failed { return Err(SearchError::Install("one or more doctor checks failed".into())); }
    println!("READY");
    Ok(())
}

fn check(label: &str, ok: bool, failed: &mut bool) {
    if ok { println!("[ok] {label}"); } else { println!("[FAIL] {label}"); *failed = true; }
}
