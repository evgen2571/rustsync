use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManifestEntry {
    File(FileEntry),
    Directory(DirectoryEntry),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub size: u64,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryEntry {}

impl ManifestEntry {
    pub fn file(size: u64, content_hash: String) -> Self {
        Self::File(FileEntry { size, content_hash })
    }

    pub fn directory() -> Self {
        Self::Directory(DirectoryEntry {})
    }
}
