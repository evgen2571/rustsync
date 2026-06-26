use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use rustsync_protocol::{Manifest, ManifestEntry, WorkspaceId};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::manifest::{
    ManifestChange, ManifestDiff, build_manifest, diff_manifests, load_manifest, save_manifest,
    validate_manifest_workspace,
};

use super::{WORKSPACE_DIR, Workspace, open_workspace};

pub type LocalWorkspaceResult<T> = Result<T, LocalWorkspaceError>;

#[derive(Debug, Error)]
pub enum LocalWorkspaceError {
    #[error(transparent)]
    Workspace(#[from] super::WorkspaceError),

    #[error(transparent)]
    Manifest(#[from] crate::error::ManifestError),

    #[error("workspace I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("nothing staged for push; run `rustsync add -A` first")]
    MissingStagedManifest,

    #[error("missing staged blob for {content_hash}; run `rustsync add -A` again ({source})")]
    MissingStagedBlob {
        content_hash: String,
        source: std::io::Error,
    },

    #[error("remote manifest contains unsafe path `{path}`")]
    UnsafeManifestPath { path: String },

    #[error("blob `{content_hash}` content hash mismatch")]
    BlobHashMismatch { content_hash: String },

    #[error("failed to fetch blob `{content_hash}`: {source}")]
    BlobSource {
        content_hash: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

#[derive(Debug, Clone)]
pub struct LocalWorkspaceEngine {
    workspace: Workspace,
}

#[derive(Debug, Clone)]
pub struct StageReport {
    pub manifest: Manifest,
    pub diff: ManifestDiff,
    pub summary: StageSummary,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StageSummary {
    pub added: usize,
    pub modified: usize,
    pub deleted: usize,
}

impl StageSummary {
    pub fn from_changes(changes: &[ManifestChange]) -> Self {
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

#[derive(Debug, Clone)]
pub struct StagedBlob {
    pub content_hash: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub removed_paths: usize,
    pub written_files: usize,
    pub prepared_directories: usize,
    pub cached_blobs: usize,
}

#[derive(Debug, Clone)]
pub struct WorkingTreeStatus {
    pub current: Manifest,
    pub baseline: Manifest,
    pub diff: ManifestDiff,
    pub has_baseline: bool,
}

impl LocalWorkspaceEngine {
    pub fn open(path: impl AsRef<Path>) -> LocalWorkspaceResult<Self> {
        Ok(Self {
            workspace: open_workspace(path)?,
        })
    }

    pub fn new(workspace: Workspace) -> Self {
        Self { workspace }
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn workspace_id(&self) -> &WorkspaceId {
        self.workspace.workspace_id()
    }

    pub fn stage_all(&self) -> LocalWorkspaceResult<StageReport> {
        let previous = self
            .load_manifest_optional()?
            .unwrap_or_else(|| Manifest::new(self.workspace.config.workspace_id.clone()));
        let next = build_manifest(&self.workspace)?;

        self.cache_manifest_blobs(&next)?;
        save_manifest(&self.workspace, &next)?;

        let diff = diff_manifests(&previous, &next);
        let summary = StageSummary::from_changes(&diff.changes);

        Ok(StageReport {
            manifest: next,
            diff,
            summary,
        })
    }

    pub fn load_staged_manifest(&self) -> LocalWorkspaceResult<Manifest> {
        self.load_manifest_optional()?
            .ok_or(LocalWorkspaceError::MissingStagedManifest)
    }

    pub fn staged_blobs_for_manifest(
        &self,
        manifest: &Manifest,
    ) -> LocalWorkspaceResult<Vec<StagedBlob>> {
        validate_manifest_workspace(&self.workspace, manifest)?;

        let mut blobs = Vec::new();
        for entry in manifest.entries.values() {
            let ManifestEntry::File(file) = entry else {
                continue;
            };

            let bytes = fs::read(staged_blob_path(&self.workspace, &file.content_hash)).map_err(
                |source| LocalWorkspaceError::MissingStagedBlob {
                    content_hash: file.content_hash.clone(),
                    source,
                },
            )?;
            verify_blob_hash(&file.content_hash, &bytes)?;
            blobs.push(StagedBlob {
                content_hash: file.content_hash.clone(),
                bytes,
            });
        }

        Ok(blobs)
    }

    pub fn validate_pulled_manifest(&self, manifest: &Manifest) -> LocalWorkspaceResult<()> {
        validate_manifest_workspace(&self.workspace, manifest)?;
        validate_manifest_paths(manifest)
    }

    pub fn apply_pulled_manifest<F, E>(
        &self,
        manifest: &Manifest,
        mut blob_source: F,
    ) -> LocalWorkspaceResult<ApplyReport>
    where
        F: FnMut(&str) -> Result<Vec<u8>, E>,
        E: std::error::Error + Send + Sync + 'static,
    {
        validate_manifest_workspace(&self.workspace, manifest)?;
        validate_manifest_paths(manifest)?;

        let mut fetched_blobs = std::collections::BTreeMap::new();
        for entry in manifest.entries.values() {
            let ManifestEntry::File(file) = entry else {
                continue;
            };
            if fetched_blobs.contains_key(&file.content_hash) {
                continue;
            }
            let bytes = blob_source(&file.content_hash).map_err(|source| {
                LocalWorkspaceError::BlobSource {
                    content_hash: file.content_hash.clone(),
                    source: Box::new(source),
                }
            })?;
            verify_blob_hash(&file.content_hash, &bytes)?;
            fetched_blobs.insert(file.content_hash.clone(), bytes);
        }

        let mut report = ApplyReport {
            removed_paths: self.remove_entries_missing_from_remote(manifest)?,
            ..ApplyReport::default()
        };

        for (relative_path, entry) in &manifest.entries {
            let target = self.workspace_path(relative_path)?;
            match entry {
                ManifestEntry::Directory(_) => {
                    prepare_directory_path(&target)?;
                    report.prepared_directories += 1;
                }
                ManifestEntry::File(file) => {
                    prepare_file_path(&target)?;
                    let bytes = fetched_blobs
                        .get(&file.content_hash)
                        .expect("blob prefetch should include every file entry");
                    fs::write(&target, bytes)?;
                    if cache_blob(&self.workspace, &file.content_hash, bytes)? {
                        report.cached_blobs += 1;
                    }
                    report.written_files += 1;
                }
            }
        }

        save_manifest(&self.workspace, manifest)?;
        Ok(report)
    }

    pub fn working_tree_status(&self) -> LocalWorkspaceResult<WorkingTreeStatus> {
        let current = build_manifest(&self.workspace)?;
        let (baseline, has_baseline) = match self.load_manifest_optional()? {
            Some(manifest) => (manifest, true),
            None => (
                Manifest::new(self.workspace.config.workspace_id.clone()),
                false,
            ),
        };
        let diff = diff_manifests(&baseline, &current);

        Ok(WorkingTreeStatus {
            current,
            baseline,
            diff,
            has_baseline,
        })
    }

    fn load_manifest_optional(&self) -> LocalWorkspaceResult<Option<Manifest>> {
        let manifest = load_manifest(&self.workspace)?;
        if let Some(manifest) = &manifest {
            validate_manifest_workspace(&self.workspace, manifest)?;
        }
        Ok(manifest)
    }

    fn cache_manifest_blobs(&self, manifest: &Manifest) -> LocalWorkspaceResult<()> {
        for (relative_path, entry) in &manifest.entries {
            let ManifestEntry::File(file) = entry else {
                continue;
            };

            let target = staged_blob_path(&self.workspace, &file.content_hash);
            if target.try_exists()? && verify_cached_blob(&target, &file.content_hash)? {
                continue;
            }

            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(self.workspace.layout.root.join(relative_path), target)?;
        }

        Ok(())
    }

    fn remove_entries_missing_from_remote(&self, remote: &Manifest) -> LocalWorkspaceResult<usize> {
        let mut local_paths = Vec::new();
        collect_workspace_paths(
            &self.workspace.layout.root,
            &self.workspace.layout.root,
            &mut local_paths,
        )?;
        local_paths.sort_by_key(|path| std::cmp::Reverse(path.matches('/').count()));

        let mut removed = 0;
        for relative_path in local_paths {
            if remote.entries.contains_key(&relative_path) {
                continue;
            }

            let target = self.workspace_path(&relative_path)?;
            if remove_path_if_exists(&target)? {
                removed += 1;
            }
        }

        Ok(removed)
    }

    fn workspace_path(&self, relative_path: &str) -> LocalWorkspaceResult<PathBuf> {
        validate_manifest_path(relative_path)?;
        Ok(self.workspace.layout.root.join(Path::new(relative_path)))
    }
}

pub fn staged_blob_path(workspace: &Workspace, content_hash: &str) -> PathBuf {
    workspace
        .layout
        .rustsync_dir
        .join("blobs")
        .join(content_hash)
}

fn validate_manifest_paths(manifest: &Manifest) -> LocalWorkspaceResult<()> {
    for path in manifest.entries.keys() {
        validate_manifest_path(path)?;
    }
    Ok(())
}

fn validate_manifest_path(relative_path: &str) -> LocalWorkspaceResult<()> {
    let path = Path::new(relative_path);
    let unsafe_path = relative_path.is_empty()
        || relative_path == WORKSPACE_DIR
        || relative_path.starts_with(&format!("{WORKSPACE_DIR}/"))
        || relative_path.contains('\\')
        || relative_path
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::CurDir
                    | Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        });

    if unsafe_path {
        return Err(LocalWorkspaceError::UnsafeManifestPath {
            path: relative_path.to_string(),
        });
    }

    Ok(())
}

fn collect_workspace_paths(
    root: &Path,
    current: &Path,
    out: &mut Vec<String>,
) -> LocalWorkspaceResult<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        if path.parent() == Some(root) && file_name == WORKSPACE_DIR {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .map_err(|_| LocalWorkspaceError::UnsafeManifestPath {
                path: path.display().to_string(),
            })?
            .to_string_lossy()
            .replace('\\', "/");
        out.push(relative);

        if entry.file_type()?.is_dir() {
            collect_workspace_paths(root, &path, out)?;
        }
    }

    Ok(())
}

fn prepare_directory_path(path: &Path) -> LocalWorkspaceResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            fs::remove_file(path)?;
            fs::create_dir_all(path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(path)?,
        Err(error) => return Err(error.into()),
    }

    Ok(())
}

fn prepare_file_path(path: &Path) -> LocalWorkspaceResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    Ok(())
}

fn remove_path_if_exists(path: &Path) -> LocalWorkspaceResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    }

    Ok(true)
}

fn cache_blob(
    workspace: &Workspace,
    content_hash: &str,
    bytes: &[u8],
) -> LocalWorkspaceResult<bool> {
    let target = staged_blob_path(workspace, content_hash);
    if target.try_exists()? && verify_cached_blob(&target, content_hash)? {
        return Ok(false);
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, bytes)?;

    Ok(true)
}

fn verify_cached_blob(path: &Path, content_hash: &str) -> LocalWorkspaceResult<bool> {
    let bytes = fs::read(path)?;
    Ok(hex::encode(Sha256::digest(&bytes)) == content_hash)
}

fn verify_blob_hash(content_hash: &str, bytes: &[u8]) -> LocalWorkspaceResult<()> {
    let actual = hex::encode(Sha256::digest(bytes));
    if actual != content_hash {
        return Err(LocalWorkspaceError::BlobHashMismatch {
            content_hash: content_hash.to_string(),
        });
    }
    Ok(())
}
