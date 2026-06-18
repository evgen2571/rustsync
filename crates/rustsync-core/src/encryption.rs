use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chacha20poly1305::{
    Key, XChaCha20Poly1305, XNonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use rustsync_protocol::{
    ContentEncryptionAlgorithm, EncryptedObject, KeyId, XCHACHA20_POLY1305_NONCE_SIZE,
};

use crate::{
    error::{EncryptionError, EncryptionResult},
    keyring::WorkspaceKey,
};

pub fn encrypt(
    plaintext: &[u8],
    key_id: &KeyId,
    key: &WorkspaceKey,
) -> EncryptionResult<EncryptedObject> {
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let encrypter = XChaCha20Poly1305::new(Key::from_slice(key.expose_secret()));

    let ciphertext = encrypter
        .encrypt(&nonce, plaintext)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    let encrypted_object = EncryptedObject::new(
        key_id.clone(),
        ContentEncryptionAlgorithm::XChaCha20Poly1305,
        nonce.to_vec(),
        ciphertext,
    );
    encrypted_object.validate()?;

    Ok(encrypted_object)
}

pub fn decrypt(
    encrypted_object: &EncryptedObject,
    key: &WorkspaceKey,
) -> EncryptionResult<Vec<u8>> {
    encrypted_object.validate()?;

    if encrypted_object.nonce.len() != XCHACHA20_POLY1305_NONCE_SIZE {
        return Err(EncryptionError::InvalidNonceLength {
            expected: XCHACHA20_POLY1305_NONCE_SIZE,
            actual: encrypted_object.nonce.len(),
        });
    }

    let nonce = XNonce::from_slice(&encrypted_object.nonce);
    let decrypter = XChaCha20Poly1305::new(Key::from_slice(key.expose_secret()));

    let decrypted_text = decrypter
        .decrypt(nonce, encrypted_object.ciphertext.as_ref())
        .map_err(|_| EncryptionError::DecryptionFailed)?;

    Ok(decrypted_text)
}

pub fn bytes_to_base64(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

pub fn base64_to_bytes(base64_str: &str) -> EncryptionResult<Vec<u8>> {
    Ok(BASE64.decode(base64_str)?)
}
