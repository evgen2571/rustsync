use std::{io, path::PathBuf};
use thiserror::Error;

use rustsync_protocol::{DeviceId, KeyId, ProtocolError, WorkspaceId};

pub type Result<T> = std::result::Result<T, RustsyncError>;

pub type KeyringResult<T> = std::result::Result<T, KeyringError>;
pub type WorkspaceResult<T> = std::result::Result<T, WorkspaceError>;
pub type EncryptionResult<T> = std::result::Result<T, EncryptionError>;
pub type ManifestResult<T> = std::result::Result<T, ManifestError>;
pub type DeviceResult<T> = std::result::Result<T, DeviceError>;
pub type AccessResult<T> = std::result::Result<T, AccessError>;

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
    Protocol(#[from] ProtocolError),

    #[error(transparent)]
    Device(#[from] DeviceError),

    #[error(transparent)]
    Keyring(#[from] KeyringError),

    #[error(transparent)]
    Access(#[from] AccessError),

    #[error("failed to serialize TOML")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("failed to deserialize TOML")]
    TomlDeserialize(#[from] toml::de::Error),
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace I/O error: {0}")]
    Io(#[from] io::Error),

    #[error(transparent)]
    Keyring(#[from] KeyringError),

    #[error(transparent)]
    Encryption(#[from] EncryptionError),

    #[error(transparent)]
    Protocol(#[from] ProtocolError),

    #[error(transparent)]
    Device(#[from] DeviceError),

    #[error(transparent)]
    Access(#[from] AccessError),

    #[error("workspace is already initialized at `{path}`")]
    AlreadyInitialized { path: PathBuf },

    #[error("No RustSync workspace found at `{path}`. Run `rustsync init` first.")]
    NotInitialized { path: PathBuf },

    #[error("workspace key not found: `{key_id}`")]
    KeyNotFound { key_id: KeyId },

    #[error("invalid workspace key id: `{key_id}`")]
    InvalidKeyId { key_id: KeyId },

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
    #[error(transparent)]
    Protocol(#[from] ProtocolError),

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

    #[error(transparent)]
    Protocol(#[from] ProtocolError),

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

    #[error("manfiest file modified time is before the unix epoch: `path`")]
    InvalidModifiedTime { path: PathBuf },

    #[error(
        "manifest belongs to workspace `{manifest_workspace_id}`, but current workspace is `{current_workspace_id}`"
    )]
    WorkspaceIdMismatch {
        manifest_workspace_id: WorkspaceId,
        current_workspace_id: WorkspaceId,
    },
}

#[derive(Debug, Error)]
pub enum KeyringError {
    #[error("keyring I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("failed to serialize keyring metadata")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("failed to deserialize keyring metadata")]
    TomlDeserialize(#[from] toml::de::Error),

    #[error("invalid key id: {key_id}")]
    InvalidKeyId { key_id: KeyId },

    #[error("key already exists: {key_id}")]
    KeyAlreadyExists { key_id: KeyId },

    #[error("key not found: {key_id}")]
    KeyNotFound { key_id: KeyId },

    #[error("invalid key size at {path}: expected {expected} bytes, got {actual} bytes")]
    InvalidKeySize {
        path: PathBuf,
        expected: usize,
        actual: usize,
    },
}

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error(transparent)]
    Protocol(#[from] ProtocolError),

    #[error("device I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("failed to serialize device metadata")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("failed to deserialize device metadata")]
    TomlDeserialize(#[from] toml::de::Error),

    #[error("device already exists: {0}")]
    AlreadyExists(DeviceId),

    #[error("unknown device: {0}")]
    UnknownDevice(DeviceId),

    #[error("signing private key does not match the stored public key")]
    SigningKeyMismatch,

    #[error("exchange private key does not match the stored public key")]
    ExchangeKeyMismatch,

    #[error("invalid device name: {device_name}")]
    InvalidDeviceName { device_name: String },

    #[error("invalid device id: {device_id}")]
    InvalidDeviceId { device_id: DeviceId },

    #[error("device is revoked: {device_id}")]
    DeviceRevoked { device_id: DeviceId },

    #[error("device is pending: {device_id}")]
    DevicePending { device_id: DeviceId },

    #[error("device fingerprint mismatch: expected `{expected}`, got `{actual}`")]
    FingerprintMismatch { expected: String, actual: String },

    #[error("workspace id mismatch: expected {expected}, got {actual}")]
    WorkspaceIdMismatch { expected: String, actual: String },

    #[error("invalid device join request: {0}")]
    InvalidJoinRequest(String),

    #[error("invalid device public key")]
    InvalidPublicKey,

    #[error("invalid device signature")]
    InvalidSignature,
}

#[derive(Debug, Error)]
pub enum AccessError {
    #[error("access I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("failed to serialize access metadata")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("failed to deserialize access metadata")]
    TomlDeserialize(#[from] toml::de::Error),

    #[error(transparent)]
    Device(#[from] DeviceError),

    #[error(transparent)]
    Protocol(#[from] ProtocolError),

    #[error(transparent)]
    Keyring(#[from] KeyringError),

    #[error("device `{device_id}` is not an active workspace member")]
    DeviceNotActiveMember { device_id: DeviceId },

    #[error("invalid workspace id")]
    InvalidWorkspaceId,

    #[error(
        "envelope access revision {envelope_revision} is newer than local revision {local_revision}"
    )]
    AccessStateTooOld {
        local_revision: u64,
        envelope_revision: u64,
    },

    #[error(
        "key generation mismatch for `{key_id}`: \
         expected {expected}, got {actual}"
    )]
    KeyGenerationMismatch {
        key_id: KeyId,
        expected: u64,
        actual: u64,
    },

    #[error(
        "shared key `{key_id}` uses implicit access \
         for all active workspace members"
    )]
    SharedKeyUsesImplicitAccess { key_id: KeyId },

    #[error("workspace mismatch: expected `{expected}`, got `{actual}`")]
    WorkspaceMismatch {
        expected: WorkspaceId,
        actual: WorkspaceId,
    },

    #[error(
        "device identity conflicts with the registered \
         public keys: {device_id}"
    )]
    DeviceIdentityConflict { device_id: DeviceId },

    #[error("device is already in workspace ACL: {0}")]
    DeviceAlreadyAllowed(DeviceId),

    #[error("unsupported key envelope algorithm")]
    UnsupportedEnvelopeAlgorithm,

    #[error(
        "key envelope belongs to another recipient: \
         expected {expected}, got {actual}"
    )]
    WrongEnvelopeRecipient {
        expected: DeviceId,
        actual: DeviceId,
    },

    #[error(
        "key envelope claims another sender: \
         expected {expected}, got {actual}"
    )]
    WrongEnvelopeSender {
        expected: DeviceId,
        actual: DeviceId,
    },

    #[error("invalid key-envelope signature")]
    InvalidEnvelopeSignature,

    #[error(
        "invalid workspace key length: expected {expected}, \
         got {actual} bytes"
    )]
    InvalidWorkspaceKeyLength { expected: usize, actual: usize },

    #[error("key-envelope encryption failed")]
    EnvelopeEncryptionFailed,

    #[error("key-envelope decryption failed")]
    EnvelopeDecryptionFailed,

    #[error("key-envelope key derivation failed")]
    KeyDerivationFailed,
}
