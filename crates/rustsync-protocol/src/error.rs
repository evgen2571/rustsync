use thiserror::Error;

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

    #[error("invalid nonce length: expected {expected} bytes, got {actual}")]
    InvalidNonceLength { expected: usize, actual: usize },

    #[error("time is before the unix epoch")]
    TimeBeforeUnixEpoch,

    #[error("invalid access event: {0}")]
    InvalidAccessEvent(String),

    #[error("invalid manifest path `{path}`: {reason}")]
    InvalidManifestPath { path: String, reason: String },
}
