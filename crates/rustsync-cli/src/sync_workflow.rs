use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt, fs,
};

use rustsync_client::{RequestSigner, RustSyncClient};
use rustsync_core::{
    manifest::{diff_manifests, manifest_from_json_bytes, manifest_to_json_bytes},
    reconciliation::{
        ConflictRecord, PublishedSyncState, ReconciliationAction, RemoteSyncState, SyncPhase,
        merge_text_three_way, plan_reconciliation,
    },
    workspace::{ApplyReport, LocalWorkspaceEngine},
};
use rustsync_protocol::{
    ApiErrorCode, BlobId, EncryptedObject, Manifest, ManifestEntry, ManifestId,
    ObjectUploadResponse, ObjectUploadStatus, WorkspaceHead, WorkspaceId,
};

pub type SyncWorkflowResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug)]
pub struct SyncPublicationRace {
    pub initial_revision: u64,
    pub current_revision: u64,
}

impl fmt::Display for SyncPublicationRace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "remote head changed from revision {} to {} while publishing; rerun `rustsync sync`",
            self.initial_revision, self.current_revision
        )
    }
}

impl Error for SyncPublicationRace {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullMode {
    Safe,
    Force,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    Reconcile,
    DryRun,
    DiscardLocal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UnstagedChangesError;

impl fmt::Display for UnstagedChangesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "safe remote replacement refused because the local working tree has changes; back them up or use `rustsync sync --discard-local --yes` only when you intend to discard them"
        )
    }
}

impl Error for UnstagedChangesError {}

#[allow(async_fn_in_trait)]
pub trait SyncRemote {
    type Error: Error + Send + Sync + 'static;

    async fn upload_blob(
        &self,
        workspace_id: &WorkspaceId,
        blob_id: &BlobId,
        bytes: Vec<u8>,
    ) -> Result<ObjectUploadResponse, Self::Error>;

    async fn upload_manifest(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
        bytes: Vec<u8>,
    ) -> Result<ObjectUploadResponse, Self::Error>;

    async fn download_blob(
        &self,
        workspace_id: &WorkspaceId,
        blob_id: &BlobId,
    ) -> Result<Vec<u8>, Self::Error>;

    async fn download_manifest(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
    ) -> Result<Vec<u8>, Self::Error>;

