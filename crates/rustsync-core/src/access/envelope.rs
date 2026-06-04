use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use hkdf::Hkdf;
use rand::RngCore;
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::{
    device::{DeviceIdentity, DeviceRecord},
    workspace::WORKSPACE_KEY_SIZE,
};

use super::{AccessError, AccessResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyEnvelope {
    pub workspace_id: String,
    pub device_id: String,
    pub key_id: String,

    pub algorithm: EnvelopeAlgorithm,

    pub sender_ephemeral_public_key: [u8; 32],
    pub nonce: [u8; 24],

    pub encrypted_workspace_key: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EnvelopeAlgorithm {
    X25519HkdfSha256XChaCha20Poly1305,
}

impl KeyEnvelope {
    pub fn encrypt_for_device(
        workspace_id: impl Into<String>,
        device_id: impl Into<String>,
        key_id: impl Into<String>,
        workspace_key: &[u8],
        recipient_exchange_public_key: [u8; 32],
    ) -> AccessResult<Self> {
        if workspace_key.len() != WORKSPACE_KEY_SIZE {
            return Err(AccessError::InvalidWorkspaceKeyLength {
                expected: WORKSPACE_KEY_SIZE,
                actual: workspace_key.len(),
            });
        }

        let workspace_id = workspace_id.into();
        let device_id = device_id.into();
        let key_id = key_id.into();
        let algorithm = EnvelopeAlgorithm::X25519HkdfSha256XChaCha20Poly1305;

        let sender_secret = StaticSecret::random_from_rng(OsRng);
        let sender_public = PublicKey::from(&sender_secret);

        let recipient_public = PublicKey::from(recipient_exchange_public_key);
        let shared_secret = sender_secret.diffie_hellman(&recipient_public);

        let envelope_key = derive_envelope_key(
            shared_secret.as_bytes(),
            &workspace_id,
            &device_id,
            &key_id,
            algorithm,
        )?;

        let cipher = XChaCha20Poly1305::new_from_slice(&envelope_key)
            .map_err(|_| AccessError::EnvelopeEncryptionFailed)?;

        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce);

        let aad = envelope_aad(&workspace_id, &device_id, &key_id, algorithm);

        let encrypted_workspace_key = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: workspace_key,
                    aad: &aad,
                },
            )
            .map_err(|_| AccessError::EnvelopeEncryptionFailed)?;

        Ok(Self {
            workspace_id,
            device_id,
            key_id,
            algorithm,
            sender_ephemeral_public_key: sender_public.to_bytes(),
            nonce,
            encrypted_workspace_key,
        })
    }

    pub fn encrypt_for_device_record(
        workspace_id: impl Into<String>,
        key_id: impl Into<String>,
        workspace_key: &[u8],
        recipient: &DeviceRecord,
    ) -> AccessResult<Self> {
        Self::encrypt_for_device(
            workspace_id,
            recipient.device_id.clone(),
            key_id,
            workspace_key,
            recipient.exchange_public_key,
        )
    }

    pub fn decrypt_for_device(
        &self,
        recipient: &DeviceIdentity,
    ) -> AccessResult<[u8; WORKSPACE_KEY_SIZE]> {
        if self.device_id != recipient.device_id {
            return Err(AccessError::WrongEnvelopeDevice {
                expected: recipient.device_id.clone(),
                actual: self.device_id.clone(),
            });
        }

        if self.algorithm != EnvelopeAlgorithm::X25519HkdfSha256XChaCha20Poly1305 {
            return Err(AccessError::UnsupportedEnvelopeAlgorithm);
        }

        let recipient_secret = StaticSecret::from(recipient.exchange_private_key);
        let sender_public = PublicKey::from(self.sender_ephemeral_public_key);

        let shared_secret = recipient_secret.diffie_hellman(&sender_public);

        let envelope_key = derive_envelope_key(
            shared_secret.as_bytes(),
            &self.workspace_id,
            &self.device_id,
            &self.key_id,
            self.algorithm,
        )?;

        let cipher = XChaCha20Poly1305::new_from_slice(&envelope_key)
            .map_err(|_| AccessError::EnvelopeDecryptionFailed)?;

        let aad = envelope_aad(
            &self.workspace_id,
            &self.device_id,
            &self.key_id,
            self.algorithm,
        );

        let decrypted = cipher
            .decrypt(
                XNonce::from_slice(&self.nonce),
                Payload {
                    msg: &self.encrypted_workspace_key,
                    aad: &aad,
                },
            )
            .map_err(|_| AccessError::EnvelopeDecryptionFailed)?;

        decrypted
            .try_into()
            .map_err(|bytes: Vec<u8>| AccessError::InvalidWorkspaceKeyLength {
                expected: WORKSPACE_KEY_SIZE,
                actual: bytes.len(),
            })
    }
}

fn derive_envelope_key(
    shared_secret: &[u8; 32],
    workspace_id: &str,
    device_id: &str,
    key_id: &str,
    algorithm: EnvelopeAlgorithm,
) -> AccessResult<[u8; 32]> {
    let salt = b"salt-dfsafafsafasf";

    let hkdf = Hkdf::<Sha256>::new(Some(salt), shared_secret);

    let info = envelope_aad(workspace_id, device_id, key_id, algorithm);

    let mut output_key = [0u8; 32];

    hkdf.expand(&info, &mut output_key)
        .map_err(|_| AccessError::KeyDerivationFailed)?;

    Ok(output_key)
}

fn envelope_aad(
    workspace_id: &str,
    device_id: &str,
    key_id: &str,
    algorithm: EnvelopeAlgorithm,
) -> Vec<u8> {
    let algorithm_name = match algorithm {
        EnvelopeAlgorithm::X25519HkdfSha256XChaCha20Poly1305 => {
            "x25519-hkdf-sha256-xchacha20poly1305"
        }
    };

    [
        "rustysync-key-envelope",
        algorithm_name,
        workspace_id,
        device_id,
        key_id,
    ]
    .join("\0")
    .into_bytes()
}
