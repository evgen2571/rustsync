use ed25519_dalek::SigningKey;
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};

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
}

fn new_device_id() -> String {
    format!("{DEVICE_ID_PREFIX}_{}", Uuid::new_v4().simple())
}
