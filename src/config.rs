use std::{env, net::SocketAddr, time::Duration};

use crate::error::SearchError;

#[derive(Clone, Debug)]
pub struct Settings {
    pub bind: SocketAddr,
    pub hermes_url: String,
    pub hermes_api_key: String,
    pub light_timeout: Duration,
    pub medium_timeout: Duration,
    pub deep_timeout: Duration,
}

impl Settings {
    pub fn from_env() -> Result<Self, SearchError> {
        let bind = env::var("HERMES_SEARCH_MCP_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8932".to_string())
            .parse::<SocketAddr>()
            .map_err(|e| SearchError::Config(format!("invalid HERMES_SEARCH_MCP_BIND: {e}")))?;

        let hermes_url = env::var("HERMES_RESEARCH_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8643".to_string());

        let hermes_api_key = env::var("HERMES_RESEARCH_API_KEY")
            .map_err(|_| SearchError::Config("HERMES_RESEARCH_API_KEY is required".to_string()))?;
        if hermes_api_key.trim().is_empty() {
            return Err(SearchError::Config(
                "HERMES_RESEARCH_API_KEY cannot be empty".to_string(),
            ));
        }

        Ok(Self {
            bind,
            hermes_url,
            hermes_api_key,
            light_timeout: timeout_from_env("HERMES_SEARCH_LIGHT_TIMEOUT_SECS", 30)?,
            medium_timeout: timeout_from_env("HERMES_SEARCH_MEDIUM_TIMEOUT_SECS", 120)?,
            deep_timeout: timeout_from_env("HERMES_SEARCH_DEEP_TIMEOUT_SECS", 360)?,
        })
    }
}

fn timeout_from_env(name: &str, default_seconds: u64) -> Result<Duration, SearchError> {
    let seconds = match env::var(name) {
        Ok(raw) => raw
            .parse::<u64>()
            .map_err(|e| SearchError::Config(format!("invalid {name}: {e}")))?,
        Err(_) => default_seconds,
    };

    if seconds == 0 {
        return Err(SearchError::Config(format!("{name} must be greater than zero")));
    }

    Ok(Duration::from_secs(seconds))
}
