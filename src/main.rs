use std::sync::Arc;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use hermes_search_agent::{
    config::Settings, hermes::HermesClient, research::ResearchRunner, server,
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
    /// Run the Streamable HTTP MCP server.
    Serve,
    /// Verify the configured Hermes API exposes the run capabilities required by the bridge.
    Doctor,
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
    let settings = Settings::from_env().context("failed to load configuration")?;
    let hermes = HermesClient::new(&settings.hermes_url, &settings.hermes_api_key);

    match cli.command {
        Command::Serve => {
            hermes
                .require_research_capabilities()
                .await
                .context("Hermes API is not compatible with the research bridge")?;
            let runner = Arc::new(ResearchRunner::new(hermes, &settings));
            server::serve(&settings, runner).await?;
        }
        Command::Doctor => {
            let caps = hermes.capabilities().await?;
            println!("Hermes URL: {}", settings.hermes_url);
            println!("run_submission: {}", caps.run_submission);
            println!("run_events_sse: {}", caps.run_events_sse);
            println!("run_stop: {}", caps.run_stop);
            if !(caps.run_submission && caps.run_events_sse && caps.run_stop) {
                bail!("Hermes is missing one or more required research-run capabilities");
            }
            println!("Hermes research API contract: OK");
        }
    }

    Ok(())
}
