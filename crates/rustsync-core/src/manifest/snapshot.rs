use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::ManifestEntry;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub workspace_id: WorkspaceId,
    pub entries: BTreeMap<String, ManifestEntry>,
}

impl Manifest {
    pub fn new(workspace_id: WorkspaceId) -> Self {
        Self {
            workspace_id,
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, path: String, entry: ManifestEntry) {
        self.entries.insert(path, entry);
    }

    pub fn get(&self, path: &str) -> Option<&ManifestEntry> {
        self.entries.get(path)
    }

    pub fn contains_path(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
