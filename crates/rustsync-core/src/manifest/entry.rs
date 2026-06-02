use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EntryKind {
    File,
    Directory,
}

impl ManifestEntry {
    pub fn file(path: String, size: u64, content_hash: String) -> Self {
        Self {
            path,
            kind: EntryKind::File,
            size,
            content_hash,
        }
    }
}
