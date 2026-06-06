use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{AccessError, AccessResult};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyAcl {
    grants: BTreeMap<String, BTreeMap<String, KeyAccessGrant>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyAccessGrant {
    pub key_id: String,
    pub device_id: String,
    pub granted_by_device_id: String,
    pub granted_at: u64,
}

impl KeyAcl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grant(
        &mut self,
        key_id: impl Into<String>,
        device_id: impl Into<String>,
        granted_by_device_id: impl Into<String>,
        granted_at: u64,
    ) -> AccessResult<()> {
        let key_id = key_id.into();
        let device_id = device_id.into();
        let granted_by_device_id = granted_by_device_id.into();

        let key_grants = self.grants.entry(key_id.clone()).or_default();

        if key_grants.contains_key(&device_id) {
            return Err(AccessError::KeyAccessAlreadyGranted { key_id, device_id });
        }

        key_grants.insert(
            device_id.clone(),
            KeyAccessGrant {
                key_id,
                device_id,
                granted_by_device_id,
                granted_at,
            },
        );

        Ok(())
    }

    pub fn revoke(&mut self, key_id: &str, device_id: &str) -> AccessResult<KeyAccessGrant> {
        let (grant, remove_key_entry) =
            {
                let key_grants = self.grants.get_mut(key_id).ok_or_else(|| {
                    AccessError::KeyAccessNotGranted {
                        key_id: key_id.to_string(),
                        device_id: device_id.to_string(),
                    }
                })?;

                let grant = key_grants.remove(device_id).ok_or_else(|| {
                    AccessError::KeyAccessNotGranted {
                        key_id: key_id.to_string(),
                        device_id: device_id.to_string(),
                    }
                })?;

                (grant, key_grants.is_empty())
            };

        if remove_key_entry {
            self.grants.remove(key_id);
        }

        Ok(grant)
    }

    pub fn require_granted(&self, key_id: &str, device_id: &str) -> AccessResult<&KeyAccessGrant> {
        self.get(key_id, device_id)
            .ok_or_else(|| AccessError::DeviceNotAuthorizedForKey {
                key_id: key_id.to_string(),
                device_id: device_id.to_string(),
            })
    }

    pub fn get(&self, key_id: &str, device_id: &str) -> Option<&KeyAccessGrant> {
        self.grants
            .get(key_id)
            .and_then(|key_grants| key_grants.get(device_id))
    }

    pub fn is_granted(&self, key_id: &str, device_id: &str) -> bool {
        self.get(key_id, device_id).is_some()
    }

    pub fn grants_for_key(&self, key_id: &str) -> impl Iterator<Item = &KeyAccessGrant> {
        self.grants
            .get(key_id)
            .into_iter()
            .flat_map(|grants| grants.values())
    }

    pub fn grants_for_device(&self, device_id: &str) -> impl Iterator<Item = &KeyAccessGrant> {
        self.grants
            .values()
            .filter_map(move |grants| grants.get(device_id))
    }

    pub fn remove_device(&mut self, device_id: &str) -> Vec<String> {
        let mut affected_key_ids = Vec::new();

        self.grants.retain(|key_id, grants| {
            if grants.remove(device_id).is_some() {
                affected_key_ids.push(key_id.clone());
            }

            !grants.is_empty()
        });

        affected_key_ids
    }
}
