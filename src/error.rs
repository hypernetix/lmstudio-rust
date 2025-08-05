//! Error types for the LM Studio Rust client

use thiserror::Error;

/// Result type alias for LM Studio operations
pub type Result<T> = std::result::Result<T, LMStudioError>;

/// Error types that can occur when using the LM Studio client
#[derive(Error, Debug)]
pub enum LMStudioError {
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Model already loaded: {0}")]
    ModelAlreadyLoaded(String),

    #[error("Model loading timeout: {0}")]
    LoadingTimeout(String),

    #[error("Invalid namespace: {0}")]
    InvalidNamespace(String),

    #[error("Remote call failed: {0}")]
    RemoteCallFailed(String),

    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Invalid response format: {0}")]
    InvalidResponse(String),

    #[error("Operation cancelled")]
    Cancelled,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("Other error: {0}")]
    Other(String),
}

impl LMStudioError {
    /// Create a new connection error
    pub fn connection<S: Into<String>>(msg: S) -> Self {
        Self::Connection(msg.into())
    }

    /// Create a new model not found error
    pub fn model_not_found<S: Into<String>>(model: S) -> Self {
        Self::ModelNotFound(model.into())
    }

    /// Create a new remote call failed error
    pub fn remote_call_failed<S: Into<String>>(msg: S) -> Self {
        Self::RemoteCallFailed(msg.into())
    }

    /// Create a new service unavailable error
    pub fn service_unavailable<S: Into<String>>(msg: S) -> Self {
        Self::ServiceUnavailable(msg.into())
    }

    /// Create a new invalid response error
    pub fn invalid_response<S: Into<String>>(msg: S) -> Self {
        Self::InvalidResponse(msg.into())
    }

    /// Create a new other error
    pub fn other<S: Into<String>>(msg: S) -> Self {
        Self::Other(msg.into())
    }
}
