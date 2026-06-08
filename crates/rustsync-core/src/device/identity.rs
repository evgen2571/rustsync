use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;
use rustsync_protocol::{
    DEVICE_ID_PREFIX, DeviceId, DeviceJoinRequest, DeviceRecord, DeviceStatus,
    JOIN_REQUEST_ID_PREFIX, JoinRequestId, WorkspaceId, device_signature_payload,
    fingerprint_from_public_keys,
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use x25519_dalek::{PublicKey, StaticSecret};

use super::{DeviceError, DeviceResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIdentity {
    device_id: DeviceId,
    device_name: String,

    signing_public_key: [u8; 32],
    signing_private_key: [u8; 32],

    exchange_public_key: [u8; 32],
    exchange_private_key: [u8; 32],
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

    pub fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn signing_public_key(&self) -> &[u8; 32] {
        &self.signing_public_key
    }

    pub fn exchange_public_key(&self) -> &[u8; 32] {
        &self.exchange_public_key
    }

    pub fn exchange_private_key(&self) -> &[u8; 32] {
        &self.exchange_private_key
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

    pub fn sign(&self, payload: &[u8]) -> DeviceResult<Vec<u8>> {
        self.validate()?;

        let signing_key = SigningKey::from_bytes(&self.signing_private_key);

        let signed_payload = device_signature_payload(payload);

        Ok(signing_key.sign(&signed_payload).to_bytes().to_vec())
    }

    pub fn create_join_request(
        &self,
        workspace_id: WorkspaceId,
    ) -> DeviceResult<DeviceJoinRequest> {
        let request_id = new_join_request_id()?;
        let created_at = now_unix();

        self.create_join_request_at(request_id, workspace_id, created_at)
    }

    pub fn create_join_request_at(
        &self,
        request_id: JoinRequestId,
        workspace_id: WorkspaceId,
        created_at: u64,
    ) -> DeviceResult<DeviceJoinRequest> {
        self.validate()?;

        let request = DeviceJoinRequest::new_unsigned(
            request_id,
            workspace_id,
            self.public_record(DeviceStatus::Pending),
            created_at,
        );

        let signature = self.sign(&request.signing_payload())?;

        Ok(request.with_signature(signature))
    }

    pub fn matches_record(&self, record: &DeviceRecord) -> bool {
        self.device_id == record.device_id
            && self.signing_public_key == record.signing_public_key
            && self.exchange_public_key == record.exchange_public_key
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

    pub(crate) fn exchange_secret(&self) -> StaticSecret {
        StaticSecret::from(self.exchange_private_key)
    }
}

pub fn default_device_name() -> String {
    hostname::get()
        .ok()
        .and_then(|name| name.into_string().ok())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "unknown-device".to_string())
}

fn new_device_id() -> DeviceResult<DeviceId> {
    let value = format!("{DEVICE_ID_PREFIX}{}", Uuid::new_v4().simple(),);

    DeviceId::parse(value).map_err(DeviceError::Protocol)
}

fn new_join_request_id() -> DeviceResult<JoinRequestId> {
    let value = format!("{JOIN_REQUEST_ID_PREFIX}{}", Uuid::new_v4().simple(),);

    JoinRequestId::parse(value).map_err(DeviceError::Protocol)
}

fn normalize_device_name(name: String) -> String {
    let name = name.trim();

    if name.is_empty() {
        default_device_name()
    } else {
        name.to_string()
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_secs()
}
