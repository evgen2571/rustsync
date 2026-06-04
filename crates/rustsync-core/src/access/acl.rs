use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::{AccessError, AccessResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceAcl {
    pub workspace_id: String,
    devices: BTreeMap<String, DeviceAccess>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceAccess {
    pub device_id: String,
    pub role: WorkspaceRole,
    pub status: DeviceAccessStatus,
    pub granted_by_device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkspaceRole {
    Owner,
    Member,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeviceAccessStatus {
    Active,
    Revoked,
}

impl WorkspaceAcl {
    pub fn new(workspace_id: impl Into<String>, owner_device_id: impl Into<String>) -> Self {
        let workspace_id = workspace_id.into();
        let owner_device_id = owner_device_id.into();

        let mut devices = BTreeMap::new();

        devices.insert(
            owner_device_id.clone(),
            DeviceAccess {
                device_id: owner_device_id,
                role: WorkspaceRole::Owner,
                status: DeviceAccessStatus::Active,
                granted_by_device_id: None,
            },
        );

        Self {
            workspace_id,
            devices,
        }
    }

    pub fn grant(
        &mut self,
        device_id: impl Into<String>,
        role: WorkspaceRole,
        granted_by_device_id: impl Into<String>,
    ) -> AccessResult<()> {
        let device_id = device_id.into();
        let granted_by_device_id = granted_by_device_id.into();

        self.require_owner(&granted_by_device_id)?;

        if let Some(access) = self.devices.get(&device_id) {
            if access.status == DeviceAccessStatus::Active {
                return Err(AccessError::DeviceAlreadyAllowed(device_id));
            }
        }

        self.devices.insert(
            device_id.clone(),
            DeviceAccess {
                device_id,
                role,
                status: DeviceAccessStatus::Active,
                granted_by_device_id: Some(granted_by_device_id),
            },
        );

        Ok(())
    }

    pub fn revoke(&mut self, device_id: &str, revoked_by_device_id: &str) -> AccessResult<()> {
        self.require_owner(revoked_by_device_id)?;

        let access = self
            .devices
            .get_mut(device_id)
            .ok_or_else(|| AccessError::DeviceNotAllowed(device_id.to_string()))?;

        access.status = DeviceAccessStatus::Revoked;

        Ok(())
    }

    pub fn remove(
        &mut self,
        device_id: &str,
        removed_by_device_id: &str,
    ) -> AccessResult<DeviceAccess> {
        self.require_owner(removed_by_device_id)?;

        self.devices
            .remove(device_id)
            .ok_or_else(|| AccessError::DeviceNotAllowed(device_id.to_string()))
    }

    pub fn get(&self, device_id: &str) -> AccessResult<&DeviceAccess> {
        self.devices
            .get(device_id)
            .ok_or_else(|| AccessError::DeviceNotAllowed(device_id.to_string()))
    }

    pub fn contains(&self, device_id: &str) -> bool {
        self.devices.contains_key(device_id)
    }

    pub fn is_active(&self, device_id: &str) -> bool {
        self.devices
            .get(device_id)
            .is_some_and(|access| access.status == DeviceAccessStatus::Active)
    }

    pub fn is_owner(&self, device_id: &str) -> bool {
        self.devices.get(device_id).is_some_and(|access| {
            access.status == DeviceAccessStatus::Active && access.role == WorkspaceRole::Owner
        })
    }

    pub fn require_active(&self, device_id: &str) -> AccessResult<()> {
        let access = self.get(device_id)?;

        match access.status {
            DeviceAccessStatus::Active => Ok(()),
            DeviceAccessStatus::Revoked => Err(AccessError::DeviceRevoked(device_id.to_string())),
        }
    }

    pub fn require_owner(&self, device_id: &str) -> AccessResult<()> {
        self.require_active(device_id)?;

        let access = self.get(device_id)?;

        if access.role != WorkspaceRole::Owner {
            return Err(AccessError::PermissionDenied(device_id.to_string()));
        }

        Ok(())
    }

    pub fn all(&self) -> impl Iterator<Item = &DeviceAccess> {
        self.devices.values()
    }

    pub fn active(&self) -> impl Iterator<Item = &DeviceAccess> {
        self.devices
            .values()
            .filter(|access| access.status == DeviceAccessStatus::Active)
    }

    pub fn active_device_ids(&self) -> impl Iterator<Item = &str> {
        self.active().map(|access| access.device_id.as_str())
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }
}

impl WorkspaceRole {
    pub fn can_manage_access(self) -> bool {
        matches!(self, WorkspaceRole::Owner)
    }

    pub fn can_sync(self) -> bool {
        matches!(self, WorkspaceRole::Owner | WorkspaceRole::Member)
    }
}
