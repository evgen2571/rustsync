use std::{collections::BTreeMap, error::Error};

use rustsync_client::{RequestSigner, RustSyncClient};
use rustsync_core::{
    manifest::{manifest_from_json_bytes, manifest_to_json_bytes},
    workspace::{ApplyReport, LocalWorkspaceEngine},
};
use rustsync_protocol::{
    BlobId, Manifest, ManifestEntry, ManifestId, ObjectUploadResponse, ObjectUploadStatus,
    WorkspaceHead, WorkspaceId,
};

pub type SyncWorkflowResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

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
        let workspace_id = self.engine.workspace_id().clone();
        let manifest = self.engine.load_staged_manifest()?;
        let changed_files = changed_files(&manifest);

        let mut uploaded_blobs = 0usize;
        let mut reused_blobs = 0usize;
        for blob in self.engine.staged_blobs_for_manifest(&manifest)? {
            let blob_id = blob_id_for_content_hash(&blob.content_hash)?;
            let response = self
                .remote
                .upload_blob(&workspace_id, &blob_id, blob.bytes)
                .await
                .map_err(boxed_error)?;
            count_upload_response(response, &mut uploaded_blobs, &mut reused_blobs);
        }

        let manifest_bytes = manifest_to_json_bytes(&manifest)?;
        let manifest_id = ManifestId::from_content(&manifest_bytes);
        let manifest_response = self
            .remote
            .upload_manifest(&workspace_id, &manifest_id, manifest_bytes)
            .await
            .map_err(boxed_error)?;
        let mut uploaded_manifests = 0usize;
        let mut reused_manifests = 0usize;
        count_upload_response(
            manifest_response,
            &mut uploaded_manifests,
            &mut reused_manifests,
        );

        let previous_head = self
            .remote
            .fetch_workspace_head(&workspace_id)
            .await
            .map_err(boxed_error)?;
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

    pub async fn pull(&self) -> SyncWorkflowResult<PullReport> {
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
        let manifest = manifest_from_json_bytes(&manifest_bytes)?;
        self.engine.validate_pulled_manifest(&manifest)?;

        let mut blobs = BTreeMap::new();
        for content_hash in unique_file_content_hashes(&manifest) {
            let blob_id = blob_id_for_content_hash(&content_hash)?;
            let bytes = self
                .remote
                .download_blob(&workspace_id, &blob_id)
                .await
                .map_err(boxed_error)?;
            blobs.insert(content_hash, bytes);
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

fn unique_file_content_hashes(manifest: &Manifest) -> Vec<String> {
    manifest
        .entries
        .values()
        .filter_map(|entry| match entry {
            ManifestEntry::File(file) => Some(file.content_hash.clone()),
            ManifestEntry::Directory(_) => None,
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn blob_id_for_content_hash(content_hash: &str) -> rustsync_protocol::ProtocolResult<BlobId> {
    BlobId::parse(format!("blob_{content_hash}"))
}

fn boxed_error<E>(error: E) -> Box<dyn Error + Send + Sync>
where
    E: Error + Send + Sync + 'static,
{
    Box::new(error)
}
