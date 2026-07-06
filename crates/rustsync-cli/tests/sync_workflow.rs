use std::{collections::HashMap, fs, sync::Mutex};

use rustsync_cli::sync_workflow::{PullMode, PullReport, PushReport, SyncRemote, SyncWorkflow};
use rustsync_core::{
    device::DeviceIdentity,
    manifest::manifest_to_json_bytes,
    workspace::{LocalWorkspaceEngine, Workspace},
};
use rustsync_protocol::{
    BlobId, Manifest, ManifestEntry, ManifestId, ObjectUploadResponse, UnixTimestamp,
    WorkspaceHead, WorkspaceId,
};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn init_workspace(root: &std::path::Path) -> Workspace {
    let owner = DeviceIdentity::generate("test laptop").expect("generate device identity");
    Workspace::init_with_device_identity(root, &owner).expect("initialize workspace")
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn file_entry(bytes: &[u8]) -> ManifestEntry {
    ManifestEntry::file(bytes.len() as u64, hash(bytes), UnixTimestamp::from_secs(0))
}

#[derive(Default)]
struct FakeRemote {
    state: Mutex<FakeRemoteState>,
}

#[derive(Default)]
struct FakeRemoteState {
    head: Option<WorkspaceHead>,
    blob_upload_responses: Vec<ObjectUploadResponse>,
    manifest_upload_response: Option<ObjectUploadResponse>,
    uploaded_blobs: Vec<BlobId>,
    uploaded_manifests: Vec<ManifestId>,
    downloaded_blobs: Vec<BlobId>,
    manifest_bytes: HashMap<ManifestId, Vec<u8>>,
    blob_bytes: HashMap<BlobId, Vec<u8>>,
}

impl FakeRemote {
    fn with_head(
        workspace_id: WorkspaceId,
        revision: u64,
        manifest_id: Option<ManifestId>,
    ) -> Self {
        let remote = Self::default();
        remote.state.lock().expect("lock").head = Some(WorkspaceHead {
            workspace_id,
            revision,
            manifest_id,
            updated_by: None,
            updated_at: None,
        });
        remote
    }
}

impl SyncRemote for FakeRemote {
    type Error = std::io::Error;

    async fn upload_blob(
        &self,
        _workspace_id: &WorkspaceId,
        blob_id: &BlobId,
        _bytes: Vec<u8>,
    ) -> Result<ObjectUploadResponse, Self::Error> {
        let mut state = self.state.lock().expect("lock");
        state.uploaded_blobs.push(blob_id.clone());
        Ok(state
            .blob_upload_responses
            .pop()
            .unwrap_or_else(ObjectUploadResponse::created))
    }

    async fn upload_manifest(
        &self,
        _workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
        _bytes: Vec<u8>,
    ) -> Result<ObjectUploadResponse, Self::Error> {
        let mut state = self.state.lock().expect("lock");
        state.uploaded_manifests.push(manifest_id.clone());
        Ok(state
            .manifest_upload_response
            .take()
            .unwrap_or_else(ObjectUploadResponse::created))
    }

    async fn download_blob(
        &self,
        _workspace_id: &WorkspaceId,
        blob_id: &BlobId,
    ) -> Result<Vec<u8>, Self::Error> {
        let mut state = self.state.lock().expect("lock");
        state.downloaded_blobs.push(blob_id.clone());
        state
            .blob_bytes
            .get(blob_id)
            .cloned()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing blob"))
    }

    async fn download_manifest(
        &self,
        _workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
    ) -> Result<Vec<u8>, Self::Error> {
        let state = self.state.lock().expect("lock");
        state
            .manifest_bytes
            .get(manifest_id)
            .cloned()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing manifest"))
    }

    async fn fetch_workspace_head(
        &self,
        _workspace_id: &WorkspaceId,
    ) -> Result<WorkspaceHead, Self::Error> {
        Ok(self.state.lock().expect("lock").head.clone().expect("head"))
    }

    async fn update_workspace_head(
        &self,
        workspace_id: &WorkspaceId,
        expected_revision: u64,
        manifest_id: &ManifestId,
    ) -> Result<WorkspaceHead, Self::Error> {
        let mut state = self.state.lock().expect("lock");
        let current = state.head.clone().expect("head");
        assert_eq!(current.revision, expected_revision);
        let updated = WorkspaceHead {
            workspace_id: workspace_id.clone(),
            revision: expected_revision + 1,
            manifest_id: Some(manifest_id.clone()),
            updated_by: None,
            updated_at: None,
        };
        state.head = Some(updated.clone());
        Ok(updated)
    }
}

#[tokio::test]
async fn push_uploads_staged_objects_and_returns_a_typed_report() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    fs::write(temp.path().join("document.txt"), b"hello").expect("write document");
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    engine.stage_all().expect("stage all");

    let remote = FakeRemote::with_head(workspace.workspace_id().clone(), 7, None);
    let report: PushReport = SyncWorkflow::new(engine, remote)
        .push()
        .await
        .expect("push");

    assert_eq!(report.workspace_id, *workspace.workspace_id());
    assert_eq!(report.uploaded_blobs, 1);
    assert_eq!(report.reused_blobs, 0);
    assert_eq!(report.uploaded_manifests, 1);
    assert_eq!(report.reused_manifests, 0);
    assert_eq!(report.previous_head_revision, 7);
    assert_eq!(report.updated_head_revision, 8);
    assert_eq!(report.changed_files, vec!["document.txt".to_string()]);
}

