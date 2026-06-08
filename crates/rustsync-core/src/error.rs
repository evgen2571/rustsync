use std::{io, path::PathBuf};
use thiserror::Error;

use rustsync_protocol::{DeviceId, ProtocolError};

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

    #[error("invalid workspace id")]
    InvalidWorkspaceId,

    #[error("device cannot perform this access operation: {0}")]
    PermissionDenied(DeviceId),

    #[error(
        "local access state is older than the envelope: \
         local={local_revision}, envelope={envelope_revision}"
    )]
    AccessControlTooOld {
        local_revision: u64,
        envelope_revision: u64,
    },

    #[error(
        "key generation mismatch for `{key_id}`: \
         expected {expected}, got {actual}"
    )]
    KeyGenerationMismatch {
        key_id: String,
        expected: u64,
        actual: u64,
    },

    #[error("workspace id mismatch: expected {expected}, got {actual}")]
    WorkspaceIdMismatch { expected: String, actual: String },

    #[error(
        "shared key `{key_id}` uses implicit access \
         for all active workspace members"
    )]
    SharedKeyUsesImplicitAccess { key_id: String },

    #[error("access-control revision overflow")]
    RevisionOverflow,

    #[error(
        "device identity conflicts with the registered \
         public keys: {0}"
    )]
    DeviceIdentityConflict(DeviceId),

    #[error("device is already in workspace ACL: {0}")]
    DeviceAlreadyAllowed(DeviceId),

    #[error("unsupported key envelope algorithm")]
    UnsupportedEnvelopeAlgorithm,

    #[error("cannot remove last owner")]
    CannotRemoveLastOwner,

    #[error("device is already a workspace member: {0}")]
    DeviceAlreadyMember(DeviceId),

    #[error("device is not a workspace member: {0}")]
    DeviceNotMember(DeviceId),

    #[error(
        "device is not authorized for key: \
         key={key_id}, device={device_id}"
    )]
    DeviceNotAuthorizedForKey { key_id: String, device_id: DeviceId },

    #[error(
        "key access is already granted: \
         key={key_id}, device={device_id}"
    )]
    KeyAccessAlreadyGranted { key_id: String, device_id: DeviceId },

    #[error(
        "key access is not granted: \
         key={key_id}, device={device_id}"
    )]
    KeyAccessNotGranted { key_id: String, device_id: DeviceId },

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
