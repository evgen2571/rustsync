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
    keyring::{WORKSPACE_KEY_SIZE, WorkspaceKey},
};

use super::{AccessError, AccessResult};

const ENVELOPE_HKDF_SALT: &[u8] = b"rustsync/key-envelope/hkdf-sha256";
const ENVELOPE_CONTEXT: &[u8] = b"rustsync/key-envelope";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyEnvelope {
    pub workspace_id: String,

    pub key_id: String,
    pub key_generation: u64,

    pub access_revision: u64,

    pub sender_device_id: String,
    pub recipient_device_id: String,

    pub algorithm: EnvelopeAlgorithm,

    pub sender_ephemeral_public_key: [u8; 32],
    pub nonce: [u8; 24],

    pub encrypted_workspace_key: Vec<u8>,

    pub created_at: u64,

    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EnvelopeAlgorithm {
    X25519HkdfSha256XChaCha20Poly1305,
}

impl KeyEnvelope {
    #[allow(clippy::too_many_arguments)]
    pub fn encrypt_for_device(
        workspace_id: impl Into<String>,
        key_id: impl Into<String>,
        key_generation: u64,
        access_revision: u64,
        workspace_key: &WorkspaceKey,
        sender: &DeviceIdentity,
        recipient: &DeviceRecord,
        created_at: u64,
    ) -> AccessResult<Self> {
        sender.validate()?;
        recipient.validate()?;

        let workspace_id = workspace_id.into();
        let key_id = key_id.into();

        let algorithm = EnvelopeAlgorithm::X25519HkdfSha256XChaCha20Poly1305;

        let ephemeral_secret = StaticSecret::random_from_rng(OsRng);
        let ephemeral_public = PublicKey::from(&ephemeral_secret);

        let recipient_public = PublicKey::from(recipient.exchange_public_key);

        let shared_secret = ephemeral_secret.diffie_hellman(&recipient_public);

        let context = envelope_context(
            &workspace_id,
            &key_id,
            key_generation,
            access_revision,
            &sender.device_id,
            &recipient.device_id,
            algorithm,
            created_at,
        );

        let envelope_key = derive_envelope_key(shared_secret.as_bytes(), &context)?;

        let cipher = XChaCha20Poly1305::new_from_slice(&envelope_key)
            .map_err(|_| AccessError::EnvelopeEncryptionFailed)?;

        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce);

        let encrypted_workspace_key = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: workspace_key,
                    aad: &context,
                },
            )
            .map_err(|_| AccessError::EnvelopeEncryptionFailed)?;

        let mut envelope = Self {
            workspace_id,

            key_id,
            key_generation,

            access_revision,

            sender_device_id: sender.device_id.clone(),
            recipient_device_id: recipient.device_id.clone(),

            algorithm,

            sender_ephemeral_public_key: ephemeral_public.to_bytes(),
            nonce,

            encrypted_workspace_key,

            created_at,
            signature: Vec::new(),
        };

        envelope.signature = sender.sign(&envelope.signature_payload());

        Ok(envelope)
    }

    pub fn verify_sender_signature(&self, sender: &DeviceRecord) -> AccessResult<()> {
        if self.sender_device_id != sender.device_id {
            return Err(AccessError::WrongEnvelopeSender {
                expected: sender.device_id.clone(),
                actual: self.sender_device_id.clone(),
            });
        }

        sender
            .verify_signature(&self.signature_payload(), &self.signature)
            .map_err(|_| AccessError::InvalidEnvelopeSignature)
    }

    pub fn decrypt_for_device(
        &self,
        recipient: &DeviceIdentity,
        sender: &DeviceRecord,
    ) -> AccessResult<WorkspaceKey> {
        if self.recipient_device_id != recipient.device_id {
            return Err(AccessError::WrongEnvelopeRecipient {
                expected: recipient.device_id.clone(),
                actual: self.recipient_device_id.clone(),
            });
        }

        self.verify_sender_signature(sender)?;
        recipient.validate()?;

        let recipient_secret = StaticSecret::from(recipient.exchange_private_key);
        let ephemeral_public = PublicKey::from(self.sender_ephemeral_public_key);
        let shared_secret = recipient_secret.diffie_hellman(&ephemeral_public);

        let context = envelope_context(
            &self.workspace_id,
            &self.key_id,
            self.key_generation,
            self.access_revision,
            &self.sender_device_id,
            &self.recipient_device_id,
            self.algorithm,
            self.created_at,
        );

        let envelope_key = derive_envelope_key(shared_secret.as_bytes(), &context)?;

        let cipher = XChaCha20Poly1305::new_from_slice(&envelope_key)
            .map_err(|_| AccessError::EnvelopeDecryptionFailed)?;

        let decrypted = cipher
            .decrypt(
                XNonce::from_slice(&self.nonce),
                Payload {
                    msg: &self.encrypted_workspace_key,
                    aad: &context,
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

    fn signature_payload(&self) -> Vec<u8> {
        let mut out = envelope_context(
            &self.workspace_id,
            &self.key_id,
            self.key_generation,
            self.access_revision,
            &self.sender_device_id,
            &self.recipient_device_id,
            self.algorithm,
            self.created_at,
        );

        push_bytes(&mut out, &self.sender_ephemeral_public_key);
        push_bytes(&mut out, &self.nonce);
        push_bytes(&mut out, &self.encrypted_workspace_key);

        out
    }
}

fn derive_envelope_key(shared_secret: &[u8; 32], context: &[u8]) -> AccessResult<[u8; 32]> {
    let hkdf = Hkdf::<Sha256>::new(Some(ENVELOPE_HKDF_SALT), shared_secret);

    let mut output_key = [0u8; 32];

    hkdf.expand(context, &mut output_key)
        .map_err(|_| AccessError::KeyDerivationFailed)?;

    Ok(output_key)
}

#[allow(clippy::too_many_arguments)]
fn envelope_context(
    workspace_id: &str,
    key_id: &str,
    key_generation: u64,
    access_revision: u64,
    sender_device_id: &str,
    recipient_device_id: &str,
    algorithm: EnvelopeAlgorithm,
    created_at: u64,
) -> Vec<u8> {
    let mut out = Vec::new();

    push_bytes(&mut out, ENVELOPE_CONTEXT);

    push_str(&mut out, workspace_id);
    push_str(&mut out, key_id);

    push_bytes(&mut out, &key_generation.to_be_bytes());

    push_bytes(&mut out, &access_revision.to_be_bytes());

    push_str(&mut out, sender_device_id);
    push_str(&mut out, recipient_device_id);
    push_str(&mut out, algorithm.as_str());

    push_bytes(&mut out, &created_at.to_be_bytes());

    out
}

fn push_str(out: &mut Vec<u8>, value: &str) {
    push_bytes(out, value.as_bytes());
}

fn push_bytes(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value);
}

impl EnvelopeAlgorithm {
    fn as_str(self) -> &'static str {
        match self {
            Self::X25519HkdfSha256XChaCha20Poly1305 => "x25519-hkdf-sha256-xchacha20poly1305",
        }
    }
}
