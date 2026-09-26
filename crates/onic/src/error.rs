use thiserror::Error;

#[derive(Debug, Error)]
pub enum OnicError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl OnicError {
    pub fn msg(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}

pub type OnicResult<T> = Result<T, OnicError>;
