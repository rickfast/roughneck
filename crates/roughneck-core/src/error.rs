use thiserror::Error;

#[derive(Debug, Error)]
pub enum RoughneckError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("runtime failure: {0}")]
    Runtime(String),
}

impl From<std::io::Error> for RoughneckError {
    fn from(value: std::io::Error) -> Self {
        Self::Runtime(value.to_string())
    }
}

impl From<serde_json::Error> for RoughneckError {
    fn from(value: serde_json::Error) -> Self {
        Self::InvalidInput(value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, RoughneckError>;
