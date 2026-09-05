use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("invalid configuration: {0}")]
    Config(String),

    #[error("invalid research brief: {0}")]
    InvalidInput(String),

    #[error("Hermes HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Hermes API returned HTTP {status}: {body}")]
    HermesHttp { status: u16, body: String },

    #[error("Hermes protocol error: {0}")]
    HermesProtocol(String),

    #[error("Hermes research run failed: {0}")]
    HermesRun(String),

    #[error("research timed out after {seconds}s")]
    Timeout { seconds: u64 },

    #[error("command failed: {program} (exit {code:?})\n{stderr}")]
    Command {
        program: String,
        code: Option<i32>,
        stderr: String,
    },

    #[error("installation error: {0}")]
    Install(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
