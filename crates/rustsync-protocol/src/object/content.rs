use serde::{Deserialize, Serialize};

use crate::{KeyId, ProtocolError, ProtocolResult};

pub const XCHACHA20_POLY1305_NONCE_SIZE: usize = 24;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptedObject {
    pub key_id: KeyId,
    pub algorithm: ContentEncryptionAlgorithm,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContentEncryptionAlgorithm {
    #[serde(alias = "XChaCha20Poly1305")]
    XChaCha20Poly1305,
}

impl EncryptedObject {
    pub fn new(
        key_id: KeyId,
        algorithm: ContentEncryptionAlgorithm,
        nonce: Vec<u8>,
        ciphertext: Vec<u8>,
    ) -> Self {
        Self {
            key_id,
            algorithm,
            nonce,
            ciphertext,
        }
    }

    pub fn validate(&self) -> ProtocolResult<()> {
        if self.ciphertext.is_empty() {
            return Err(ProtocolError::EmptyCiphertext);
        }

        match self.algorithm {
            ContentEncryptionAlgorithm::XChaCha20Poly1305 => {
                if self.nonce.len() != XCHACHA20_POLY1305_NONCE_SIZE {
                    return Err(ProtocolError::InvalidNonceLength {
                        expected: XCHACHA20_POLY1305_NONCE_SIZE,
                        actual: self.nonce.len(),
                    });
                }
            }
        }

        Ok(())
    }

    pub fn to_binary_bytes(&self) -> ProtocolResult<Vec<u8>> {
        super::binary::encode(self)
    }

    pub fn from_binary_bytes(bytes: &[u8]) -> ProtocolResult<Self> {
        super::binary::decode(bytes)
    }

    pub fn from_remote_bytes(bytes: &[u8]) -> ProtocolResult<Self> {
        if bytes.starts_with(super::binary::MAGIC) {
            return Self::from_binary_bytes(bytes);
        }

        // TODO: Remove this JSON fallback after the documented migration and support window ends.
        let object: Self = serde_json::from_slice(bytes)
            .map_err(|_| ProtocolError::InvalidEncryptedObjectEncoding)?;
        object.validate()?;
        Ok(object)
    }
}