    async fn fetch_workspace_head(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<WorkspaceHead, Self::Error>;

    async fn update_workspace_head(
        &self,
        workspace_id: &WorkspaceId,
        expected_revision: u64,
        manifest_id: &ManifestId,
    ) -> Result<WorkspaceHead, Self::Error>;
}

impl<S> SyncRemote for RustSyncClient<S>
where
    S: RequestSigner,
{
    type Error = rustsync_client::ClientError;

    async fn upload_blob(
        &self,
        workspace_id: &WorkspaceId,
        blob_id: &BlobId,
        bytes: Vec<u8>,
    ) -> Result<ObjectUploadResponse, Self::Error> {
        self.upload_blob(workspace_id, blob_id, bytes).await
    }

    async fn upload_manifest(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
        bytes: Vec<u8>,
    ) -> Result<ObjectUploadResponse, Self::Error> {
        self.upload_manifest(workspace_id, manifest_id, bytes).await
    }

    async fn download_blob(
        &self,
        workspace_id: &WorkspaceId,
        blob_id: &BlobId,
    ) -> Result<Vec<u8>, Self::Error> {
        self.download_blob(workspace_id, blob_id).await
    }

    async fn download_manifest(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
    ) -> Result<Vec<u8>, Self::Error> {
        self.download_manifest(workspace_id, manifest_id).await
    }

    async fn fetch_workspace_head(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<WorkspaceHead, Self::Error> {
        self.fetch_workspace_head(workspace_id).await
    }

    async fn update_workspace_head(
        &self,
        workspace_id: &WorkspaceId,
        expected_revision: u64,
        manifest_id: &ManifestId,
    ) -> Result<WorkspaceHead, Self::Error> {
        self.update_workspace_head(workspace_id, expected_revision, manifest_id)
            .await
    }
}

pub struct SyncWorkflow<R> {
    engine: LocalWorkspaceEngine,
    remote: R,
}

impl<R> SyncWorkflow<R>
where
    R: SyncRemote,
{
    #[must_use]
    pub fn new(engine: LocalWorkspaceEngine, remote: R) -> Self {
        Self { engine, remote }
    }

    pub async fn push(&self) -> SyncWorkflowResult<PushReport> {
        let previous_head = self.remote_status().await?;
        self.push_with_expected_head(previous_head).await
    }

    async fn push_with_expected_head(
        &self,
        previous_head: WorkspaceHead,
    ) -> SyncWorkflowResult<PushReport> {
        let workspace_id = self.engine.workspace_id().clone();
        let manifest = self.engine.load_staged_manifest()?;
        let changed_files = changed_files(&manifest);

        let mut uploaded_blobs = 0usize;
        let mut reused_blobs = 0usize;
        let mut remote_manifest = manifest.clone();
        let mut remote_blob_ids = BTreeMap::new();
        for blob in self.engine.staged_blobs_for_manifest(&manifest)? {
            if remote_blob_ids.contains_key(&blob.content_hash) {
                continue;
            }

            let encrypted_blob = self
                .engine
                .workspace()
                .crypto()
                .encrypt_bytes(&blob.bytes)?;
            let encrypted_blob_bytes = encrypted_blob.to_binary_bytes()?;
            let blob_id = BlobId::from_content(&encrypted_blob_bytes);
            let response = self
                .remote
                .upload_blob(&workspace_id, &blob_id, encrypted_blob_bytes)
                .await
                .map_err(boxed_error)?;
            count_upload_response(response, &mut uploaded_blobs, &mut reused_blobs);
            remote_blob_ids.insert(blob.content_hash, blob_id);
        }

        for entry in remote_manifest.entries.values_mut() {
            let ManifestEntry::File(file) = entry else {
                continue;
            };
            file.remote_blob_id = remote_blob_ids.get(&file.content_hash).cloned();
        }

        let manifest_bytes = manifest_to_json_bytes(&remote_manifest)?;
        let encrypted_manifest = self
            .engine
            .workspace()
            .crypto()
            .encrypt_bytes(&manifest_bytes)?;
        let encrypted_manifest_bytes = encrypted_manifest.to_binary_bytes()?;
        let manifest_id = ManifestId::from_content(&encrypted_manifest_bytes);
        let manifest_response = self
            .remote
            .upload_manifest(&workspace_id, &manifest_id, encrypted_manifest_bytes)
            .await
            .map_err(boxed_error)?;
        let mut uploaded_manifests = 0usize;
        let mut reused_manifests = 0usize;
        count_upload_response(
            manifest_response,
            &mut uploaded_manifests,
            &mut reused_manifests,
        );

        let updated_head = self
            .remote
            .update_workspace_head(&workspace_id, previous_head.revision, &manifest_id)
            .await
            .map_err(boxed_error)?;

        Ok(PushReport {
            workspace_id,
            manifest_id,
            uploaded_blobs,
            reused_blobs,
            uploaded_manifests,
            reused_manifests,
            changed_files,
            previous_head_revision: previous_head.revision,
            updated_head_revision: updated_head.revision,
        })
    }

    pub async fn remote_status(&self) -> SyncWorkflowResult<WorkspaceHead> {
        self.remote
            .fetch_workspace_head(self.engine.workspace_id())
            .await
            .map_err(boxed_error)
    }

    pub async fn sync(&self, mode: SyncMode) -> SyncWorkflowResult<SyncReport> {
        self.sync_attempt(mode, None, 0).await
    }

    async fn sync_attempt(
        &self,
        mode: SyncMode,
        initial_revision: Option<u64>,
        retry_count: u8,
    ) -> SyncWorkflowResult<SyncReport> {
        if mode == SyncMode::DiscardLocal {
            return self.discard_local().await;
        }

        let workspace_id = self.engine.workspace_id().clone();
        let local = self.engine.working_tree_status()?.current;
        let mut state = self.engine.load_sync_state()?;
        let remote_head = self.remote_status().await?;
        let remote = self
            .load_remote_manifest(&workspace_id, &remote_head)
            .await?;
        let base = state
            .last_synced_manifest
            .clone()
            .unwrap_or_else(|| Manifest::new(workspace_id.clone()));
        let plan = plan_reconciliation(&base, &local, &remote)?;
        let mut report = SyncReport {
            workspace_id: workspace_id.clone(),
            observed_remote_revision: remote_head.revision,
            plan,
            mode,
            local_content_changed: false,
            published: false,
            retry_count,
            conflicts: Vec::new(),
        };
        if mode == SyncMode::DryRun {
            return Ok(report);
        }

        state.last_observed_remote = Some(RemoteSyncState {
            head: remote_head.clone(),
        });
        if !report.plan.has_changes() {
            let staged = self.engine.stage_all()?.manifest;
            ensure_unchanged(&local, &staged)?;
            state.last_synced_manifest = Some(staged);
            state.complete_pending();
            self.engine.save_sync_state(&state)?;
            return Ok(report);
        }
        state.begin_pending(SyncPhase::Planning, remote_head.revision);
        self.engine.save_sync_state(&state)?;

        // Stage first so every local version is addressable by its content hash during apply.
        let staged = self.engine.stage_all()?.manifest;
        ensure_unchanged(&local, &staged)?;
        let local_blobs = self.local_blob_bytes(&staged)?;
        let base_blobs = self.local_blob_bytes(&base)?;
        let mut reserved_paths: BTreeSet<String> = local
            .entries
            .keys()
            .chain(remote.entries.keys())
            .chain(base.entries.keys())
            .cloned()
            .collect();
        reserved_paths.extend(
            state
                .conflicts
                .values()
                .map(|record| record.remote_copy_path.clone()),
        );
        let remote_blobs = self.download_manifest_blobs(&workspace_id, &remote).await?;
        let mut combined = Manifest::new(workspace_id.clone());
        let mut merged_bytes = BTreeMap::new();

        for item in report.plan.paths.clone() {
            let base_entry = base.entries.get(&item.path);
            let local_entry = staged.entries.get(&item.path);
            let remote_entry = remote.entries.get(&item.path);
            match item.action {
                ReconciliationAction::Unchanged | ReconciliationAction::Upload => {
                    if let Some(entry) = local_entry {
                        combined.insert(item.path.clone(), entry.clone())?;
                    }
                }
                ReconciliationAction::Download => {
                    if let Some(entry) = remote_entry {
                        combined.insert(item.path.clone(), entry.clone())?;
                    }
                }
                ReconciliationAction::Merge => {
                    let Some(ManifestEntry::File(base_file)) = base_entry else {
                        self.add_conflict(
                            &mut combined,
                            &mut state,
                            &mut report,
                            &item.path,
                            &staged,
                            &remote,
                            remote_head.revision,
                            &mut reserved_paths,
                        )?;
                        continue;
                    };
                    let Some(ManifestEntry::File(local_file)) = local_entry else {
                        self.add_conflict(
                            &mut combined,
                            &mut state,
                            &mut report,
                            &item.path,
                            &staged,
                            &remote,
                            remote_head.revision,
                            &mut reserved_paths,
                        )?;
                        continue;
                    };
                    let Some(ManifestEntry::File(remote_file)) = remote_entry else {
                        self.add_conflict(
                            &mut combined,
                            &mut state,
                            &mut report,
                            &item.path,
                            &staged,
                            &remote,
                            remote_head.revision,
                            &mut reserved_paths,
                        )?;
                        continue;
                    };
                    let base_bytes = base_blobs.get(&base_file.content_hash);
                    let local_bytes = local_blobs.get(&local_file.content_hash);
                    let remote_bytes = remote_blobs.get(&remote_file.content_hash);
                    if let (Some(base_bytes), Some(local_bytes), Some(remote_bytes)) =
                        (base_bytes, local_bytes, remote_bytes)
                        && let Some(bytes) =
                            merge_text_three_way(base_bytes, local_bytes, remote_bytes)
                    {
                        combined.insert(
                            item.path.clone(),
                            local_entry.expect("file checked").clone(),
                        )?;
                        merged_bytes.insert(item.path.clone(), bytes);
                        continue;
                    }
                    self.add_conflict(
                        &mut combined,
                        &mut state,
                        &mut report,
                        &item.path,
                        &staged,
                        &remote,
                        remote_head.revision,
                        &mut reserved_paths,
                    )?;
                }
                ReconciliationAction::Conflict => {
                    self.add_conflict(
                        &mut combined,
                        &mut state,
                        &mut report,
                        &item.path,
                        &staged,
                        &remote,
                        remote_head.revision,
                        &mut reserved_paths,
                    )?;
                }
            }
        }

        ensure_unchanged(&staged, &self.engine.working_tree_status()?.current)?;
        state.begin_pending(SyncPhase::Download, remote_head.revision);
        self.engine.save_sync_state(&state)?;
        self.engine.apply_pulled_manifest(&combined, |hash| {
            local_blobs
                .get(hash)
                .or_else(|| remote_blobs.get(hash))
                .cloned()
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("missing planned blob {hash}"),
                    )
                })
        })?;
        report.local_content_changed = true;
        for (path, bytes) in merged_bytes {
            fs::write(self.engine.workspace().layout.root.join(path), bytes)?;
        }

