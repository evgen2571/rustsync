use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;
use rustsync_protocol::{
    DEVICE_ID_PREFIX, DeviceId, DeviceRecord, DeviceStatus, ProtocolResult,
    device_signature_payload, fingerprint_from_public_keys,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};

use super::{DeviceError, DeviceResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub device_id: DeviceId,
    pub device_name: String,

    pub signing_public_key: [u8; 32],
    pub signing_private_key: [u8; 32],

    pub exchange_public_key: [u8; 32],
    pub exchange_private_key: [u8; 32],
}

impl DeviceIdentity {
    pub fn generate(device_name: impl Into<String>) -> DeviceResult<Self> {
        let device_name = normalize_device_name(device_name.into());

        let signing_key = SigningKey::generate(&mut OsRng);
        let signing_public_key = signing_key.verifying_key().to_bytes();

        let exchange_private_key = StaticSecret::random_from_rng(OsRng);
        let exchange_public_key = PublicKey::from(&exchange_private_key).to_bytes();

        let identity = Self {
            device_id: new_device_id()?,
            device_name,

            signing_public_key,
            signing_private_key: signing_key.to_bytes(),

            exchange_public_key,
            exchange_private_key: exchange_private_key.to_bytes(),
        };

        identity.validate()?;

        Ok(identity)
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

    pub fn sign(&self, payload: &[u8]) -> Vec<u8> {
        let signing_key = SigningKey::from_bytes(&self.signing_private_key);

        signing_key
            .sign(&device_signature_payload(payload))
            .to_bytes()
            .to_vec()
    }

    pub fn validate(&self) -> DeviceResult<()> {
        let signing_key = SigningKey::from_bytes(&self.signing_private_key);

        let derived_signing_public_key = signing_key.verifying_key().to_bytes();

        if derived_signing_public_key != self.signing_public_key {
            return Err(DeviceError::SigningKeyMismatch);
        }

        let exchange_private_key = StaticSecret::from(self.exchange_private_key);

        let derived_exchange_public_key = PublicKey::from(&exchange_private_key).to_bytes();

        if derived_exchange_public_key != self.exchange_public_key {
            return Err(DeviceError::ExchangeKeyMismatch);
        }

        self.public_record(DeviceStatus::Active)
            .validate()
            .map_err(DeviceError::Protocol)?;

        Ok(())
    }
}

fn new_device_id() -> ProtocolResult<DeviceId> {
    let value = format!("{DEVICE_ID_PREFIX}{}", Uuid::new_v4().simple());

    DeviceId::parse(value)
}

fn normalize_device_name(name: String) -> String {
    let name = name.trim();

    if name.is_empty() {
        "unknown-device".to_string()
    } else {
        name.to_string()
    }
}
