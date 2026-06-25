use rustsync_core::{
    manifest::{build_manifest, diff_manifests, load_manifest, save_manifest, ManifestChange},
    workspace::Workspace,
};
use rustsync_protocol::{Manifest, ManifestEntry};
use std::{error::Error, fs, path::PathBuf};

pub fn run(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::open(&path)?;
    let previous = load_manifest(&workspace)?.unwrap_or_else(|| empty_manifest(&workspace));
    let next = build_manifest(&workspace)?;

    cache_manifest_blobs(&workspace, &next)?;
    save_manifest(&workspace, &next)?;

    let diff = diff_manifests(&previous, &next);
    let summary = StageSummary::from_changes(diff.changes);

    println!(
        "staged {} added, {} modified, {} deleted path(s) for push",
        summary.added, summary.modified, summary.deleted
    );

    Ok(())
}

pub(crate) fn staged_blob_path(workspace: &Workspace, content_hash: &str) -> PathBuf {
    workspace
        .layout
        .rustsync_dir
        .join("blobs")
        .join(content_hash)
}

fn cache_manifest_blobs(workspace: &Workspace, manifest: &Manifest) -> Result<(), Box<dyn Error>> {
    for (relative_path, entry) in &manifest.entries {
        let ManifestEntry::File(file) = entry else {
            continue;
        };

        let target = staged_blob_path(workspace, &file.content_hash);
        if target.try_exists()? {
            continue;
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::copy(workspace.layout.root.join(relative_path), target)?;
    }

    Ok(())
}

fn empty_manifest(workspace: &Workspace) -> Manifest {
    Manifest::new(workspace.config.workspace_id.clone())
}

#[derive(Default)]
struct StageSummary {
    added: usize,
    modified: usize,
    deleted: usize,
}

impl StageSummary {
    fn from_changes(changes: Vec<ManifestChange>) -> Self {
        let mut summary = Self::default();

        for change in changes {
            match change {
                ManifestChange::Added { .. } => summary.added += 1,
                ManifestChange::Modified { .. } => summary.modified += 1,
                ManifestChange::Deleted { .. } => summary.deleted += 1,
            }
        }

        summary
    }
}
