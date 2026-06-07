use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{DeviceId, ProtocolError, ProtocolResult, version::DEVICE_SIGNATURE_DOMAIN};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceRecord {
    pub device_id: DeviceId,
    pub device_name: String,

    pub signing_public_key: [u8; 32],
    pub exchange_public_key: [u8; 32],

    pub fingerprint: String,
    pub status: DeviceStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    #[serde(alias = "Pending")]
    Pending,
    #[serde(alias = "Active")]
    Active,
    #[serde(alias = "Revoked")]
    Revoked,
}

impl DeviceRecord {
    pub fn validate(&self) -> ProtocolResult<()> {
        if self.device_name.trim().is_empty() {
            return Err(ProtocolError::InvalidDeviceName);
        }

        let expected =
            fingerprint_from_public_keys(&self.signing_public_key, &self.exchange_public_key);

        if expected != self.fingerprint {
            return Err(ProtocolError::FingerprintMismatch {
                expected,
                actual: self.fingerprint.clone(),
            });
        }

        VerifyingKey::from_bytes(&self.signing_public_key)
            .map_err(|_| ProtocolError::InvalidPublicKey)?;

        Ok(())
    }

    pub fn verify_signature(&self, message: &[u8], signature: &[u8]) -> ProtocolResult<()> {
        self.validate()?;

        let verifying_key = VerifyingKey::from_bytes(&self.signing_public_key)
            .map_err(|_| ProtocolError::InvalidPublicKey)?;

        let signature =
            Signature::from_slice(signature).map_err(|_| ProtocolError::InvalidSignature)?;

        verifying_key
            .verify(&device_signature_payload(message), &signature)
            .map_err(|_| ProtocolError::InvalidSignature)
    }

    pub fn activate(&mut self) {
        self.status = DeviceStatus::Active;
    }

    pub fn revoke(&mut self) {
        self.status = DeviceStatus::Revoked;
    }

    pub fn is_active(&self) -> bool {
        self.status == DeviceStatus::Active
    }

    pub fn is_pending(&self) -> bool {
        self.status == DeviceStatus::Pending
    }

    pub fn is_revoked(&self) -> bool {
        self.status == DeviceStatus::Revoked
    }
}

pub fn device_signature_payload(message: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        DEVICE_SIGNATURE_DOMAIN.len() + std::mem::size_of::<u64>() + message.len(),
    );

    push_bytes(&mut out, DEVICE_SIGNATURE_DOMAIN);
    push_bytes(&mut out, message);

    out
}

pub fn fingerprint_from_public_keys(
    signing_public_key: &[u8; 32],
    exchange_public_key: &[u8; 32],
) -> String {
    let mut hasher = Sha256::new();

    hasher.update(signing_public_key);
    hasher.update(exchange_public_key);

    let hash = hasher.finalize();

    short_fingerprint(&hash)
}

pub fn short_fingerprint(bytes: &[u8]) -> String {
    let take = bytes.len().min(10);
    let encoded = URL_SAFE_NO_PAD.encode(&bytes[..take]).to_uppercase();

    encoded
        .as_bytes()
        .chunks(4)
        .map(|chunk| std::str::from_utf8(chunk).expect("base64 is valid utf8"))
        .collect::<Vec<_>>()
        .join("-")
}

fn push_bytes(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value);
}
