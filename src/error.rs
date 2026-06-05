//! Unified error types.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Base64 error: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("CSV parse error: {0}")]
    Csv(#[from] csv::Error),

    #[error("Crypto error: {0}")]
    Crypto(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Server not supported: {0}")]
    UnsupportedServer(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Version parse error: {0}")]
    Version(String),
}

/// Convenience alias for Result<T, Error>.
pub type Result<T> = std::result::Result<T, Error>;
