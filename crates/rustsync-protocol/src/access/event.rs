use serde::{Deserialize, Serialize};

use crate::{
    DeviceId, DeviceJoinRequest, DeviceRecord, DeviceStatus, JoinRequestId, KeyId, ProtocolError,
    ProtocolResult, UnixTimestamp, WorkspaceId, id::AccessEventId, version::ACCESS_EVENT_DOMAIN,
};

use super::{WorkspacePermission, WorkspaceRole};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AccessEvent {
    WorkspaceCreated {
        owner: DeviceRecord,
    },
    DeviceJoined {
        join_request_id: JoinRequestId,
        device: DeviceRecord,
        role: WorkspaceRole,
    },
    DeviceRoleChanged {
        device_id: DeviceId,
        new_role: WorkspaceRole,
    },
    DeviceRemoved {
        device_id: DeviceId,
    },
    RestrictedKeyGranted {
        key_id: KeyId,
        key_generation: u64,
        device_id: DeviceId,
    },
    RestrictedKeyRevoked {
        key_id: KeyId,
        key_generation: u64,
        device_id: DeviceId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedAccessEvent {
    pub event_id: AccessEventId,
    pub workspace_id: WorkspaceId,
    pub expected_revision: u64,
    pub actor_device_id: DeviceId,
    pub created_at: UnixTimestamp,
    pub event: AccessEvent,
    pub signature: Vec<u8>,
}

impl AccessEvent {
    pub fn required_permission(&self) -> Option<WorkspacePermission> {
        match self {
            Self::WorkspaceCreated { .. } => None,
            Self::DeviceJoined { .. } | Self::DeviceRemoved { .. } => {
                Some(WorkspacePermission::ManageDevices)
            }
            Self::DeviceRoleChanged { .. } => Some(WorkspacePermission::ManageRoles),
            Self::RestrictedKeyGranted { .. } | Self::RestrictedKeyRevoked { .. } => {
                Some(WorkspacePermission::ManageKeys)
            }
        }
    }

    pub fn validate(&self) -> ProtocolResult<()> {
        match self {
            Self::WorkspaceCreated { owner } => {
                owner.validate()?;

                if owner.status != DeviceStatus::Active {
                    return Err(ProtocolError::InvalidAccessEvent(
                        "workspace owner must have active status".to_string(),
                    ));
                }
            }
            Self::DeviceJoined { device, .. } => {
                device.validate()?;

                if device.status != DeviceStatus::Pending {
                    return Err(ProtocolError::InvalidAccessEvent(
                        "joined device record must have pending status".to_string(),
                    ));
                }
            }
            Self::RestrictedKeyGranted { key_generation, .. }
            | Self::RestrictedKeyRevoked { key_generation, .. } => {
                if *key_generation == 0 {
                    return Err(ProtocolError::InvalidKeyGeneration(*key_generation));
                }
            }
            Self::DeviceRoleChanged { .. } | Self::DeviceRemoved { .. } => {}
        }

        Ok(())
    }
}

impl SignedAccessEvent {
    pub fn new_unsigned(
        event_id: AccessEventId,
        workspace_id: WorkspaceId,
        expected_revision: u64,
        actor_device_id: DeviceId,
        created_at: UnixTimestamp,
        event: AccessEvent,
    ) -> Self {
        Self {
            event_id,
            workspace_id,
            expected_revision,
            actor_device_id,
            created_at,
            event,
            signature: Vec::new(),
        }
    }

    pub fn with_signature(mut self, signature: Vec<u8>) -> Self {
        self.signature = signature;
        self
    }

    pub fn validate(&self) -> ProtocolResult<()> {
        if self.signature.is_empty() {
            return Err(ProtocolError::InvalidAccessEvent(
                "signature is missing".to_string(),
            ));
        }

        self.event.validate()
    }

    pub fn signing_payload(&self) -> Vec<u8> {
        let mut out = Vec::new();

        push_bytes(&mut out, ACCESS_EVENT_DOMAIN);
        push_str(&mut out, self.event_id.as_str());
        push_str(&mut out, self.workspace_id.as_str());
        push_u64(&mut out, self.expected_revision);
        push_str(&mut out, self.actor_device_id.as_str());
        push_u64(&mut out, self.created_at.as_secs());
        push_access_event(&mut out, &self.event);

        out
    }

    pub fn verify_signature(&self, actor: &DeviceRecord) -> ProtocolResult<()> {
        self.validate()?;

        if actor.device_id != self.actor_device_id {
            return Err(ProtocolError::InvalidAccessEvent(format!(
                "actore record `{}` does not match event actor `{}`",
                actor.device_id, self.actor_device_id,
            )));
        }

        actor.verify_signature(&self.signing_payload(), &self.signature)
    }

    pub fn verify_join_request(&self, request: &DeviceJoinRequest) -> ProtocolResult<()> {
        match &self.event {
            AccessEvent::DeviceJoined {
                join_request_id,
                device,
                ..
            } => {
                if join_request_id != &request.request_id {
                    return Err(ProtocolError::InvalidAccessEvent(
                        "join request ID does not match event".to_string(),
                    ));
                }

                if device != &request.device {
                    return Err(ProtocolError::InvalidAccessEvent(
                        "join request device does not match event device".to_string(),
                    ));
                }

                if self.workspace_id != request.workspace_id {
                    return Err(ProtocolError::InvalidAccessEvent(
                        "join request workspace does not match event workspace".to_string(),
                    ));
                }

                Ok(())
            }

            _ => Err(ProtocolError::InvalidAccessEvent(
                "event is not a device join event".to_string(),
            )),
        }
    }
}

fn push_access_event(out: &mut Vec<u8>, event: &AccessEvent) {
    match event {
        AccessEvent::WorkspaceCreated { owner } => {
            push_u8(out, 1);
            push_device_record(out, owner);
        }
        AccessEvent::DeviceJoined {
            join_request_id,
            device,
            role,
        } => {
            push_u8(out, 2);
            push_str(out, join_request_id.as_str());
            push_device_record(out, device);
            push_role(out, *role);
        }
        AccessEvent::DeviceRoleChanged {
            device_id,
            new_role,
        } => {
            push_u8(out, 3);
            push_str(out, device_id.as_str());
            push_role(out, *new_role);
        }
        AccessEvent::DeviceRemoved { device_id } => {
            push_u8(out, 4);
            push_str(out, device_id.as_str());
        }
        AccessEvent::RestrictedKeyGranted {
            key_id,
            key_generation,
            device_id,
        } => {
            push_u8(out, 5);
            push_str(out, key_id.as_str());
            push_u64(out, *key_generation);
            push_str(out, device_id.as_str());
        }
        AccessEvent::RestrictedKeyRevoked {
            key_id,
            key_generation,
            device_id,
        } => {
            push_u8(out, 6);
            push_str(out, key_id.as_str());
            push_u64(out, *key_generation);
            push_str(out, device_id.as_str());
        }
    }
}

fn push_device_record(out: &mut Vec<u8>, device: &DeviceRecord) {
    push_str(out, device.device_id.as_str());
    push_str(out, &device.device_name);
    push_bytes(out, &device.signing_public_key);
    push_bytes(out, &device.exchange_public_key);
    push_str(out, &device.fingerprint);
    push_u8(
        out,
        match device.status {
            DeviceStatus::Pending => 1,
            DeviceStatus::Active => 2,
            DeviceStatus::Revoked => 3,
        },
    );
}

fn push_role(out: &mut Vec<u8>, role: WorkspaceRole) {
    push_u8(
        out,
        match role {
            WorkspaceRole::Owner => 1,
            WorkspaceRole::Member => 2,
        },
    );
}

fn push_u8(out: &mut Vec<u8>, value: u8) {
    push_bytes(out, &[value]);
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
