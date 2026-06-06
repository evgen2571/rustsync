use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    device::{DeviceIdentity, DeviceJoinRequest, DeviceRecord, DeviceRegistry},
    keyring::{KeyVisibility, WorkspaceKey, WorkspaceKeyRecord, WorkspaceKeyring},
};

use super::{AccessError, AccessResult, KeyAcl, KeyEnvelope, WorkspaceAcl, WorkspaceRole};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessControl {
    pub workspace_id: String,
    pub revision: u64,

    workspace_acl: WorkspaceAcl,
    key_acl: KeyAcl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRevocation {
    pub device_id: String,
    pub key_ids_requiring_rotation: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyAccessRevocation {
    pub key_id: String,
    pub device_id: String,
    pub requires_rotation: bool,
}

impl AccessControl {
    pub fn new(workspace_id: impl Into<String>, owner_device_id: impl Into<String>) -> Self {
        Self::new_at(workspace_id, owner_device_id, now_unix())
    }

    pub fn new_at(
        workspace_id: impl Into<String>,
        owner_device_id: impl Into<String>,
        create_at: u64,
    ) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            revision: 0,
            workspace_acl: WorkspaceAcl::new(owner_device_id, create_at),
            key_acl: KeyAcl::new(),
        }
    }

    pub fn validate(&self) -> AccessResult<()> {
        if self.workspace_id.trim().is_empty() {
            return Err(AccessError::InvalidWorkspaceId);
        }

        if self.workspace_acl.owner_count() == 0 {
            return Err(AccessError::CannotRemoveLastOwner);
        }

        Ok(())
    }

    pub fn workspace_acl(&self) -> &WorkspaceAcl {
        &self.workspace_acl
    }

    pub fn key_acl(&self) -> &KeyAcl {
        &self.key_acl
    }

    pub fn approve_device(
        &mut self,
        devices: &mut DeviceRegistry,
        request: &DeviceJoinRequest,
        role: WorkspaceRole,
        approved_by_device_id: &str,
    ) -> AccessResult<DeviceRecord> {
        self.require_active_owner(devices, approved_by_device_id)?;

        request.verify_for_workspace(&self.workspace_id)?;

        let device_id = request.device.device_id.clone();

        if self.workspace_acl.contains(&device_id) {
            return Err(AccessError::DeviceAlreadyMember(device_id));
        }

        let mut next_devices = devices.clone();
        let mut next_access = self.clone();

        if next_devices.contains(&device_id) {
            let existing = next_devices.get(&device_id)?;

            if existing.signing_public_key != request.device.signing_public_key
                || existing.exchange_public_key != request.device.exchange_public_key
            {
                return Err(AccessError::DeviceIdentityConflict(device_id));
            }
        } else {
            next_devices.insert_pending(request.device.clone())?;
        }

        next_devices.activate(&device_id)?;

        next_access.workspace_acl.grant(
            device_id.clone(),
            role,
            approved_by_device_id,
            now_unix(),
        )?;

        next_access.bump_revision()?;

        let approved_record = next_devices.get(&device_id)?.clone();

        *devices = next_devices;
        *self = next_access;

        Ok(approved_record)
    }

    pub fn revoke_device(
        &mut self,
        devices: &mut DeviceRegistry,
        keyring: &WorkspaceKeyring,
        device_id: &str,
        revoked_by_device_id: &str,
    ) -> AccessResult<DeviceRevocation> {
        self.require_active_owner(devices, revoked_by_device_id)?;

        devices.require_active(device_id)?;
        self.workspace_acl.require_member(device_id)?;

        let key_ids_requiring_rotation = keyring
            .list()
            .filter_map(
                |key| match self.can_access_key_record(devices, key, device_id) {
                    Ok(true) => Some(key.key_id.clone()),
                    Ok(false) | Err(_) => None,
                },
            )
            .collect::<Vec<_>>();

        let mut next_devices = devices.clone();
        let mut next_access = self.clone();

        next_access
            .workspace_acl
            .remove(device_id, revoked_by_device_id)?;

        next_access.key_acl.remove_device(device_id);

        next_devices.revoke(device_id)?;

        next_access.bump_revision()?;

        *devices = next_devices;
        *self = next_access;

        Ok(DeviceRevocation {
            device_id: device_id.to_string(),
            key_ids_requiring_rotation,
        })
    }

    pub fn set_workspace_role(
        &mut self,
        devices: &DeviceRegistry,
        device_id: &str,
        role: WorkspaceRole,
        changed_by_device_id: &str,
    ) -> AccessResult<()> {
        self.require_active_owner(devices, changed_by_device_id)?;

        devices.require_active(device_id)?;

        self.workspace_acl
            .set_role(device_id, role, changed_by_device_id)?;

        self.bump_revision()
    }

    pub fn initialize_key_access(
        &mut self,
        devices: &DeviceRegistry,
        keyring: &WorkspaceKeyring,
        key_id: &str,
        created_by_device_id: &str,
    ) -> AccessResult<()> {
        self.require_active_owner(devices, created_by_device_id)?;

        let key = keyring.get(key_id)?;

        if key.visibility == KeyVisibility::Restricted {
            self.key_acl.grant(
                key_id,
                created_by_device_id,
                created_by_device_id,
                now_unix(),
            )?;

            self.bump_revision()?;
        }

        Ok(())
    }

    pub fn grant_key_access(
        &mut self,
        devices: &DeviceRegistry,
        keyring: &WorkspaceKeyring,
        key_id: &str,
        recipient_device_id: &str,
        granted_by_device_id: &str,
    ) -> AccessResult<()> {
        self.require_active_owner(devices, granted_by_device_id)?;

        devices.require_active(recipient_device_id)?;

        self.workspace_acl.require_member(recipient_device_id)?;

        let key = keyring.get(key_id)?;
        require_restricted_key(key)?;

        self.key_acl.grant(
            key_id,
            recipient_device_id,
            granted_by_device_id,
            now_unix(),
        )?;

        self.bump_revision()
    }

    pub fn revoke_key_access(
        &mut self,
        devices: &DeviceRegistry,
        keyring: &WorkspaceKeyring,
        key_id: &str,
        recipient_device_id: &str,
        revoked_by_device_id: &str,
    ) -> AccessResult<KeyAccessRevocation> {
        self.require_active_owner(devices, revoked_by_device_id)?;

        devices.require_active(recipient_device_id)?;

        self.workspace_acl.require_member(recipient_device_id)?;

        let key = keyring.get(key_id)?;
        require_restricted_key(key)?;

        self.key_acl.revoke(key_id, recipient_device_id)?;

        self.bump_revision()?;

        Ok(KeyAccessRevocation {
            key_id: key_id.to_string(),
            device_id: recipient_device_id.to_string(),
            requires_rotation: true,
        })
    }

    pub fn can_access_key(
        &self,
        devices: &DeviceRegistry,
        keyring: &WorkspaceKeyring,
        key_id: &str,
        device_id: &str,
    ) -> AccessResult<bool> {
        let key = keyring.get(key_id)?;

        self.can_access_key_record(devices, key, device_id)
    }

    pub fn require_key_access(
        &self,
        devices: &DeviceRegistry,
        keyring: &WorkspaceKeyring,
        key_id: &str,
        device_id: &str,
    ) -> AccessResult<()> {
        if self.can_access_key(devices, keyring, key_id, device_id)? {
            Ok(())
        } else {
            Err(AccessError::DeviceNotAuthorizedForKey {
                key_id: key_id.to_string(),
                device_id: device_id.to_string(),
            })
        }
    }

    pub fn issue_key_envelope(
        &self,
        devices: &DeviceRegistry,
        keyring: &WorkspaceKeyring,
        key_id: &str,
        sender: &DeviceIdentity,
        recipient_device_id: &str,
    ) -> AccessResult<KeyEnvelope> {
        self.require_active_owner(devices, &sender.device_id)?;

        self.require_registered_identity(devices, sender)?;

        self.require_key_access(devices, keyring, key_id, recipient_device_id)?;

        let recipient = devices.require_active(recipient_device_id)?;

        let key_record = keyring.get(key_id)?;
        let workspace_key = keyring.load_key(key_id)?;

        KeyEnvelope::encrypt_for_device(
            self.workspace_id.clone(),
            key_id,
            key_record.generation,
            self.revision,
            &workspace_key,
            sender,
            recipient,
            now_unix(),
        )
    }

    pub fn issue_authorized_key_envelopes(
        &self,
        devices: &DeviceRegistry,
        keyring: &WorkspaceKeyring,
        sender: &DeviceIdentity,
        recipient_device_id: &str,
    ) -> AccessResult<Vec<KeyEnvelope>> {
        self.require_active_owner(devices, &sender.device_id)?;

        self.require_registered_identity(devices, sender)?;

        devices.require_active(recipient_device_id)?;

        self.workspace_acl.require_member(recipient_device_id)?;

        let key_ids = keyring
            .list()
            .filter(|key| match key.visibility {
                KeyVisibility::Shared => true,

                KeyVisibility::Restricted => {
                    self.key_acl.is_granted(&key.key_id, recipient_device_id)
                }
            })
            .map(|key| key.key_id.clone())
            .collect::<Vec<_>>();

        key_ids
            .iter()
            .map(|key_id| {
                self.issue_key_envelope(devices, keyring, key_id, sender, recipient_device_id)
            })
            .collect()
    }

    pub fn open_key_envelope(
        &self,
        devices: &DeviceRegistry,
        keyring: &WorkspaceKeyring,
        envelope: &KeyEnvelope,
        recipient: &DeviceIdentity,
    ) -> AccessResult<WorkspaceKey> {
        if envelope.workspace_id != self.workspace_id {
            return Err(AccessError::WorkspaceIdMismatch {
                expected: self.workspace_id.clone(),
                actual: envelope.workspace_id.clone(),
            });
        }

        if envelope.access_revision > self.revision {
            return Err(AccessError::AccessControlTooOld {
                local_revision: self.revision,
                envelope_revision: envelope.access_revision,
            });
        }

        let key_record = keyring.get(&envelope.key_id)?;

        if envelope.key_generation != key_record.generation {
            return Err(AccessError::KeyGenerationMismatch {
                key_id: envelope.key_id.clone(),
                expected: key_record.generation,
                actual: envelope.key_generation,
            });
        }

        self.require_registered_identity(devices, recipient)?;
        self.require_key_access(devices, keyring, &envelope.key_id, &recipient.device_id)?;
        self.require_active_owner(devices, &envelope.sender_device_id)?;

        let sender = devices.require_active(&envelope.sender_device_id)?;

        envelope.decrypt_for_device(recipient, sender)
    }

    fn can_access_key_record(
        &self,
        devices: &DeviceRegistry,
        key: &WorkspaceKeyRecord,
        device_id: &str,
    ) -> AccessResult<bool> {
        devices.require_active(device_id)?;

        self.workspace_acl.require_member(device_id)?;

        Ok(match key.visibility {
            KeyVisibility::Shared => true,

            KeyVisibility::Restricted => self.key_acl.is_granted(&key.key_id, device_id),
        })
    }

    fn require_active_owner(&self, devices: &DeviceRegistry, device_id: &str) -> AccessResult<()> {
        devices.require_active(device_id)?;

        self.workspace_acl.require_owner(device_id)?;

        Ok(())
    }

    fn require_registered_identity(
        &self,
        devices: &DeviceRegistry,
        identity: &DeviceIdentity,
    ) -> AccessResult<()> {
        identity.validate()?;

        let record = devices.require_active(&identity.device_id)?;

        if record.signing_public_key != identity.signing_public_key
            || record.exchange_public_key != identity.exchange_public_key
        {
            return Err(AccessError::DeviceIdentityConflict(
                identity.device_id.clone(),
            ));
        }

        Ok(())
    }

    fn bump_revision(&mut self) -> AccessResult<()> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(AccessError::RevisionOverflow)?;

        Ok(())
    }
}

fn require_restricted_key(key: &WorkspaceKeyRecord) -> AccessResult<()> {
    if key.visibility == KeyVisibility::Shared {
        return Err(AccessError::SharedKeyUsesImplicitAccess {
            key_id: key.key_id.clone(),
        });
    }

    Ok(())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_secs()
}
