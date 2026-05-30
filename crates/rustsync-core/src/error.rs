use serde::de::value::Error;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EncError {
    #[error("Serialization failed: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}