        let requires_publication = report.plan.paths.iter().any(|path| {
            matches!(
                path.action,
                ReconciliationAction::Upload
                    | ReconciliationAction::Merge
                    | ReconciliationAction::Conflict
            )
        });
        let published_manifest = self.engine.stage_all()?.manifest;
        // The working tree now includes this remote revision. A retry must reconcile
        // against it rather than treating already-applied changes as new conflicts.
        state.last_synced_manifest = Some(remote);
        self.engine.save_sync_state(&state)?;
        if requires_publication {
            state.begin_pending(SyncPhase::Upload, remote_head.revision);
            self.engine.save_sync_state(&state)?;
            state.begin_pending(SyncPhase::Publication, remote_head.revision);
            self.engine.save_sync_state(&state)?;
            let published = match self.push_with_expected_head(remote_head.clone()).await {
                Ok(published) => published,
                Err(error) if is_head_revision_conflict(error.as_ref()) && retry_count == 0 => {
                    return Box::pin(self.sync_attempt(
                        SyncMode::Reconcile,
                        Some(remote_head.revision),
                        retry_count + 1,
                    ))
                    .await;
                }
                Err(error) if is_head_revision_conflict(error.as_ref()) => {
                    let current_revision = self.remote_status().await?.revision;
                    return Err(Box::new(SyncPublicationRace {
                        initial_revision: initial_revision.unwrap_or(remote_head.revision),
                        current_revision,
                    }));
                }
                Err(error) => return Err(error),
            };
            state.last_published_local = Some(PublishedSyncState {
                manifest_id: published.manifest_id,
                head_revision: published.updated_head_revision,
            });
            report.published = true;
        }
        state.last_synced_manifest = Some(published_manifest);
        state.complete_pending();
        self.engine.save_sync_state(&state)?;
        Ok(report)
    }

    async fn discard_local(&self) -> SyncWorkflowResult<SyncReport> {
        let workspace_id = self.engine.workspace_id().clone();
        let pull_report = self.pull_with_mode(PullMode::Force).await?;
        let mut state = self.engine.load_sync_state()?;
        state.last_observed_remote = Some(RemoteSyncState {
            head: WorkspaceHead {
                workspace_id: workspace_id.clone(),
                revision: pull_report.remote_head_revision,
                manifest_id: pull_report.manifest_id.clone(),
                updated_by: None,
                updated_at: None,
            },
        });
        state.last_synced_manifest = Some(self.engine.load_staged_manifest()?);
        state.conflicts.clear();
        state.complete_pending();
        self.engine.save_sync_state(&state)?;

        Ok(SyncReport {
            workspace_id,
            observed_remote_revision: pull_report.remote_head_revision,
            plan: rustsync_core::reconciliation::ReconciliationPlan { paths: Vec::new() },
            mode: SyncMode::DiscardLocal,
            local_content_changed: true,
            published: false,
            retry_count: 0,
            conflicts: Vec::new(),
        })
    }

    pub async fn pull(&self) -> SyncWorkflowResult<PullReport> {
        self.pull_with_mode(PullMode::Safe).await
    }

    pub async fn pull_with_mode(&self, mode: PullMode) -> SyncWorkflowResult<PullReport> {
        if mode == PullMode::Safe {
            self.ensure_working_tree_clean()?;
        }

        let workspace_id = self.engine.workspace_id().clone();
        let head = self
            .remote
            .fetch_workspace_head(&workspace_id)
            .await
            .map_err(boxed_error)?;

        let Some(manifest_id) = head.manifest_id.clone() else {
            let manifest = Manifest::new(workspace_id.clone());
            let apply = self
                .engine
                .apply_pulled_manifest(&manifest, |_content_hash| {
                    Err::<Vec<u8>, _>(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "empty manifest has no blobs",
                    ))
                })?;
            return Ok(PullReport::from_apply(
                workspace_id,
                head.revision,
                None,
                0,
                Vec::new(),
                apply,
            ));
        };

        let manifest_bytes = self
            .remote
            .download_manifest(&workspace_id, &manifest_id)
            .await
            .map_err(boxed_error)?;
        verify_manifest_id(&manifest_bytes, &manifest_id)?;
        let manifest_plaintext = self.decrypt_remote_object_bytes(&manifest_bytes)?;
        let manifest = manifest_from_json_bytes(&manifest_plaintext)?;
        self.engine.validate_pulled_manifest(&manifest)?;

        let mut blobs = BTreeMap::new();
        for (content_hash, blob_id) in unique_file_remote_blob_ids(&manifest)? {
            let bytes = self
                .remote
                .download_blob(&workspace_id, &blob_id)
                .await
                .map_err(boxed_error)?;
            verify_blob_id(&bytes, &blob_id)?;
            let plaintext = self.decrypt_remote_object_bytes(&bytes)?;
            blobs.insert(content_hash, plaintext);
        }
        let downloaded_blobs = blobs.len();
        let changed_files = changed_files(&manifest);

        let apply = self
            .engine
            .apply_pulled_manifest(&manifest, |content_hash| {
                blobs.get(content_hash).cloned().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("missing downloaded blob {content_hash}"),
                    )
                })
            })?;

        Ok(PullReport::from_apply(
            workspace_id,
            head.revision,
            Some(manifest_id),
            downloaded_blobs,
            changed_files,
            apply,
        ))
    }

    async fn load_remote_manifest(
        &self,
        workspace_id: &WorkspaceId,
        head: &WorkspaceHead,
    ) -> SyncWorkflowResult<Manifest> {
        let Some(manifest_id) = &head.manifest_id else {
            return Ok(Manifest::new(workspace_id.clone()));
        };
        let bytes = self
            .remote
            .download_manifest(workspace_id, manifest_id)
            .await
            .map_err(boxed_error)?;
        verify_manifest_id(&bytes, manifest_id)?;
        let manifest = manifest_from_json_bytes(&self.decrypt_remote_object_bytes(&bytes)?)?;
        self.engine.validate_pulled_manifest(&manifest)?;
        Ok(manifest)
    }

    async fn download_manifest_blobs(
        &self,
        workspace_id: &WorkspaceId,
        manifest: &Manifest,
    ) -> SyncWorkflowResult<BTreeMap<String, Vec<u8>>> {
        let mut blobs = BTreeMap::new();
        for (content_hash, blob_id) in unique_file_remote_blob_ids(manifest)? {
            let bytes = self
                .remote
                .download_blob(workspace_id, &blob_id)
                .await
                .map_err(boxed_error)?;
            verify_blob_id(&bytes, &blob_id)?;
            blobs.insert(content_hash, self.decrypt_remote_object_bytes(&bytes)?);
        }
        Ok(blobs)
    }

    fn local_blob_bytes(
        &self,
        manifest: &Manifest,
    ) -> SyncWorkflowResult<BTreeMap<String, Vec<u8>>> {
        let mut blobs = BTreeMap::new();
        for blob in self.engine.staged_blobs_for_manifest(manifest)? {
            blobs.insert(blob.content_hash, blob.bytes);
        }
        Ok(blobs)
    }

    #[allow(clippy::too_many_arguments)]
    fn add_conflict(
        &self,
        combined: &mut Manifest,
        state: &mut rustsync_core::reconciliation::SyncState,
        report: &mut SyncReport,
        path: &str,
        local: &Manifest,
        remote: &Manifest,
        revision: u64,
        reserved_paths: &mut BTreeSet<String>,
    ) -> SyncWorkflowResult<()> {
        let subtree_prefix = format!("{path}/");
        for (entry_path, entry) in &local.entries {
            if entry_path == path || entry_path.starts_with(&subtree_prefix) {
                combined.insert(entry_path.clone(), entry.clone())?;
            }
        }
        let prefix = conflict_copy_path(path, revision);
        let mut remote_copy_path = prefix.clone();
        let mut suffix = 1;
        while !reserved_paths.insert(remote_copy_path.clone()) {
            remote_copy_path = format!("{prefix}-{suffix}");
            suffix += 1;
        }
        for (entry_path, entry) in &remote.entries {
            if entry_path == path || entry_path.starts_with(&subtree_prefix) {
                let copy_path = format!("{remote_copy_path}{}", &entry_path[path.len()..]);
                reserved_paths.insert(copy_path.clone());
                combined.insert(copy_path, entry.clone())?;
            }
        }
        let record = ConflictRecord {
            local_copy_path: path.to_string(),
            remote_copy_path: remote_copy_path.clone(),
            remote_deleted: !remote.contains_path(path),
        };
        state.conflicts.insert(path.to_string(), record);
        report.conflicts.push(path.to_string());
        Ok(())
    }

    fn ensure_working_tree_clean(&self) -> SyncWorkflowResult<()> {
        let status = self.engine.working_tree_status()?;
        if status.diff.changes.is_empty() {
            Ok(())
        } else {
            Err(Box::new(UnstagedChangesError))
        }
    }

    fn decrypt_remote_object_bytes(&self, bytes: &[u8]) -> SyncWorkflowResult<Vec<u8>> {
        let encrypted = EncryptedObject::from_remote_bytes(bytes)?;
        Ok(self.engine.workspace().crypto().decrypt_file(&encrypted)?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushReport {
    pub workspace_id: WorkspaceId,
    pub manifest_id: ManifestId,
    pub uploaded_blobs: usize,
    pub reused_blobs: usize,
    pub uploaded_manifests: usize,
    pub reused_manifests: usize,
    pub changed_files: Vec<String>,
    pub previous_head_revision: u64,
    pub updated_head_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub workspace_id: WorkspaceId,
    pub observed_remote_revision: u64,
    pub plan: rustsync_core::reconciliation::ReconciliationPlan,
    pub mode: SyncMode,
    pub local_content_changed: bool,
    pub published: bool,
    pub retry_count: u8,
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullReport {
    pub workspace_id: WorkspaceId,
    pub remote_head_revision: u64,
    pub manifest_id: Option<ManifestId>,
    pub downloaded_blobs: usize,
    pub changed_files: Vec<String>,
    pub removed_paths: usize,
    pub prepared_directories: usize,
    pub written_files: usize,
    pub cached_blobs: usize,
}

impl PullReport {
    fn from_apply(
        workspace_id: WorkspaceId,
        remote_head_revision: u64,
        manifest_id: Option<ManifestId>,
        downloaded_blobs: usize,
        changed_files: Vec<String>,
        apply: ApplyReport,
    ) -> Self {
        Self {
            workspace_id,
            remote_head_revision,
            manifest_id,
            downloaded_blobs,
            changed_files,
            removed_paths: apply.removed_paths,
            prepared_directories: apply.prepared_directories,
            written_files: apply.written_files,
            cached_blobs: apply.cached_blobs,
        }
    }
}

fn count_upload_response(response: ObjectUploadResponse, uploaded: &mut usize, reused: &mut usize) {
    match response.status {
        ObjectUploadStatus::Created => *uploaded += 1,
        ObjectUploadStatus::AlreadyExists => *reused += 1,
    }
}

fn ensure_unchanged(expected: &Manifest, current: &Manifest) -> SyncWorkflowResult<()> {
    if !diff_manifests(expected, current).is_empty() {
        return Err(std::io::Error::other(
            "workspace changed during synchronization; local edits were preserved, rerun `rustsync sync`",
        ).into());
    }
    Ok(())
}

fn conflict_copy_path(path: &str, revision: u64) -> String {
    format!("{path}.rustsync-conflict-remote-r{revision}")
}

fn changed_files(manifest: &Manifest) -> Vec<String> {
    manifest
        .entries
        .iter()
        .filter_map(|(path, entry)| match entry {
            ManifestEntry::File(_) => Some(path.clone()),
            ManifestEntry::Directory(_) => None,
        })
        .collect()
}

fn unique_file_remote_blob_ids(manifest: &Manifest) -> SyncWorkflowResult<Vec<(String, BlobId)>> {
    let mut blobs = BTreeMap::new();
    for entry in manifest.entries.values() {
        let ManifestEntry::File(file) = entry else {
            continue;
        };
        if blobs.contains_key(&file.content_hash) {
            continue;
        }
        let remote_blob_id = file.remote_blob_id.clone().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "remote manifest file entry for content hash {} is missing remote_blob_id",
                    file.content_hash
                ),
            )
        })?;
        blobs.insert(file.content_hash.clone(), remote_blob_id);
    }

    Ok(blobs.into_iter().collect())
}

fn verify_manifest_id(bytes: &[u8], manifest_id: &ManifestId) -> SyncWorkflowResult<()> {
    let actual = ManifestId::from_content(bytes);
    if &actual == manifest_id {
        Ok(())
    } else {
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "downloaded manifest content id mismatch: expected {manifest_id}, got {actual}"
            ),
        )))
    }
}

fn verify_blob_id(bytes: &[u8], blob_id: &BlobId) -> SyncWorkflowResult<()> {
    let actual = BlobId::from_content(bytes);
    if &actual == blob_id {
        Ok(())
    } else {
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("downloaded blob content id mismatch: expected {blob_id}, got {actual}"),
        )))
    }
}

fn is_head_revision_conflict(error: &(dyn Error + 'static)) -> bool {
    error
        .downcast_ref::<rustsync_client::ClientError>()
        .is_some_and(|error| {
            matches!(
                error,
                rustsync_client::ClientError::Server(response)
                    if response.error == ApiErrorCode::HeadRevisionConflict
            )
        })
        || error.to_string().contains("head revision conflict")
}

fn boxed_error<E>(error: E) -> Box<dyn Error + Send + Sync>
where
    E: Error + Send + Sync + 'static,
{
    Box::new(error)
}
