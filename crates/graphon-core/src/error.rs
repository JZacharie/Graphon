use thiserror::Error;

#[derive(Error, Debug)]
pub enum GraphonError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Gmail API error: {0}")]
    Gmail(String),

    #[error("Classifier error: {0}")]
    Classifier(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, GraphonError>;
