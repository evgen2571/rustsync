use super::{Manifest, ManifestEntry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestDiff {
    pub changes: Vec<ManifestChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestChange {
    Added { path: String },
    Modified { path: String },
    Deleted { path: String },
}

impl ManifestDiff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.changes.len()
    }
}

pub fn diff_manifests(old: &Manifest, new: &Manifest) -> ManifestDiff {
    let mut changes = Vec::new();

    for (path, new_entry) in &new.entries {
        match old.entries.get(path) {
            None => {
                changes.push(ManifestChange::Added { path: path.clone() });
            }

            Some(old_entry) if entry_changed(old_entry, new_entry) => {
                changes.push(ManifestChange::Modified { path: path.clone() });
            }

            Some(_) => {}
        }
    }

    for path in old.entries.keys() {
        if !new.entries.contains_key(path) {
            changes.push(ManifestChange::Deleted { path: path.clone() });
        }
    }

    ManifestDiff { changes }
}

fn entry_changed(old: &ManifestEntry, new: &ManifestEntry) -> bool {
    match (old, new) {
        (ManifestEntry::File(old_file), ManifestEntry::File(new_file)) => {
            old_file.size != new_file.size || old_file.content_hash != new_file.content_hash
        }

        (ManifestEntry::Directory(_), ManifestEntry::Directory(_)) => false,

        _ => true,
    }
}
