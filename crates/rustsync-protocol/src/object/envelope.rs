use serde::{Deserialize, Serialize};

use crate::{
    DeviceId, DeviceRecord, KeyId, ProtocolError, ProtocolResult, WorkspaceId,
    version::KEY_ENVELOPE_DOMAIN,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyEnvelope {
    pub workspace_id: WorkspaceId,
    pub key_id: KeyId,
    pub key_generation: u64,
    pub access_revision: u64,
    pub sender_device_id: DeviceId,
    pub recipient_device_id: DeviceId,
    pub algorithm: EnvelopeAlgorithm,
    pub sender_ephemeral_public_key: [u8; 32],
    pub nonce: [u8; 24],
    pub encrypted_workspace_key: Vec<u8>,
    pub created_at: u64,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EnvelopeAlgorithm {
    #[serde(alias = "X25519HkdfSha256XChaCha20Poly1305")]
    X25519HkdfSha256XChaCha20Poly1305,
}

impl KeyEnvelope {
    pub fn validate(&self) -> ProtocolResult<()> {
        if self.key_generation == 0 {
            return Err(ProtocolError::InvalidKeyGeneration(self.key_generation));
        }

        if self.encrypted_workspace_key.is_empty() {
            return Err(ProtocolError::EmptyCiphertext);
        }

        if self.signature.is_empty() {
            return Err(ProtocolError::InvalidSignature);
        }

        Ok(())
    }

    pub fn context_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();

        push_bytes(&mut out, KEY_ENVELOPE_DOMAIN);
        push_str(&mut out, self.workspace_id.as_str());
        push_str(&mut out, self.key_id.as_str());
        push_u64(&mut out, self.key_generation);
        push_u64(&mut out, self.access_revision);
        push_str(&mut out, self.sender_device_id.as_str());
        push_str(&mut out, self.recipient_device_id.as_str());
        push_str(&mut out, self.algorithm.as_str());
        push_u64(&mut out, self.created_at);

        out
    }

    pub fn signing_payload(&self) -> Vec<u8> {
        let mut out = self.context_bytes();

        push_bytes(&mut out, &self.sender_ephemeral_public_key);
        push_bytes(&mut out, &self.nonce);
        push_bytes(&mut out, &self.encrypted_workspace_key);

        out
    }

    pub fn verify_sender_signature(&self, sender: &DeviceRecord) -> ProtocolResult<()> {
        self.validate()?;

        if sender.device_id != self.sender_device_id {
            return Err(ProtocolError::InvalidAccessEvent(format!(
                "envelope sender `{}` does not match record `{}`",
                self.sender_device_id, sender.device_id
            )));
        }

        sender.verify_signature(&self.signing_payload(), &self.signature)
    }
}

impl EnvelopeAlgorithm {
    fn as_str(self) -> &'static str {
        match self {
            Self::X25519HkdfSha256XChaCha20Poly1305 => "x25519-hkdf-sha256-xchacha20poly1305",
        }
    }
}

fn push_u64(out: &mut Vec<u8>, value: u64) {
    push_bytes(out, &value.to_be_bytes());
}

fn push_str(out: &mut Vec<u8>, value: &str) {
    push_bytes(out, value.as_bytes());
}

fn push_bytes(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value);
}
