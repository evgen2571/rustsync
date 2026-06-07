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
}
