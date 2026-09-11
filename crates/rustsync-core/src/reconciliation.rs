use std::collections::{BTreeMap, BTreeSet};

use rustsync_protocol::{Manifest, ManifestEntry, ManifestId, WorkspaceHead};
use serde::{Deserialize, Serialize};

use crate::manifest::ManifestError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationAction {
    Unchanged,
    Upload,
    Download,
    Merge,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReconciliationPath {
    pub path: String,
    pub action: ReconciliationAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReconciliationPlan {
    pub paths: Vec<ReconciliationPath>,
}

impl ReconciliationPlan {
    #[must_use]
    pub fn has_changes(&self) -> bool {
        self.paths
            .iter()
            .any(|path| path.action != ReconciliationAction::Unchanged)
    }

    pub fn conflicts(&self) -> impl Iterator<Item = &ReconciliationPath> {
        self.paths
            .iter()
            .filter(|path| path.action == ReconciliationAction::Conflict)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncPhase {
    Discovery,
    Planning,
    Upload,
    Download,
    Merge,
    Publication,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteSyncState {
    pub head: WorkspaceHead,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublishedSyncState {
    pub manifest_id: ManifestId,
    pub head_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingSyncOperation {
    pub phase: SyncPhase,
    pub observed_remote_revision: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncState {
    /// Revision successfully applied to the local working tree, independent of later observations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_manifest: Option<Manifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observed_remote: Option<RemoteSyncState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_published_local: Option<PublishedSyncState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_operation: Option<PendingSyncOperation>,
    #[serde(default)]
    pub conflicts: BTreeMap<String, ConflictRecord>,
}

impl SyncState {
    pub fn begin_pending(&mut self, phase: SyncPhase, observed_remote_revision: u64) {
        self.pending_operation = Some(PendingSyncOperation {
            phase,
            observed_remote_revision,
        });
    }

    pub fn complete_pending(&mut self) {
        self.pending_operation = None;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConflictRecord {
    pub local_copy_path: String,
    pub remote_copy_path: String,
    #[serde(default)]
    pub remote_deleted: bool,
}

pub fn plan_reconciliation(
    base: &Manifest,
    local: &Manifest,
    remote: &Manifest,
) -> Result<ReconciliationPlan, ManifestError> {
    if base.workspace_id != local.workspace_id || base.workspace_id != remote.workspace_id {
        return Err(ManifestError::WorkspaceIdMismatch {
            manifest_workspace_id: local.workspace_id.clone(),
            current_workspace_id: base.workspace_id.clone(),
        });
    }

    let paths: BTreeSet<_> = base
        .entries
        .keys()
        .chain(local.entries.keys())
        .chain(remote.entries.keys())
        .cloned()
        .collect();

    let mut planned = Vec::new();
    let mut conflict_roots: Vec<String> = Vec::new();
    for path in paths {
        if conflict_roots
            .iter()
            .any(|root| path.starts_with(&format!("{root}/")))
        {
            continue;
        }
        let local_entry = local.entries.get(&path);
        let remote_entry = remote.entries.get(&path);
        let structural = matches!(local_entry, Some(ManifestEntry::Directory(_)))
            != matches!(remote_entry, Some(ManifestEntry::Directory(_)));
        let action = if structural {
            if subtrees_equal(base, local, &path) {
                ReconciliationAction::Download
            } else if subtrees_equal(base, remote, &path) {
                ReconciliationAction::Upload
            } else {
                conflict_roots.push(path.clone());
                ReconciliationAction::Conflict
            }
        } else {
            reconcile_entry(base.entries.get(&path), local_entry, remote_entry)
        };
        planned.push(ReconciliationPath { path, action });
    }
    let paths = planned;

    Ok(ReconciliationPlan { paths })
}

fn subtrees_equal(left: &Manifest, right: &Manifest, root: &str) -> bool {
    let prefix = format!("{root}/");
    left.entries
        .keys()
        .chain(right.entries.keys())
        .filter(|path| path.as_str() == root || path.starts_with(&prefix))
        .all(|path| entries_equal(left.entries.get(path), right.entries.get(path)))
}

fn reconcile_entry(
    base: Option<&ManifestEntry>,
    local: Option<&ManifestEntry>,
    remote: Option<&ManifestEntry>,
) -> ReconciliationAction {
    if entries_equal(local, remote) {
        return ReconciliationAction::Unchanged;
    }
    if entries_equal(base, local) {
        return ReconciliationAction::Download;
    }
    if entries_equal(base, remote) {
        return ReconciliationAction::Upload;
    }

    match (local, remote) {
        (Some(ManifestEntry::File(_)), Some(ManifestEntry::File(_))) => ReconciliationAction::Merge,
        _ => ReconciliationAction::Conflict,
    }
}

fn entries_equal(left: Option<&ManifestEntry>, right: Option<&ManifestEntry>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(ManifestEntry::Directory(_)), Some(ManifestEntry::Directory(_))) => true,
        (Some(ManifestEntry::File(left)), Some(ManifestEntry::File(right))) => {
            left.size == right.size && left.content_hash == right.content_hash
        }
        _ => false,
    }
}

/// Merges line replacements when both versions modify distinct base lines. Returns `None` for
/// binary data, line insertions/deletions, or overlapping edits so callers can preserve copies.
#[must_use]
pub fn merge_text_three_way(base: &[u8], local: &[u8], remote: &[u8]) -> Option<Vec<u8>> {
    let base = std::str::from_utf8(base).ok()?;
    let local = std::str::from_utf8(local).ok()?;
    let remote = std::str::from_utf8(remote).ok()?;
    let base_lines: Vec<_> = base.split_inclusive('\n').collect();
    let local_lines: Vec<_> = local.split_inclusive('\n').collect();
    let remote_lines: Vec<_> = remote.split_inclusive('\n').collect();
    if base_lines.len() != local_lines.len() || base_lines.len() != remote_lines.len() {
        return None;
    }

    let mut merged = String::new();
    for ((base, local), remote) in base_lines.into_iter().zip(local_lines).zip(remote_lines) {
        let local_changed = local != base;
        let remote_changed = remote != base;
        match (local_changed, remote_changed) {
            (false, false) => merged.push_str(base),
            (true, false) => merged.push_str(local),
            (false, true) => merged.push_str(remote),
            (true, true) if local == remote => merged.push_str(local),
            (true, true) => return None,
        }
    }
    Some(merged.into_bytes())
}
