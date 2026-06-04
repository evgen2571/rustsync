use std::{io, path::PathBuf};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, RustsyncError>;

pub type KeyringResult<T> = std::result::Result<T, KeyringError>;
pub type WorkspaceResult<T> = std::result::Result<T, WorkspaceError>;
pub type EncryptionResult<T> = std::result::Result<T, EncryptionError>;
pub type ManifestResult<T> = std::result::Result<T, ManifestError>;
pub type DeviceResult<T> = std::result::Result<T, DeviceError>;

#[derive(Debug, Error)]
pub enum RustsyncError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Workspace(#[from] WorkspaceError),

    #[error(transparent)]
    Encryption(#[from] EncryptionError),

    #[error(transparent)]
    Manifest(#[from] ManifestError),

    #[error(transparent)]
    Device(#[from] DeviceError),

    #[error(transparent)]
    Keyring(#[from] KeyringError),

    #[error("failed to serialize TOML")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("failed to deserialize TOML")]
    TomlDeserialize(#[from] toml::de::Error),
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("workspace is already initialized at `{path}`")]
    AlreadyInitialized { path: PathBuf },

    #[error("workspace is not initialized at `{path}`")]
    NotInitialized { path: PathBuf },

    #[error("workspace key not found: `{key_id}`")]
    KeyNotFound { key_id: String },

    #[error("invalid workspace key id: `{key_id}`")]
    InvalidKeyId { key_id: String },

    #[error("failed to serialize workspace config: {0}")]
    ConfigSerialize(#[from] toml::ser::Error),

    #[error("failed to parse workspace config: {0}")]
    ConfigDeserialize(#[from] toml::de::Error),

    #[error(
        "invalid workspace key size at `{path}`: expected {expected} bytes, got {actual} bytes"
    )]
    InvalidKeySize {
        path: PathBuf,
        expected: usize,
        actual: usize,
    },
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

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("manifest I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("failed to walk workspace while building manfiest: {0}")]
    WalkDir(#[from] walkdir::Error),

    #[error("failed to serialize manifest to JSON: {source}")]
    Serialize { source: serde_json::Error },

    #[error("failed to deserialize manifest from JSON: {source}")]
    Deserialize { source: serde_json::Error },

    #[error("manifest path contains invalid utf-8: `{path}`")]
    InvalidUtf8Path { path: PathBuf },

    #[error("manifest path must be relative and normalized: `{path}`")]
    InvalidRelativePath { path: PathBuf },

    #[error("failed to strip workspace root `{root}` from path `{path}`")]
    StripRootError { root: PathBuf, path: PathBuf },

    #[error(
        "manifest belongs to workspace `{manifest_workspace_id}`, but current workspace is `{current_workspace_id}`"
    )]
    WorkspaceIdMismatch {
        manifest_workspace_id: String,
        current_workspace_id: String,
    },
}

#[derive(Debug, Error)]
pub enum KeyringError {
    #[error("keyring I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("invalid key id: {key_id}")]
    InvalidKeyId { key_id: String },

    #[error("key already exists: {key_id}")]
    KeyAlreadyExists { key_id: String },

    #[error("key not found: {key_id}")]
    KeyNotFound { key_id: String },

    #[error("invalid key size at {path}: expected {expected} bytes, got {actual} bytes")]
    InvalidKeySize {
        path: PathBuf,
        expected: usize,
        actual: usize,
    },
    #[error("failed to serialize keyring metadata")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("failed to deserialize keyring metadata")]
    TomlDeserialize(#[from] toml::de::Error),
}

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("device already exists: {0}")]
    AlreadyExists(String),

    #[error("unknown device: {0}")]
    UnknownDevice(String),

    #[error("invalid device public key")]
    InvalidPublicKey,

    #[error("invalid device signature")]
    InvalidSignature,
}
