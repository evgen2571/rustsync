use crate::error::EncryptionError;
use rand::RngCore;

#[derive(Debug)]
pub struct EncryptedFile {
    pub key_id: String,
    pub nonce: [u8; 12],
    pub encrypted_data: Vec<u8>,
}

pub fn encrypt(plain_data: Vec<u8>, key_id: &str) -> Result<EncryptedFile, EncryptionError> {
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);

    let encrypted_data = plain_data;

    Ok(EncryptedFile {
        key_id: key_id.to_string(),
        nonce,
        encrypted_data,
    })
}
