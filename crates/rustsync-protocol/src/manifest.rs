use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{UnixTimestamp, WorkspaceId};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub workspace_id: WorkspaceId,
    pub entries: BTreeMap<String, ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ManifestEntry {
    File(FileEntry),
    Directory(DirectoryEntry),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub size: u64,
    pub content_hash: String,
    pub modified_at: UnixTimestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectoryEntry {}

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

impl ManifestEntry {
    pub fn file(size: u64, content_hash: String, modified_at: UnixTimestamp) -> Self {
        Self::File(FileEntry {
            size,
            content_hash,
            modified_at,
        })
    }

    pub fn directory() -> Self {
        Self::Directory(DirectoryEntry {})
    }
}
