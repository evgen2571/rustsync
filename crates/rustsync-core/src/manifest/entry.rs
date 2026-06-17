use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ManifestEntry {
    File(FileEntry),
    Directory(DirectoryEntry),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub size: u64,
    pub content_hash: String,
    pub modified_at_unix_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectoryEntry {}

impl ManifestEntry {
    pub fn file(size: u64, content_hash: String, modified_at_unix_seconds: u64) -> Self {
        Self::File(FileEntry {
            size,
            content_hash,
            modified_at_unix_seconds,
        })
    }

    pub fn directory() -> Self {
        Self::Directory(DirectoryEntry {})
    }
}
