use serde::{Deserialize, Serialize};
use std::{any, collections::BTreeMap};

use crate::{ProtocolError, ProtocolResult, UnixTimestamp, WorkspaceId};

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

    pub fn insert(&mut self, path: String, entry: ManifestEntry) -> ProtocolResult<()> {
        validate_manifest_path(&path)?;
        self.entries.insert(path, entry);
        Ok(())
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

fn validate_manifest_path(path: &str) -> ProtocolResult<()> {
    if path.is_empty() {
        return Err(invalid_manifest_path(path, "path must not be empty"));
    }

    if path.starts_with('/') {
        return Err(invalid_manifest_path(path, "path must be relative"));
    }

    if path.contains('\\') {
        return Err(invalid_manifest_path(path, "path must use `/` separators"));
    }

    if path
        .split('/')
        .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(invalid_manifest_path(
            path,
            "path must be normalized and must not contain empty, `.`, or `..` components",
        ));
    }

    Ok(())
}

fn invalid_manifest_path(path: &str, reason: impl Into<String>) -> ProtocolError {
    ProtocolError::InvalidManifestPath {
        path: path.to_string(),
        reason: reason.into(),
    }
}
