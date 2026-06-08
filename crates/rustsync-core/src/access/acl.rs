use rustsync_protocol::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::{AccessError, AccessResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceAcl {
    members: BTreeMap<DeviceId, WorkspaceMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceMember {
    pub device_id: DeviceId,
    pub role: WorkspaceRole,
    pub granted_by_device_id: Option<DeviceId>,
    pub granted_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkspaceRole {
    Owner,
    Member,
}

impl WorkspaceAcl {
    pub fn new(owner_device_id: DeviceId, created_at: u64) -> Self {
        let mut members = BTreeMap::new();

        members.insert(
            owner_device_id.clone(),
            WorkspaceMember {
                device_id: owner_device_id,
                role: WorkspaceRole::Owner,
                granted_by_device_id: None,
                granted_at: created_at,
            },
        );

        Self { members }
    }

    pub fn grant(
        &mut self,
        device_id: DeviceId,
        role: WorkspaceRole,
        granted_by_device_id: &DeviceId,
        granted_at: u64,
    ) -> AccessResult<()> {
        self.require_owner(granted_by_device_id)?;

        if self.members.contains_key(&device_id) {
            return Err(AccessError::DeviceAlreadyMember(device_id));
        }

        self.members.insert(
            device_id.clone(),
            WorkspaceMember {
                device_id,
                role,
                granted_by_device_id: Some(granted_by_device_id.clone()),
                granted_at,
            },
        );

        Ok(())
    }

    pub fn set_role(
        &mut self,
        device_id: &DeviceId,
        role: WorkspaceRole,
        changed_by_device_id: &DeviceId,
    ) -> AccessResult<()> {
        self.require_owner(changed_by_device_id)?;

        let current = self.get(device_id)?;

        if current.role == WorkspaceRole::Owner
            && role != WorkspaceRole::Owner
            && self.owner_count() == 1
        {
            return Err(AccessError::CannotRemoveLastOwner);
        }

        self.members
            .get_mut(device_id)
            .expect("member was checked above")
            .role = role;

        Ok(())
    }

    pub fn remove(
        &mut self,
        device_id: &DeviceId,
        removed_by_device_id: &DeviceId,
    ) -> AccessResult<WorkspaceMember> {
        self.require_owner(removed_by_device_id)?;

        let member = self.get(device_id)?;

        if member.role == WorkspaceRole::Owner && self.owner_count() == 1 {
            return Err(AccessError::CannotRemoveLastOwner);
        }

        Ok(self
            .members
            .remove(device_id)
            .expect("member was checked above"))
    }

    pub fn get(&self, device_id: &DeviceId) -> AccessResult<&WorkspaceMember> {
        self.members
            .get(device_id)
            .ok_or_else(|| AccessError::DeviceNotMember(device_id.clone()))
    }

    pub fn require_member(&self, device_id: &DeviceId) -> AccessResult<&WorkspaceMember> {
        self.get(device_id)
    }

    pub fn require_owner(&self, device_id: &DeviceId) -> AccessResult<&WorkspaceMember> {
        let member = self.get(device_id)?;

        if member.role != WorkspaceRole::Owner {
            return Err(AccessError::PermissionDenied(device_id.clone()));
        }

        Ok(member)
    }

    pub fn contains(&self, device_id: &DeviceId) -> bool {
        self.members.contains_key(device_id)
    }

    pub fn all(&self) -> impl Iterator<Item = &WorkspaceMember> {
        self.members.values()
    }

    pub fn owners(&self) -> impl Iterator<Item = &WorkspaceMember> {
        self.members
            .values()
            .filter(|member| member.role == WorkspaceRole::Owner)
    }

    pub fn owner_count(&self) -> usize {
        self.owners().count()
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

impl WorkspaceRole {
    pub fn can_manage_access(self) -> bool {
        self == Self::Owner
    }

    pub fn can_sync(self) -> bool {
        matches!(self, Self::Owner | Self::Member)
    }
}
