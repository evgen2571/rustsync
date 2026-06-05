use ed25519_dalek::{Signature, SigningKey, Verifier, VerifyingKey, ed25519::signature::SignerMut};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};

use super::{DeviceError, DeviceResult, fingerprint_from_public_keys};

pub const DEVICE_ID_PREFIX: &str = "device";

const DEVICE_SIGNATURE_DOMAIN: &[u8] = b"rustsync/device-signature";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub device_id: String,
    pub device_name: String,

    pub signing_public_key: [u8; 32],
    pub signing_private_key: [u8; 32],

    pub exchange_public_key: [u8; 32],
    pub exchange_private_key: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceRecord {
    pub device_id: String,
    pub device_name: String,

    pub signing_public_key: [u8; 32],
    pub exchange_public_key: [u8; 32],

    pub fingerprint: String,
    pub status: DeviceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeviceStatus {
    Pending,
    Active,
    Revoked,
}

impl DeviceIdentity {
    pub fn generate(device_name: impl Into<String>) -> Self {
        let device_name = device_name.into();

        let signing_key = SigningKey::generate(&mut OsRng);
        let signing_public_key = signing_key.verifying_key().to_bytes();

        let exchange_private_key = StaticSecret::random_from_rng(OsRng);
        let exchange_public_key = PublicKey::from(&exchange_private_key);

        Self {
            device_id: new_device_id(),
            device_name,
            signing_public_key,
            signing_private_key: signing_key.to_bytes(),
            exchange_public_key: exchange_public_key.to_bytes(),
            exchange_private_key: exchange_private_key.to_bytes(),
        }
    }

    pub fn public_record(&self, status: DeviceStatus) -> DeviceRecord {
        DeviceRecord {
            device_id: self.device_id.clone(),
            device_name: self.device_name.clone(),
            signing_public_key: self.signing_public_key,
            exchange_public_key: self.exchange_public_key,
            fingerprint: self.fingerprint(),
            status,
        }
    }

    pub fn fingerprint(&self) -> String {
        fingerprint_from_public_keys(&self.signing_public_key, &self.exchange_public_key)
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        let mut signing_key = SigningKey::from_bytes(&self.signing_private_key);
        let signed_message = device_signed_message(message);

        signing_key.sign(&signed_message).to_bytes().to_vec()
    }

    pub fn validate(&self) -> DeviceResult<()> {
        validate_device_id(&self.device_id)?;
        validate_device_name(&self.device_name)?;

        let signing_key = SigningKey::from_bytes(&self.signing_private_key);

        if signing_key.verifying_key().to_bytes() != self.signing_public_key {
            return Err(DeviceError::InvalidPublicKey);
        }

        let exchange_private_key = StaticSecret::from(self.exchange_private_key);
        let exchange_public_key = PublicKey::from(&exchange_private_key).to_bytes();

        if exchange_public_key != self.exchange_public_key {
            return Err(DeviceError::InvalidPublicKey);
        }

        Ok(())
    }
}

impl DeviceRecord {
    pub fn validate(&self) -> DeviceResult<()> {
        validate_device_id(&self.device_id)?;
        validate_device_name(&self.device_name)?;

        let expected_fingerprint =
            fingerprint_from_public_keys(&self.signing_public_key, &self.exchange_public_key);

        if expected_fingerprint != self.fingerprint {
            return Err(DeviceError::FingerprintMismatch {
                expected: expected_fingerprint,
                actual: self.fingerprint.clone(),
            });
        }

        VerifyingKey::from_bytes(&self.signing_public_key)
            .map_err(|_| DeviceError::InvalidPublicKey)?;

        Ok(())
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

    pub fn verify_signature(&self, message: &[u8], signature: &[u8]) -> DeviceResult<()> {
        let verifying_key = VerifyingKey::from_bytes(&self.signing_public_key)
            .map_err(|_| DeviceError::InvalidPublicKey)?;

        let signature =
            Signature::from_slice(signature).map_err(|_| DeviceError::InvalidSignature)?;

        verifying_key
            .verify(message, &signature)
            .map_err(|_| DeviceError::InvalidSignature)
    }
}

pub fn default_device_name() -> String {
    hostname::get()
        .ok()
        .and_then(|name| name.into_string().ok())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "unknown-device".to_string())
}

fn new_device_id() -> String {
    format!("{DEVICE_ID_PREFIX}_{}", Uuid::new_v4().simple())
}

fn validate_device_id(device_id: &str) -> DeviceResult<()> {
    let is_valid = device_id.starts_with(&format!("{DEVICE_ID_PREFIX}_"))
        && device_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

    if !is_valid {
        return Err(DeviceError::InvalidDeviceId {
            device_id: device_id.to_string(),
        });
    }

    Ok(())
}

fn validate_device_name(device_name: &str) -> DeviceResult<()> {
    if device_name.trim().is_empty() {
        return Err(DeviceError::InvalidDeviceName {
            device_name: device_name.to_string(),
        });
    }

    Ok(())
}

fn device_signed_message(message: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        DEVICE_SIGNATURE_DOMAIN.len() + 1 + std::mem::size_of::<u64>() + message.len(),
    );

    out.extend_from_slice(DEVICE_SIGNATURE_DOMAIN);
    out.push(0);
    out.extend_from_slice(&(message.len() as u64).to_be_bytes());
    out.extend_from_slice(message);

    out
}
