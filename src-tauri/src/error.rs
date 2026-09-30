use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("yt-dlp failed: {0}")]
    YtDlp(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid subtitle: {0}")]
    Subtitle(String),
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("Claude API error: {0}")]
    Api(String),
    #[error("translation failed: {0}")]
    Translation(String),
    #[error("translation was cut off at the output limit")]
    Truncated,
    #[error("unsupported target language: {0}")]
    UnknownLanguage(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("cannot read the login shell environment: {0}")]
    Environment(String),
}

impl AppError {
    /// Malformed or misaligned model output may succeed on a fresh attempt; transport
    /// retries (429/5xx) happen inside the API client instead.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Translation(_))
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
