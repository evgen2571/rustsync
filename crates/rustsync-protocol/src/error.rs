use thiserror::Error;

use crate::{DeviceId, KeyId, WorkspaceId, WorkspacePermission};

pub type ProtocolResult<T> = Result<T, ProtocolError>;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("invalid {kind} identifier `{value}`: {reason}")]
    InvalidIdentifier {
        kind: String,
        value: String,
        reason: String,
    },

    #[error("device name must not be empty")]
    InvalidDeviceName,

    #[error("invalid public signing key")]
    InvalidPublicKey,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("device fingerprint mismatch: expected `{expected}`, got `{actual}`")]
    FingerprintMismatch { expected: String, actual: String },

    #[error("invalid join request: {0}")]
    InvalidJoinRequest(String),

    #[error("workspace mismatch: expected `{expected}`, got `{actual}`")]
    WorkspaceMismatch { expected: String, actual: String },

    #[error("invalid key generation {0}; generations start at 1")]
    InvalidKeyGeneration(u64),

    #[error("encrypted ciphertext must not be empty")]
    EmptyCiphertext,

    #[error("encrypted ciphertext must contain at least {minimum} bytes, got {actual}")]
    CiphertextTooShort { minimum: usize, actual: usize },

    #[error("invalid nonce length: expected {expected} bytes, got {actual}")]
    InvalidNonceLength { expected: usize, actual: usize },

    #[error(
        "encrypted object exceeds the maximum supported size of {maximum} bytes (got {actual})"
    )]
    EncryptedObjectTooLarge { maximum: usize, actual: usize },

    #[error("invalid encrypted object binary format")]
    InvalidEncryptedObjectBinary,

    #[error("unsupported encrypted object binary version {0}")]
    UnsupportedEncryptedObjectVersion(u8),

    #[error("unsupported content encryption algorithm tag {0}")]
    UnsupportedContentEncryptionAlgorithm(u8),

    #[error("invalid encrypted object encoding")]
    InvalidEncryptedObjectEncoding,

    #[error("time is before the unix epoch")]
    TimeBeforeUnixEpoch,

    #[error("invalid access event: {0}")]
    InvalidAccessEvent(String),

    #[error("invalid manifest path `{path}`: {reason}")]
    InvalidManifestPath { path: String, reason: String },

    #[error("invalid request signature")]
    InvalidRequestSignature,

    #[error("invalid authentication header: {0}")]
    InvalidAuthHeader(String),

    // access state errors
    #[error("device `{device_id}` is not an active workspace member")]
    DeviceNotActiveMember { device_id: DeviceId },

    #[error("permission denied for device `{device_id}`; required `{permission:?}`")]
    PermissionDenied {
        device_id: DeviceId,
        permission: WorkspacePermission,
    },

    #[error("device `{device_id}` is not authorized for key `{key_id}` generation {generation}")]
    DeviceNotAuthorizedForKey {
        key_id: KeyId,
        generation: u64,
        device_id: DeviceId,
    },

    #[error("device is already a workspace member: {0}")]
    DeviceAlreadyMember(DeviceId),

    #[error("device is not a workspace member: {0}")]
    DeviceNotMember(DeviceId),

    #[error("workspace id mismatch: expected {expected}, got {actual}")]
    WorkspaceIdMismatch {
        expected: WorkspaceId,
        actual: WorkspaceId,
    },

    #[error("access-control revision overflow")]
    RevisionOverflow,

    #[error("cannot remove last owner")]
    CannotRemoveLastOwner,

    #[error("invalid persisted access state: {0}")]
    InvalidState(String),

    #[error("key `{key_id}` generation {generation} is already granted to device `{device_id}`")]
    KeyAccessAlreadyGranted {
        key_id: KeyId,
        generation: u64,
        device_id: DeviceId,
    },

    #[error("key `{key_id}` generation {generation} is not granted to device `{device_id}`")]
    KeyAccessNotGranted {
        key_id: KeyId,
        generation: u64,
        device_id: DeviceId,
    },
}
