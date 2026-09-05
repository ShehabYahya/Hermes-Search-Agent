use std::sync::Arc;

use axum::{Router, routing::get};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;

use crate::{config::Settings, error::SearchError, research::ResearchRunner, tools_mcp::SearchMcp};

pub async fn serve(settings: &Settings, runner: Arc<ResearchRunner>) -> Result<(), SearchError> {
    let cancellation = CancellationToken::new();
    let handler = SearchMcp::new(runner);

    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_cancellation_token(cancellation.child_token()),
    );

    let router = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(settings.bind).await?;
    tracing::info!(bind = %settings.bind, endpoint = "/mcp", "MCP server listening");

    axum::serve(listener, router)
        .with_graceful_shutdown({
            let cancellation = cancellation.clone();
            async move {
                let _ = tokio::signal::ctrl_c().await;
                cancellation.cancel();
            }
        })
        .await?;

    Ok(())
}
