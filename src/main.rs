use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};
use hermes_search_agent::{
    config::Settings,
    doctor,
    hermes::HermesClient,
    install::{self, InstallOptions},
    paths::{AppPaths, DEFAULT_DSH_PROFILE},
    research::ResearchRunner,
    server,
    status,
    update,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Parser)]
#[command(name = "hermes-search-agent", version, about = "Hermes-backed MCP research agent")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Provision Hermes, the dedicated research profile, systemd services, and DSH MCP integration.
    Install {
        #[arg(long)] dry_run: bool,
        #[arg(long)] non_interactive: bool,
        #[arg(long, default_value = DEFAULT_DSH_PROFILE)] dsh_profile: String,
        /// Disable DSH's native web_search without prompting.
        #[arg(long, conflicts_with = "keep_dsh_web_search")]
        disable_dsh_web_search: bool,
        /// Keep DSH's native web_search without prompting.
        #[arg(long, conflicts_with = "disable_dsh_web_search")]
        keep_dsh_web_search: bool,
        /// Disable DSH's native web_fetch without prompting.
        #[arg(long, conflicts_with = "keep_dsh_web_fetch")]
        disable_dsh_web_fetch: bool,
        /// Keep DSH's native web_fetch without prompting.
        #[arg(long, conflicts_with = "disable_dsh_web_fetch")]
        keep_dsh_web_fetch: bool,
    },
    /// Run the Streamable HTTP MCP server.
    Serve,
    /// Show installed component and service state without changing anything.
    Status,
    /// Verify the complete installation. By default includes a real light research query.
    Doctor { #[arg(long)] quick: bool },
    /// Reconcile the managed Hermes profile, services, DSH patch, and native-web policy with the install manifest.
    Repair { #[arg(long)] non_interactive: bool },
    /// Update from the latest GitHub release and repair the installation.
    Update { #[arg(long)] check: bool },
    /// Remove DSH integration and services. --purge also deletes owned profile/config/state.
    Uninstall { #[arg(long)] purge: bool },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hermes_search_agent=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Install {
            dry_run,
            non_interactive,
            dsh_profile,
            disable_dsh_web_search,
            keep_dsh_web_search,
            disable_dsh_web_fetch,
            keep_dsh_web_fetch,
        } => {
            let dsh_web_search = if disable_dsh_web_search { Some(false) } else if keep_dsh_web_search { Some(true) } else { None };
            let dsh_web_fetch = if disable_dsh_web_fetch { Some(false) } else if keep_dsh_web_fetch { Some(true) } else { None };
            install::install(InstallOptions { dry_run, non_interactive, dsh_profile, dsh_web_search, dsh_web_fetch }).await?;
        }
        Command::Serve => {
            let paths = AppPaths::discover().context("failed to resolve application paths")?;
            let settings = Settings::load(&paths).context("failed to load configuration")?;
            let hermes = HermesClient::new(&settings.hermes_url, &settings.hermes_api_key);
            // Deliberately do not capability-probe here: the MCP service stays online while
            // Hermes restarts. Calls surface a temporary Hermes error and recover naturally.
            let runner = Arc::new(ResearchRunner::new(hermes, &settings));
            server::serve(&settings, runner).await?;
        }
        Command::Status => status::run()?,
        Command::Doctor { quick } => doctor::run(quick).await?,
        Command::Repair { non_interactive } => install::repair(non_interactive).await?,
        Command::Update { check } => update::run(check).await?,
        Command::Uninstall { purge } => install::uninstall(purge)?,
    }
    Ok(())
}
