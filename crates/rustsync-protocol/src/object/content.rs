use crate::{KeyId, ProtocolError, ProtocolResult};

pub const XCHACHA20_POLY1305_NONCE_SIZE: usize = 24;
pub const XCHACHA20_POLY1305_TAG_SIZE: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedObject {
    pub key_id: KeyId,
    pub algorithm: ContentEncryptionAlgorithm,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentEncryptionAlgorithm {
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
                if self.ciphertext.len() < XCHACHA20_POLY1305_TAG_SIZE {
                    return Err(ProtocolError::CiphertextTooShort {
                        minimum: XCHACHA20_POLY1305_TAG_SIZE,
                        actual: self.ciphertext.len(),
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
        if !bytes.starts_with(super::binary::MAGIC) {
            return Err(ProtocolError::InvalidEncryptedObjectEncoding);
        }

        Self::from_binary_bytes(bytes)
    }
}
