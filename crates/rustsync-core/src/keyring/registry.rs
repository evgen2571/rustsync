use rustsync_protocol::{DeviceId, KeyId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyringRegistry {
    pub keys: BTreeMap<KeyId, WorkspaceKeyRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceKeyRecord {
    pub key_id: KeyId,

    pub generation: u64,

    pub visibility: KeyVisibility,
    pub algorithm: KeyAlgorithm,

    pub create_by_device_id: DeviceId,
    pub created_at: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KeyAlgorithm {
    XChaCha20Poly1305,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KeyVisibility {
    Shared,
    Restricted,
}

impl KeyringRegistry {
    pub fn new() -> Self {
        Self {
            keys: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, record: WorkspaceKeyRecord) {
        self.keys.insert(record.key_id.clone(), record);
    }

    pub fn get(&self, key_id: &KeyId) -> Option<&WorkspaceKeyRecord> {
        self.keys.get(key_id)
    }

    pub fn get_mut(&mut self, key_id: &KeyId) -> Option<&mut WorkspaceKeyRecord> {
        self.keys.get_mut(key_id)
    }

    pub fn contains(&self, key_id: &KeyId) -> bool {
        self.keys.contains_key(key_id)
    }

    pub fn all(&self) -> impl Iterator<Item = &WorkspaceKeyRecord> {
        self.keys.values()
    }
}

impl Default for KeyringRegistry {
    fn default() -> Self {
        Self::new()
    }
}