#[tokio::test]
async fn pull_refuses_to_overwrite_unstaged_local_changes_by_default() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    fs::write(temp.path().join("document.txt"), b"local baseline").expect("write baseline");
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    engine.stage_all().expect("stage baseline");
    fs::write(temp.path().join("document.txt"), b"unstaged local change").expect("write change");

    let bytes = b"remote contents";
    let content_hash = hash(bytes);
    let blob_id = BlobId::parse(format!("blob_{content_hash}")).expect("blob id");
    let mut manifest = Manifest::new(workspace.workspace_id().clone());
    manifest
        .insert("document.txt".to_string(), file_entry(bytes))
        .expect("insert document");
    let manifest_bytes = manifest_to_json_bytes(&manifest).expect("manifest bytes");
    let manifest_id = ManifestId::from_content(&manifest_bytes);

    let remote = FakeRemote::with_head(
        workspace.workspace_id().clone(),
        12,
        Some(manifest_id.clone()),
    );
    {
        let mut state = remote.state.lock().expect("lock");
        state.manifest_bytes.insert(manifest_id, manifest_bytes);
        state.blob_bytes.insert(blob_id, bytes.to_vec());
    }

    let error = SyncWorkflow::new(engine, remote)
        .pull()
        .await
        .expect_err("safe pull should refuse unstaged local changes");
    let message = error.to_string();

    assert!(
        message.contains("unstaged changes"),
        "error should mention unstaged changes: {message}"
    );
    assert!(
        message.contains("rustsync add -A"),
        "error should suggest staging changes: {message}"
    );
    assert!(
        message.contains("rustsync pull --force"),
        "error should mention force override: {message}"
    );
    assert_eq!(
        fs::read(temp.path().join("document.txt")).expect("document"),
        b"unstaged local change"
    );
}

#[tokio::test]
async fn force_pull_overwrites_unstaged_local_changes() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    fs::write(temp.path().join("document.txt"), b"local baseline").expect("write baseline");
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    engine.stage_all().expect("stage baseline");
    fs::write(temp.path().join("document.txt"), b"unstaged local change").expect("write change");

    let bytes = b"remote contents";
    let content_hash = hash(bytes);
    let blob_id = BlobId::parse(format!("blob_{content_hash}")).expect("blob id");
    let mut manifest = Manifest::new(workspace.workspace_id().clone());
    manifest
        .insert("document.txt".to_string(), file_entry(bytes))
        .expect("insert document");
    let manifest_bytes = manifest_to_json_bytes(&manifest).expect("manifest bytes");
    let manifest_id = ManifestId::from_content(&manifest_bytes);

    let remote = FakeRemote::with_head(
        workspace.workspace_id().clone(),
        13,
        Some(manifest_id.clone()),
    );
    {
        let mut state = remote.state.lock().expect("lock");
        state
            .manifest_bytes
            .insert(manifest_id.clone(), manifest_bytes);
        state.blob_bytes.insert(blob_id, bytes.to_vec());
    }

    let report = SyncWorkflow::new(engine, remote)
        .pull_with_mode(PullMode::Force)
        .await
        .expect("force pull");

    assert_eq!(report.manifest_id, Some(manifest_id));
    assert_eq!(report.written_files, 1);
    assert_eq!(
        fs::read(temp.path().join("document.txt")).expect("document"),
        bytes
    );
}

#[tokio::test]
async fn pull_downloads_unique_remote_blobs_applies_manifest_and_returns_a_typed_report() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    fs::write(temp.path().join("stale.txt"), b"stale").expect("write stale");
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    engine.stage_all().expect("stage stale baseline");

    let bytes = b"same remote contents";
    let content_hash = hash(bytes);
    let blob_id = BlobId::parse(format!("blob_{content_hash}")).expect("blob id");
    let mut manifest = Manifest::new(workspace.workspace_id().clone());
    manifest
        .insert("a.txt".to_string(), file_entry(bytes))
        .expect("insert a");
    manifest
        .insert("nested/b.txt".to_string(), file_entry(bytes))
        .expect("insert b");
    let manifest_bytes = manifest_to_json_bytes(&manifest).expect("manifest bytes");
    let manifest_id = ManifestId::from_content(&manifest_bytes);

    let remote = FakeRemote::with_head(
        workspace.workspace_id().clone(),
        11,
        Some(manifest_id.clone()),
    );
    {
        let mut state = remote.state.lock().expect("lock");
        state
            .manifest_bytes
            .insert(manifest_id.clone(), manifest_bytes);
        state.blob_bytes.insert(blob_id.clone(), bytes.to_vec());
    }

    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let report: PullReport = SyncWorkflow::new(engine, remote)
        .pull()
        .await
        .expect("pull");

    assert_eq!(report.workspace_id, *workspace.workspace_id());
    assert_eq!(report.remote_head_revision, 11);
    assert_eq!(report.manifest_id, Some(manifest_id));
    assert_eq!(
        report.downloaded_blobs, 1,
        "duplicate content should be downloaded once"
    );
    assert_eq!(report.written_files, 2);
    assert_eq!(report.removed_paths, 1);
    assert_eq!(
        report.changed_files,
        vec!["a.txt".to_string(), "nested/b.txt".to_string()]
    );
    assert_eq!(fs::read(temp.path().join("a.txt")).expect("a"), bytes);
    assert_eq!(
        fs::read(temp.path().join("nested/b.txt")).expect("b"),
        bytes
    );
    assert!(!temp.path().join("stale.txt").exists());
}
