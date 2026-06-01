use std::{io, path::PathBuf};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, RustsyncError>;

#[derive(Debug, Error)]
pub enum RustsyncError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Workspace(#[from] WorkspaceError),

    #[error(transparent)]
    Encryption(#[from] EncryptionError),
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace is already initialized at `{path}`")]
    AlreadyInitialized { path: PathBuf },

    #[error("workspace is not initialized at `{path}`")]
    NotInitialized { path: PathBuf },

    #[error("workspace key not found: `{key_id}`")]
    KeyNotFound { key_id: String },

    #[error("invalid workspace key id: `{key_id}`")]
    InvalidKeyId { key_id: String },

    #[error("workspace I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("failed to serialize workspace config: {0}")]
    ConfigSerialize(#[from] toml::ser::Error),

    #[error("failed to parse workspace config: {0}")]
    ConfigDeserialize(#[from] toml::de::Error),
}

#[derive(Debug, Error)]
pub enum EncryptionError {
    #[error("invalid nonce length: expected {expected} bytes, got {actual} bytes")]
    InvalidNonceLength { expected: usize, actual: usize },

    #[error("encryption failed")]
    EncryptionFailed,

    #[error("decryption failed")]
    DecryptionFailed,

    #[error("base64 decode failed: {0}")]
    Base64(#[from] base64::DecodeError),
}
