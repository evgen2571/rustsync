use rustsync_protocol::{
    AccessEvent, DeviceId, KeyId, SignedAccessEvent, WorkspaceId, WorkspacePermission,
    WorkspaceRole, id::AccessEventId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::{AccessError, AccessResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessState {
    workspace_id: WorkspaceId,
    revision: u64,
    last_event_id: Option<AccessEventId>,
    memberships: BTreeMap<DeviceId, Membership>,
    key_grants: BTreeMap<KeyVersion, BTreeMap<DeviceId, KeyGrant>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Membership {
    pub device_id: DeviceId,
    pub role: WorkspaceRole,
    pub status: MembershipStatus,
    pub joined_by_device_id: DeviceId,
    pub joined_at: u64,
    pub joined_at_revision: u64,
    pub removed_by_device_id: Option<DeviceId>,
    pub removed_at: Option<u64>,
    pub removed_at_revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MembershipStatus {
    Active,
    Removed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyVersion {
    pub key_id: KeyId,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyGrant {
    pub key_id: KeyId,
    pub generation: u64,
    pub device_id: DeviceId,
    pub granted_by_device_id: DeviceId,
    pub granted_at: u64,
    pub granted_at_revision: u64,
    pub revoked_by_device_id: Option<DeviceId>,
    pub revoked_at: Option<u64>,
    pub revoked_at_revision: Option<u64>,
}

impl AccessState {
    pub fn empty(workspace_id: WorkspaceId) -> Self {
        Self {
            workspace_id,
            revision: 0,
            last_event_id: None,
            memberships: BTreeMap::new(),
            key_grants: BTreeMap::new(),
        }
    }

    pub fn workspace_id(&self) -> &WorkspaceId {
        &self.workspace_id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn last_event_id(&self) -> Option<&AccessEventId> {
        self.last_event_id.as_ref()
    }

    pub fn membership(&self, device_id: &DeviceId) -> Option<&Membership> {
        self.memberships.get(device_id)
    }

    pub fn active_membership(&self, device_id: &DeviceId) -> AccessResult<&Membership> {
        let membership =
            self.membership(device_id)
                .ok_or_else(|| AccessError::DeviceNotActiveMember {
                    device_id: device_id.clone(),
                })?;

        if membership.status != MembershipStatus::Active {
            return Err(AccessError::DeviceNotActiveMember {
                device_id: device_id.clone(),
            });
        }

        Ok(membership)
    }

    pub fn role(&self, device_id: &DeviceId) -> AccessResult<WorkspaceRole> {
        Ok(self.active_membership(device_id)?.role)
    }

    pub fn require_permission(
        &self,
        device_id: &DeviceId,
        permission: WorkspacePermission,
    ) -> AccessResult<()> {
        let membership = self.active_membership(device_id)?;

        if membership.role.allows(permission) {
            Ok(())
        } else {
            Err(AccessError::PermissionDenied {
                device_id: device_id.clone(),
                permission,
            })
        }
    }

    pub fn is_active_member(&self, device_id: &DeviceId) -> bool {
        self.active_membership(device_id).is_ok()
    }

    pub fn active_members(&self) -> impl Iterator<Item = &Membership> {
        self.memberships
            .values()
            .filter(|membership| membership.status == MembershipStatus::Active)
    }

    pub fn all_memberships(&self) -> impl Iterator<Item = &Membership> {
        self.memberships.values()
    }

    pub fn active_owner_count(&self) -> usize {
        self.active_members()
            .filter(|membership| membership.role == WorkspaceRole::Owner)
            .count()
    }

    pub fn key_grant(
        &self,
        key_id: &KeyId,
        generation: u64,
        device_id: &DeviceId,
    ) -> Option<&KeyGrant> {
        let version = KeyVersion {
            key_id: key_id.clone(),
            generation,
        };

        self.key_grants
            .get(&version)
            .and_then(|grants| grants.get(device_id))
    }

    pub fn has_active_key_grant(
        &self,
        key_id: &KeyId,
        generation: u64,
        device_id: &DeviceId,
    ) -> bool {
        self.key_grant(key_id, generation, device_id)
            .is_some_and(|grant| grant.revoked_at_revision.is_none())
    }

    pub fn require_key_grant(
        &self,
        key_id: &KeyId,
        generation: u64,
        device_id: &DeviceId,
    ) -> AccessResult<&KeyGrant> {
        let grant = self
            .key_grant(key_id, generation, device_id)
            .filter(|grant| grant.revoked_at_revision.is_none())
            .ok_or_else(|| AccessError::DeviceNotAuthorizedForKey {
                key_id: key_id.clone(),
                generation,
                device_id: device_id.clone(),
            })?;

        Ok(grant)
    }

    pub fn grants_for_device(&self, device_id: &DeviceId) -> impl Iterator<Item = &KeyGrant> {
        self.key_grants
            .values()
            .filter_map(move |grants| grants.get(device_id))
            .filter(|grant| grant.revoked_at_revision.is_none())
    }

    pub fn grants_for_key(
        &self,
        key_id: &KeyId,
        generation: u64,
    ) -> impl Iterator<Item = &KeyGrant> {
        let version = KeyVersion {
            key_id: key_id.clone(),
            generation,
        };

        self.key_grants
            .get(&version)
            .into_iter()
            .flat_map(|grants| grants.values())
            .filter(|grant| grant.revoked_at_revision.is_none())
    }

    pub fn apply_verified_event(&mut self, event: &SignedAccessEvent) -> AccessResult<()> {
        event.validate()?;

        if self.workspace_id != event.workspace_id {
            return Err(AccessError::WorkspaceIdMismatch {
                expected: self.workspace_id.clone(),
                actual: event.workspace_id.clone(),
            });
        }

        if self.revision != event.expected_revision {
            return Err(AccessError::InvalidState(format!(
                "access event `{}` expected revision {}, but local revision is {}",
                event.event_id, event.expected_revision, self.revision
            )));
        }

        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(AccessError::RevisionOverflow)?;

        match &event.event {
            AccessEvent::WorkspaceCreated { owner } => {
                if self.revision != 0 || !self.memberships.is_empty() {
                    return Err(AccessError::InvalidState(
                        "workspace-created event can only initialize an empty access state"
                            .to_string(),
                    ));
                }

                if event.actor_device_id != owner.device_id {
                    return Err(AccessError::InvalidState(format!(
                        "workspace-created actor `{}` does not match owner `{}`",
                        event.actor_device_id, owner.device_id
                    )));
                }

                self.insert_membership(Membership {
                    device_id: owner.device_id.clone(),
                    role: WorkspaceRole::Owner,
                    status: MembershipStatus::Active,
                    joined_by_device_id: owner.device_id.clone(),
                    joined_at: event.created_at.as_secs(),
                    joined_at_revision: next_revision,
                    removed_by_device_id: None,
                    removed_at: None,
                    removed_at_revision: None,
                });
            }
            AccessEvent::DeviceJoined { device, role, .. } => {
                self.require_event_permission(event)?;

                if self.membership(&device.device_id).is_some() {
                    return Err(AccessError::DeviceAlreadyMember(device.device_id.clone()));
                }

                self.insert_membership(Membership {
                    device_id: device.device_id.clone(),
                    role: *role,
                    status: MembershipStatus::Active,
                    joined_by_device_id: event.actor_device_id.clone(),
                    joined_at: event.created_at.as_secs(),
                    joined_at_revision: next_revision,
                    removed_by_device_id: None,
                    removed_at: None,
                    removed_at_revision: None,
                });
            }
            AccessEvent::DeviceRoleChanged {
                device_id,
                new_role,
            } => {
                self.require_event_permission(event)?;

                if *new_role != WorkspaceRole::Owner {
                    let membership = self.active_membership(device_id)?;
                    if membership.role == WorkspaceRole::Owner && self.active_owner_count() == 1 {
                        return Err(AccessError::CannotRemoveLastOwner);
                    }
                }

                self.membership_mut(device_id)?.role = *new_role;
            }
            AccessEvent::DeviceRemoved { device_id } => {
                self.require_event_permission(event)?;

                let membership = self.active_membership(device_id)?;
                if membership.role == WorkspaceRole::Owner && self.active_owner_count() == 1 {
                    return Err(AccessError::CannotRemoveLastOwner);
                }

                let membership = self.membership_mut(device_id)?;
                membership.status = MembershipStatus::Removed;
                membership.removed_by_device_id = Some(event.actor_device_id.clone());
                membership.removed_at = Some(event.created_at.as_secs());
                membership.removed_at_revision = Some(next_revision);

                self.revoke_all_for_device(
                    device_id,
                    &event.actor_device_id,
                    event.created_at.as_secs(),
                    next_revision,
                );
            }
            AccessEvent::RestrictedKeyGranted {
                key_id,
                key_generation,
                device_id,
            } => {
                self.require_event_permission(event)?;
                self.active_membership(device_id)?;

                if self.has_active_key_grant(key_id, *key_generation, device_id) {
                    return Err(AccessError::KeyAccessAlreadyGranted {
                        key_id: key_id.clone(),
                        generation: *key_generation,
                        device_id: device_id.clone(),
                    });
                }
                self.insert_key_grant(KeyGrant {
                    key_id: key_id.clone(),
                    generation: *key_generation,
                    device_id: device_id.clone(),
                    granted_by_device_id: event.actor_device_id.clone(),
                    granted_at: event.created_at.as_secs(),
                    granted_at_revision: next_revision,
                    revoked_by_device_id: None,
                    revoked_at: None,
                    revoked_at_revision: None,
                });
            }
            AccessEvent::RestrictedKeyRevoked {
                key_id,
                key_generation,
                device_id,
            } => {
                self.require_event_permission(event)?;

                let grant = self.key_grant_mut(key_id, *key_generation, device_id)?;
                if grant.revoked_at_revision.is_some() {
                    return Err(AccessError::KeyAccessNotGranted {
                        key_id: key_id.clone(),
                        generation: *key_generation,
                        device_id: device_id.clone(),
                    });
                }

                grant.revoked_by_device_id = Some(event.actor_device_id.clone());
                grant.revoked_at = Some(event.created_at.as_secs());
                grant.revoked_at_revision = Some(next_revision);
            }
        }

        self.commit_event(event.event_id.clone(), next_revision);
        self.validate()
    }

    fn require_event_permission(&self, event: &SignedAccessEvent) -> AccessResult<()> {
        if let Some(permission) = event.event.required_permission() {
            self.require_permission(&event.actor_device_id, permission)?;
        }

        Ok(())
    }

    pub fn validate(&self) -> AccessResult<()> {
        if self.revision == 0 {
            if !self.memberships.is_empty() || self.last_event_id.is_some() {
                return Err(AccessError::InvalidState(
                    "revision 0 state must not contain applied access events".to_string(),
                ));
            }

            return Ok(());
        }

        if self.active_owner_count() == 0 {
            return Err(AccessError::CannotRemoveLastOwner);
        }

        for (device_id, membership) in &self.memberships {
            if device_id != &membership.device_id {
                return Err(AccessError::InvalidState(format!(
                    "membership map key `{device_id}` does not match record `{}`",
                    membership.device_id
                )));
            }

            if membership.joined_at_revision == 0 || membership.joined_at_revision > self.revision {
                return Err(AccessError::InvalidState(format!(
                    "membership for `{device_id}` has invalid join revision {}",
                    membership.joined_at_revision
                )));
            }

            match membership.status {
                MembershipStatus::Active => {
                    if membership.removed_at_revision.is_some()
                        || membership.removed_at.is_some()
                        || membership.removed_by_device_id.is_some()
                    {
                        return Err(AccessError::InvalidState(format!(
                            "active membership for `{device_id}` contains removal metadata"
                        )));
                    }
                }
                MembershipStatus::Removed => {
                    let removed_revision = membership.removed_at_revision.ok_or_else(|| {
                        AccessError::InvalidState(format!(
                            "removed membership for `{device_id}` has no removal revision"
                        ))
                    })?;

                    if removed_revision > self.revision {
                        return Err(AccessError::InvalidState(format!(
                            "removed membership for `{device_id}` has future revision"
                        )));
                    }
                }
            }
        }

        for (version, grants) in &self.key_grants {
            if version.generation == 0 {
                return Err(AccessError::InvalidState(format!(
                    "key `{}` contains generation 0",
                    version.key_id
                )));
            }

            for (device_id, grant) in grants {
                if device_id != &grant.device_id
                    || version.key_id != grant.key_id
                    || version.generation != grant.generation
                {
                    return Err(AccessError::InvalidState(
                        "key grant index does not match grant record".to_string(),
                    ));
                }

                if grant.granted_at_revision == 0 || grant.granted_at_revision > self.revision {
                    return Err(AccessError::InvalidState(format!(
                        "grant for key `{}` and device `{device_id}` has invalid revision",
                        version.key_id
                    )));
                }
            }
        }

        Ok(())
    }

    pub(crate) fn insert_membership(&mut self, membership: Membership) {
        self.memberships
            .insert(membership.device_id.clone(), membership);
    }

    pub(crate) fn membership_mut(&mut self, device_id: &DeviceId) -> AccessResult<&mut Membership> {
        self.memberships
            .get_mut(device_id)
            .ok_or_else(|| AccessError::DeviceNotActiveMember {
                device_id: device_id.clone(),
            })
    }

    pub(crate) fn insert_key_grant(&mut self, grant: KeyGrant) {
        let version = KeyVersion {
            key_id: grant.key_id.clone(),
            generation: grant.generation,
        };

        self.key_grants
            .entry(version)
            .or_default()
            .insert(grant.device_id.clone(), grant);
    }

    pub(crate) fn key_grant_mut(
        &mut self,
        key_id: &KeyId,
        generation: u64,
        device_id: &DeviceId,
    ) -> AccessResult<&mut KeyGrant> {
        let version = KeyVersion {
            key_id: key_id.clone(),
            generation,
        };

        self.key_grants
            .get_mut(&version)
            .and_then(|grants| grants.get_mut(device_id))
            .ok_or_else(|| AccessError::KeyAccessNotGranted {
                key_id: key_id.clone(),
                generation,
                device_id: device_id.clone(),
            })
    }

    pub(crate) fn revoke_all_for_device(
        &mut self,
        device_id: &DeviceId,
        revoked_by_device_id: &DeviceId,
        revoked_at: u64,
        revoked_at_revision: u64,
    ) -> Vec<KeyVersion> {
        let mut affected = Vec::new();

        for (version, grants) in &mut self.key_grants {
            if let Some(grant) = grants.get_mut(device_id)
                && grant.revoked_at_revision.is_none()
            {
                grant.revoked_by_device_id = Some(revoked_by_device_id.clone());
                grant.revoked_at = Some(revoked_at);
                grant.revoked_at_revision = Some(revoked_at_revision);
                affected.push(version.clone());
            }
        }

        affected
    }

    pub(crate) fn commit_event(&mut self, event_id: AccessEventId, next_revision: u64) {
        self.revision = next_revision;
        self.last_event_id = Some(event_id);
    }
}
