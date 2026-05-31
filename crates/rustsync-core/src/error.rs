use std::fmt;

#[derive(Debug)]
pub enum EncryptionError {
    Io(std::io::Error),
    KeyNotFound { key_id: String },
    InvalidNonceLenth { expected: usize, actual: usize },
    EncryptionFailed,
}

impl fmt::Display for EncryptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncryptionError::Io(err) => {
                write!(f, "I/O error: {}", err)
            }

            EncryptionError::InvalidNonceLenth { expected, actual } => {
                write!(
                    f,
                    "Invalid nonce length: expected {} bytes, got {} bytes",
                    expected, actual,
                )
            }

            EncryptionError::KeyNotFound { key_id } => {
                write!(f, "Encryption key not found: {}", key_id)
            }

            EncryptionError::EncryptionFailed => {
                write!(f, "Encryption failed")
            }
        }
    }
}

impl From<std::io::Error> for EncryptionError {
    fn from(error: std::io::Error) -> Self {
        EncryptionError::Io(error)
    }
}
