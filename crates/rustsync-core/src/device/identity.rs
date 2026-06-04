use ed25519_dalek::{Signature, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};

use super::{DeviceError, DeviceResult, fingerprint_from_public_keys};

pub const DEVICE_ID_PREFIX: &str = "device";

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
        let fingerprint = self.fingerprint();

        DeviceRecord {
            device_id: self.device_id.clone(),
            device_name: self.device_name.clone(),
            signing_public_key: self.signing_public_key,
            exchange_public_key: self.exchange_public_key,
            fingerprint,
            status,
        }
    }

    pub fn fingerprint(&self) -> String {
        fingerprint_from_public_keys(&self.signing_public_key, &self.exchange_public_key)
    }
}

impl DeviceRecord {
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
