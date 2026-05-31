use crate::error::EncryptionError::{self, DecodeError, EncryptionFailed};
use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

#[derive(Debug)]
pub struct EncryptedFile {
    pub key_id: String,
    pub nonce: String,
    pub encrypted_data: Vec<u8>,
}

pub fn encrypt(plain_data: Vec<u8>, key_id: &str) -> Result<EncryptedFile, EncryptionError> {
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let key: &[u8; 32] = &[
        142, 23, 199, 84, 11, 201, 45, 178, 93, 255, 12, 67, 184, 39, 90, 212, 5, 131, 74, 162, 89,
        41, 117, 3, 168, 54, 190, 22, 135, 77, 241, 106,
    ];

    let encrypter = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));

    let encrypted_data = encrypter
        .encrypt(&nonce, plain_data.as_ref())
        .map_err(|_| EncryptionFailed)?;

    Ok(EncryptedFile {
        key_id: key_id.to_string(),
        nonce: bytes_to_base64(&nonce),
        encrypted_data,
    })
}

pub fn decrypt(encrypted_file: EncryptedFile) -> Result<Vec<u8>, EncryptionError> {
    let nonce_bytes = base64_to_bytes(&encrypted_file.nonce)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    if nonce.len() != 12 {
        return Err(DecodeError);
    }

    let key: &[u8; 32] = &[
        142, 23, 199, 84, 11, 201, 45, 178, 93, 255, 12, 67, 184, 39, 90, 212, 5, 131, 74, 162, 89,
        41, 117, 3, 168, 54, 190, 22, 135, 77, 241, 106,
    ];
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
