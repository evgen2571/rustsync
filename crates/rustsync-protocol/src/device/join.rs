use serde::{Deserialize, Serialize};

use crate::{
    DeviceRecord, DeviceStatus, JoinRequestId, ProtocolError, ProtocolResult, UnixTimestamp,
    WorkspaceId, version::DEVICE_JOIN_REQUEST_DOMAIN,
};

#[derive(Serialize, Deserialize)]
pub struct DeviceJoinRequest {
    pub request_id: JoinRequestId,
    pub workspace_id: WorkspaceId,
    pub device: DeviceRecord,
    pub created_at: UnixTimestamp,
    pub signature: Vec<u8>,
}

impl DeviceJoinRequest {
    pub fn new_unsigned(
        request_id: JoinRequestId,
        workspace_id: WorkspaceId,
        mut device: DeviceRecord,
        created_at: UnixTimestamp,
    ) -> Self {
        device.status = DeviceStatus::Pending;

        Self {
            request_id,
            workspace_id,
            device,
            created_at,
            signature: Vec::new(),
        }
    }

    pub fn with_signature(mut self, signature: Vec<u8>) -> Self {
        self.signature = signature;
        self
    }

    pub fn signing_payload(&self) -> Vec<u8> {
        let mut out = Vec::new();

        push_bytes(&mut out, DEVICE_JOIN_REQUEST_DOMAIN);
        push_str(&mut out, self.request_id.as_str());
        push_str(&mut out, self.workspace_id.as_str());
        push_device_record(&mut out, &self.device);
        push_u64(&mut out, self.created_at.as_secs());

        out
    }

    pub fn validate(&self) -> ProtocolResult<()> {
        if self.signature.is_empty() {
            return Err(ProtocolError::InvalidJoinRequest(
                ".signature is missing".to_string(),
            ));
        }

        if !self.device.is_pending() {
            return Err(ProtocolError::InvalidJoinRequest(
                "joining device must have pending status".to_string(),
            ));
        }

        self.device.validate()
    }

    pub fn verify(&self) -> ProtocolResult<()> {
        self.validate()?;
        self.device
            .verify_signature(&self.signing_payload(), &self.signature)
    }

    pub fn verify_for_workspace(&self, expected: &WorkspaceId) -> ProtocolResult<()> {
        if &self.workspace_id != expected {
            return Err(ProtocolError::WorkspaceMismatch {
                expected: expected.to_string(),
                actual: self.workspace_id.to_string(),
            });
        }

        self.verify()
    }
}

fn push_device_record(out: &mut Vec<u8>, device: &DeviceRecord) {
    push_str(out, device.device_id.as_str());
    push_str(out, &device.device_name);
    push_bytes(out, &device.signing_public_key);
    push_bytes(out, &device.exchange_public_key);
    push_str(out, &device.fingerprint);
    push_str(out, device_status_name(device.status));
}

fn device_status_name(status: DeviceStatus) -> &'static str {
    match status {
        DeviceStatus::Pending => "pending",
        DeviceStatus::Active => "active",
        DeviceStatus::Revoked => "revoked",
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
