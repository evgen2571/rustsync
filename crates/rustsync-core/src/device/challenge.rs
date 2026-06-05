use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{DeviceError, DeviceIdentity, DeviceRecord, DeviceResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceJoinRequest {
    pub workspace_id: String,
    pub device: DeviceRecord,
    pub created_at: u64,
    pub signature: Vec<u8>,
}

impl DeviceJoinRequest {
    pub fn create(
        workspace_id: impl Into<String>,
        identity: &DeviceIdentity,
    ) -> DeviceResult<Self> {
        identity.validate()?;

        let workspace_id = workspace_id.into();
        let device = identity.public_record(super::DeviceStatus::Pending);
        let created_at = now_unix();

        let payload = join_request_payload(&workspace_id, &device, created_at);

        let signature = identity.sign(&payload);

        Ok(Self {
            workspace_id,
            device,
            created_at,
            signature,
        })
    }

    pub fn verify_for_workspace(&self, expected_workspace_id: &str) -> DeviceResult<()> {
        if self.workspace_id != expected_workspace_id {
            return Err(DeviceError::WorkspaceIdMismatch {
                expected: expected_workspace_id.to_string(),
                actual: self.workspace_id.clone(),
            });
        }

        if !self.device.is_pending() {
            return Err(DeviceError::InvalidJoinRequest(
                "join request device record must be pending".to_string(),
            ));
        }

        self.device.validate()?;

        let payload = join_request_payload(&self.workspace_id, &self.device, self.created_at);

        self.device.verify_signature(&payload, &self.signature)
    }
}

fn join_request_payload(workspace_id: &str, device: &DeviceRecord, created_at: u64) -> Vec<u8> {
    let mut out = Vec::new();

    out.extend_from_slice(b"rustsync/device-join-request");
    out.push(0);

    out.extend_from_slice(workspace_id.as_bytes());
    out.push(0);

    out.extend_from_slice(device.device_id.as_bytes());
    out.push(0);

    out.extend_from_slice(device.device_name.as_bytes());
    out.push(0);

    out.extend_from_slice(&device.signing_public_key);
    out.push(0);

    out.extend_from_slice(&device.exchange_public_key);
    out.push(0);

    out.extend_from_slice(device.fingerprint.as_bytes());
    out.push(0);

    out.extend_from_slice(&created_at.to_be_bytes());
    out.push(0);

    out
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_secs()
}
