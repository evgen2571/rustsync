use rustsync_protocol::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyringRegistry {
    pub keys: BTreeMap<String, WorkspaceKeyRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceKeyRecord {
    pub key_id: String,

    pub generation: u64,

    pub visibility: KeyVisibility,
    pub algorithm: KeyAlgorithm,

    pub create_by_device_id: DeviceId,
    pub created_at: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum KeyAlgorithm {
    Aes256Gcm,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

    pub fn get(&self, key_id: &str) -> Option<&WorkspaceKeyRecord> {
        self.keys.get(key_id)
    }

    pub fn get_mut(&mut self, key_id: &str) -> Option<&mut WorkspaceKeyRecord> {
        self.keys.get_mut(key_id)
    }

    pub fn contains(&self, key_id: &str) -> bool {
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
