use crate::error::EncryptionError::{self, DecodeError, EncryptionFailed};
use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct EncryptedFile {
    pub key_id: String,
    pub nonce: String,
    pub encrypted_data: Vec<u8>,
}

pub fn encrypt(
    plaintext: Vec<u8>,
    key_id: &str,
    key: &[u8; 32],
) -> Result<EncryptedFile, EncryptionError> {
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let encrypter = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));

    let encrypted_data = encrypter
        .encrypt(&nonce, plaintext.as_ref())
        .map_err(|_| EncryptionFailed)?;

    Ok(EncryptedFile {
        key_id: key_id.to_string(),
        nonce: bytes_to_base64(&nonce),
        encrypted_data,
    })
}

pub fn decrypt(encrypted_file: EncryptedFile, key: &[u8; 32]) -> Result<Vec<u8>, EncryptionError> {
    let nonce_bytes = base64_to_bytes(&encrypted_file.nonce)?;

    if nonce_bytes.len() != 12 {
        return Err(DecodeError);
    }

    let nonce = Nonce::from_slice(&nonce_bytes);
    let decrypter = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));

    let decrypted_text = decrypter
        .decrypt(nonce, encrypted_file.encrypted_data.as_ref())
        .map_err(|_| DecodeError)?;

    Ok(decrypted_text)
}

pub fn bytes_to_base64(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

pub fn base64_to_bytes(base64_str: &str) -> Result<Vec<u8>, EncryptionError> {
    BASE64
        .decode(base64_str)
        .map_err(|e| EncryptionError::Base64Error(e.to_string()))
}
