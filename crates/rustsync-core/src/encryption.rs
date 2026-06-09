use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rustsync_protocol::KeyId;
use serde::{Deserialize, Serialize};

use crate::{
    error::{EncryptionError, EncryptionResult},
    keyring::WorkspaceKey,
};

const AES_GCM_NONCE_SIZE: usize = 12;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EncryptedFile {
    pub key_id: KeyId,
    pub nonce: String,
    pub encrypted_data: Vec<u8>,
}

pub fn encrypt(
    plaintext: &[u8],
    key_id: &KeyId,
    key: &WorkspaceKey,
) -> EncryptionResult<EncryptedFile> {
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let encrypter = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.expose_secret()));

    let encrypted_data = encrypter
        .encrypt(&nonce, plaintext)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    Ok(EncryptedFile {
        key_id: key_id.clone(),
        nonce: bytes_to_base64(&nonce),
        encrypted_data,
    })
}

pub fn decrypt(encrypted_file: &EncryptedFile, key: &[u8; 32]) -> EncryptionResult<Vec<u8>> {
    let nonce_bytes = base64_to_bytes(&encrypted_file.nonce)?;

    if nonce_bytes.len() != AES_GCM_NONCE_SIZE {
        return Err(EncryptionError::InvalidNonceLength {
            expected: AES_GCM_NONCE_SIZE,
            actual: nonce_bytes.len(),
        });
    }

    let nonce = Nonce::from_slice(&nonce_bytes);
    let decrypter = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));

    let decrypted_text = decrypter
        .decrypt(nonce, encrypted_file.encrypted_data.as_ref())
        .map_err(|_| EncryptionError::DecryptionFailed)?;

    Ok(decrypted_text)
}

pub fn bytes_to_base64(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

pub fn base64_to_bytes(base64_str: &str) -> EncryptionResult<Vec<u8>> {
    Ok(BASE64.decode(base64_str)?)
}
