use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::{AccessError, AccessResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceAcl {
    members: BTreeMap<String, WorkspaceMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceMember {
    pub device_id: String,
    pub role: WorkspaceRole,
    pub granted_by_device_id: Option<String>,
    pub granted_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkspaceRole {
    Owner,
    Member,
}

impl WorkspaceAcl {
    pub fn new(owner_device_id: impl Into<String>, created_at: u64) -> Self {
        let owner_device_id = owner_device_id.into();
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
        device_id: impl Into<String>,
        role: WorkspaceRole,
        granted_by_device_id: &str,
        granted_at: u64,
    ) -> AccessResult<()> {
        self.require_owner(granted_by_device_id)?;

        let device_id = device_id.into();

        if self.members.contains_key(&device_id) {
            return Err(AccessError::DeviceAlreadyMember(device_id));
        }

        self.members.insert(
            device_id.clone(),
            WorkspaceMember {
                device_id,
                role,
                granted_by_device_id: Some(granted_by_device_id.to_string()),
                granted_at,
            },
        );

        Ok(())
    }

    pub fn set_role(
        &mut self,
        device_id: &str,
        role: WorkspaceRole,
        changed_by_device_id: &str,
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
        device_id: &str,
        removed_by_device_id: &str,
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

    pub fn get(&self, device_id: &str) -> AccessResult<&WorkspaceMember> {
        self.members
            .get(device_id)
            .ok_or_else(|| AccessError::DeviceNotMember(device_id.to_string()))
    }

    pub fn require_member(&self, device_id: &str) -> AccessResult<&WorkspaceMember> {
        self.get(device_id)
    }

    pub fn require_owner(&self, device_id: &str) -> AccessResult<&WorkspaceMember> {
        let member = self.get(device_id)?;

        if member.role != WorkspaceRole::Owner {
            return Err(AccessError::PermissionDenied(device_id.to_string()));
        }

        Ok(member)
    }

    pub fn contains(&self, device_id: &str) -> bool {
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
}

impl WorkspaceRole {
    pub fn can_manage_access(self) -> bool {
        self == Self::Owner
    }

    pub fn can_sync(self) -> bool {
        matches!(self, Self::Owner | Self::Member)
    }
}
