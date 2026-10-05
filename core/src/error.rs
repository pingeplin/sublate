#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("yt-dlp failed: {0}")]
    YtDlp(String),
    #[error("yt-dlp is not installed; download it in Settings (⌘,)")]
    YtDlpMissing,
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
    #[error("the app's bundled tools are unusable: {0}")]
    Toolchain(String),
    #[error("yt-dlp update failed: {0}")]
    Update(String),
}

impl AppError {
    /// Malformed or misaligned model output may succeed on a fresh attempt; transport
    /// retries (429/5xx) happen inside the API client instead.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Translation(_))
    }
}

pub type AppResult<T> = Result<T, AppError>;
