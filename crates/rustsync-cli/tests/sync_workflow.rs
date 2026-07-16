use std::{collections::HashMap, fs, sync::Arc, sync::Mutex};

use rustsync_cli::sync_workflow::{PullReport, PushReport, SyncMode, SyncRemote, SyncWorkflow};
use rustsync_core::{
    device::DeviceIdentity,
    manifest::{manifest_from_json_bytes, manifest_to_json_bytes},
    workspace::{LocalWorkspaceEngine, Workspace},
};
use rustsync_protocol::{
    BlobId, EncryptedObject, Manifest, ManifestEntry, ManifestId, ObjectUploadResponse,
    UnixTimestamp, WorkspaceHead, WorkspaceId,
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

fn encrypted_object_bytes(workspace: &Workspace, plaintext: &[u8]) -> Vec<u8> {
    let encrypted = workspace
        .crypto()
        .encrypt_bytes(plaintext)
        .expect("encrypt object");
    encrypted
        .to_binary_bytes()
        .expect("serialize encrypted object")
}

fn decrypt_object_bytes(workspace: &Workspace, bytes: &[u8]) -> Vec<u8> {
    let encrypted = EncryptedObject::from_remote_bytes(bytes).expect("encrypted object");
    workspace
        .crypto()
        .decrypt_file(&encrypted)
        .expect("decrypt object")
}

fn assert_malformed_binary_object(bytes: &[u8]) {
    assert!(bytes.starts_with(b"RSOB"), "fixture must be an RSOB object");
    assert!(
        EncryptedObject::from_remote_bytes(bytes).is_err(),
        "fixture must fail binary object decoding"
    );
}

fn assert_does_not_contain(haystack: &[u8], needle: &[u8], label: &str) {
    assert!(
        !haystack
            .windows(needle.len())
            .any(|window| window == needle),
        "{label} must not contain plaintext marker `{}`",
        String::from_utf8_lossy(needle)
    );
}

#[derive(Clone, Default)]
struct FakeRemote {
    state: Arc<Mutex<FakeRemoteState>>,
}

#[derive(Default)]
struct FakeRemoteState {
    head: Option<WorkspaceHead>,
    blob_upload_responses: Vec<ObjectUploadResponse>,
    manifest_upload_response: Option<ObjectUploadResponse>,
    uploaded_blobs: Vec<BlobId>,
    uploaded_manifests: Vec<ManifestId>,
    downloaded_blobs: Vec<BlobId>,
    head_update_calls: Vec<u64>,
    stale_head_failures_remaining: usize,
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
        bytes: Vec<u8>,
    ) -> Result<ObjectUploadResponse, Self::Error> {
        let mut state = self.state.lock().expect("lock");
        state.uploaded_blobs.push(blob_id.clone());
        state.blob_bytes.insert(blob_id.clone(), bytes);
        Ok(state
            .blob_upload_responses
            .pop()
            .unwrap_or_else(ObjectUploadResponse::created))
    }

    async fn upload_manifest(
        &self,
        _workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
        bytes: Vec<u8>,
    ) -> Result<ObjectUploadResponse, Self::Error> {
        let mut state = self.state.lock().expect("lock");
        state.uploaded_manifests.push(manifest_id.clone());
        state.manifest_bytes.insert(manifest_id.clone(), bytes);
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
        state.head_update_calls.push(expected_revision);
        if state.stale_head_failures_remaining > 0 {
            state.stale_head_failures_remaining -= 1;
            state.head = Some(WorkspaceHead {
                revision: current.revision + 1,
                ..current
            });
            return Err(std::io::Error::other("head revision conflict"));
        }
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
async fn push_uploads_encrypted_blob_objects_without_plaintext_file_bytes() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    let plaintext = b"super secret plaintext blob marker";
    fs::write(temp.path().join("document.txt"), plaintext).expect("write document");
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    engine.stage_all().expect("stage all");

    let remote = FakeRemote::with_head(workspace.workspace_id().clone(), 1, None);
    let remote_state = remote.state.clone();
    SyncWorkflow::new(engine, remote)
        .push()
        .await
        .expect("push");

    let state = remote_state.lock().expect("lock");
    assert_eq!(state.blob_bytes.len(), 1);
    let (blob_id, uploaded_bytes) = state.blob_bytes.iter().next().expect("uploaded blob");
    assert!(
        uploaded_bytes.starts_with(b"RSOB"),
        "uploaded blob must use RSOB"
    );
    assert_eq!(*blob_id, BlobId::from_content(uploaded_bytes));
    assert_does_not_contain(uploaded_bytes, b"ciphertext", "remote blob body");
    assert_does_not_contain(uploaded_bytes, b"key_id", "remote blob body");
    assert_does_not_contain(uploaded_bytes, plaintext, "remote blob body");
    assert_eq!(decrypt_object_bytes(&workspace, uploaded_bytes), plaintext);
}

#[tokio::test]
async fn push_uploads_encrypted_manifest_without_plaintext_paths_hashes_or_file_bytes() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    let path = temp.path().join("private").join("secret-name.txt");
    fs::create_dir_all(path.parent().expect("parent")).expect("create private dir");
    let plaintext = b"super secret manifest plaintext marker";
    fs::write(&path, plaintext).expect("write document");
    let content_hash = hash(plaintext);
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    engine.stage_all().expect("stage all");

    let remote = FakeRemote::with_head(workspace.workspace_id().clone(), 2, None);
    let remote_state = remote.state.clone();
    SyncWorkflow::new(engine, remote)
        .push()
        .await
        .expect("push");

    let state = remote_state.lock().expect("lock");
    assert_eq!(state.manifest_bytes.len(), 1);
    let (manifest_id, uploaded_bytes) = state
        .manifest_bytes
        .iter()
        .next()
        .expect("uploaded manifest");
    assert!(
        uploaded_bytes.starts_with(b"RSOB"),
        "uploaded manifest must use RSOB"
    );
    assert_eq!(*manifest_id, ManifestId::from_content(uploaded_bytes));
    assert_does_not_contain(uploaded_bytes, b"ciphertext", "remote manifest body");
    assert_does_not_contain(uploaded_bytes, b"key_id", "remote manifest body");
    assert_does_not_contain(uploaded_bytes, b"private", "remote manifest body");
    assert_does_not_contain(uploaded_bytes, b"secret-name", "remote manifest body");
    assert_does_not_contain(
        uploaded_bytes,
        content_hash.as_bytes(),
        "remote manifest body",
    );
    assert_does_not_contain(uploaded_bytes, plaintext, "remote manifest body");

    let decrypted_manifest_bytes = decrypt_object_bytes(&workspace, uploaded_bytes);
    let decrypted_manifest =
        manifest_from_json_bytes(&decrypted_manifest_bytes).expect("decrypted remote manifest");
    let entry = decrypted_manifest
        .get("private/secret-name.txt")
        .expect("manifest entry");
    let ManifestEntry::File(file) = entry else {
        panic!("expected file entry");
    };
    assert_eq!(file.content_hash, content_hash);
    assert!(file.remote_blob_id.is_some());
}

#[tokio::test]
async fn pull_decrypts_remote_manifest_and_encrypted_blobs() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    let engine = LocalWorkspaceEngine::new(workspace.clone());

    let bytes = b"restored super secret plaintext";
    let encrypted_blob_bytes = encrypted_object_bytes(&workspace, bytes);
    let remote_blob_id = BlobId::from_content(&encrypted_blob_bytes);
    let mut manifest = Manifest::new(workspace.workspace_id().clone());
    let mut entry = file_entry(bytes);
    let ManifestEntry::File(file) = &mut entry else {
        panic!("expected file entry");
    };
    file.remote_blob_id = Some(remote_blob_id.clone());
    manifest
        .insert("private/restored.txt".to_string(), entry)
        .expect("insert document");
    let manifest_plaintext = manifest_to_json_bytes(&manifest).expect("manifest bytes");
    let encrypted_manifest_bytes = encrypted_object_bytes(&workspace, &manifest_plaintext);
    let manifest_id = ManifestId::from_content(&encrypted_manifest_bytes);

    let remote = FakeRemote::with_head(
        workspace.workspace_id().clone(),
        3,
        Some(manifest_id.clone()),
    );
    {
        let mut state = remote.state.lock().expect("lock");
        state
            .manifest_bytes
            .insert(manifest_id.clone(), encrypted_manifest_bytes);
        state
            .blob_bytes
            .insert(remote_blob_id.clone(), encrypted_blob_bytes);
    }

    let report = SyncWorkflow::new(engine, remote.clone())
        .pull()
        .await
        .expect("pull");

    assert_eq!(report.manifest_id, Some(manifest_id));
    assert_eq!(report.downloaded_blobs, 1);
    assert_eq!(report.written_files, 1);
    assert_eq!(
        fs::read(temp.path().join("private/restored.txt")).expect("restored file"),
        bytes
    );
    assert_eq!(
        remote.state.lock().expect("lock").downloaded_blobs,
        vec![remote_blob_id]
    );
}

#[tokio::test]
async fn pull_rejects_matching_id_malformed_binary_manifest_before_workspace_write() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    let tracked_path = temp.path().join("tracked.txt");
    fs::write(&tracked_path, b"tracked local contents").expect("write tracked file");
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let staged_manifest = engine.stage_all().expect("stage tracked file").manifest;
    let tracked_before = fs::read(&tracked_path).expect("read tracked file before pull");
    let staged_manifest_before =
        fs::read(&workspace.layout.manifest_path).expect("read staged manifest before pull");
    let malformed_binary = b"RSOB\x01malformed".to_vec();
    assert_malformed_binary_object(&malformed_binary);
    let manifest_id = ManifestId::from_content(&malformed_binary);
    let remote = FakeRemote::with_head(
        workspace.workspace_id().clone(),
        7,
        Some(manifest_id.clone()),
    );
    remote
        .state
        .lock()
        .expect("lock")
        .manifest_bytes
        .insert(manifest_id, malformed_binary);

    SyncWorkflow::new(engine.clone(), remote)
        .pull()
        .await
        .expect_err("malformed RSOB manifest should fail");

    assert_eq!(
        fs::read(&tracked_path).expect("read tracked file after pull"),
        tracked_before,
        "a malformed manifest must not overwrite or remove existing workspace files"
    );
    assert_eq!(
        fs::read(&workspace.layout.manifest_path).expect("read staged manifest after pull"),
        staged_manifest_before,
        "a malformed manifest must not update workspace metadata"
    );
    assert_eq!(
        engine.load_staged_manifest().expect("load staged manifest"),
        staged_manifest,
        "a malformed manifest must leave the tracked workspace state unchanged"
    );
}

#[tokio::test]
async fn pull_rejects_matching_id_json_manifest_before_workspace_write() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    let tracked_path = temp.path().join("tracked.txt");
    fs::write(&tracked_path, b"tracked local contents").expect("write tracked file");
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let staged_manifest = engine.stage_all().expect("stage tracked file").manifest;
    let tracked_before = fs::read(&tracked_path).expect("read tracked file before pull");
    let staged_manifest_before =
        fs::read(&workspace.layout.manifest_path).expect("read staged manifest before pull");
    let json_payload = br#"{"key_id":"main","nonce":"not-rsob"}"#.to_vec();
    assert!(!json_payload.starts_with(b"RSOB"));
    assert!(matches!(
        EncryptedObject::from_remote_bytes(&json_payload),
        Err(rustsync_protocol::ProtocolError::InvalidEncryptedObjectEncoding)
    ));
    let manifest_id = ManifestId::from_content(&json_payload);
    let remote = FakeRemote::with_head(
        workspace.workspace_id().clone(),
        8,
        Some(manifest_id.clone()),
    );
    remote
        .state
        .lock()
        .expect("lock")
        .manifest_bytes
        .insert(manifest_id, json_payload);

    SyncWorkflow::new(engine.clone(), remote)
        .pull()
        .await
        .expect_err("JSON-looking non-RSOB manifest should fail");

    assert_eq!(
        fs::read(&tracked_path).expect("read tracked file after pull"),
        tracked_before,
        "a non-RSOB manifest must not overwrite or remove existing workspace files"
    );
    assert_eq!(
        fs::read(&workspace.layout.manifest_path).expect("read staged manifest after pull"),
        staged_manifest_before,
        "a non-RSOB manifest must not update workspace metadata"
    );
    assert_eq!(
        engine.load_staged_manifest().expect("load staged manifest"),
        staged_manifest,
        "a non-RSOB manifest must leave the tracked workspace state unchanged"
    );
}

#[tokio::test]
async fn pull_rejects_malformed_binary_manifest_with_mismatched_content_id_before_decode() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    let engine = LocalWorkspaceEngine::new(workspace.clone());

    let manifest_id = ManifestId::from_content(b"RSOB\x01expected-manifest");
    let malformed_binary = b"RSOB\x01malformed".to_vec();
    assert_malformed_binary_object(&malformed_binary);
    assert_ne!(ManifestId::from_content(&malformed_binary), manifest_id);

    let remote = FakeRemote::with_head(
        workspace.workspace_id().clone(),
        4,
        Some(manifest_id.clone()),
    );
    remote
        .state
        .lock()
        .expect("lock")
        .manifest_bytes
        .insert(manifest_id, malformed_binary);

    let error = SyncWorkflow::new(engine, remote)
        .pull()
        .await
        .expect_err("pull should reject manifest content id mismatch");
    let io_error = error
        .downcast_ref::<std::io::Error>()
        .expect("invalid data io error");

    assert_eq!(io_error.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        io_error
            .to_string()
            .contains("downloaded manifest content id mismatch"),
        "error should mention manifest content id mismatch: {io_error}"
    );
    assert!(
        !io_error.to_string().contains("binary"),
        "content ID validation must happen before binary decoding: {io_error}"
    );
    assert!(!temp.path().join("unexpected.txt").exists());
}

#[tokio::test]
async fn pull_rejects_malformed_binary_blob_with_mismatched_content_id_before_decode() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    let engine = LocalWorkspaceEngine::new(workspace.clone());

    let expected_bytes = b"expected restored plaintext";
    let expected_encrypted_blob_bytes = encrypted_object_bytes(&workspace, expected_bytes);
    let remote_blob_id = BlobId::from_content(&expected_encrypted_blob_bytes);

    let mut manifest = Manifest::new(workspace.workspace_id().clone());
    let mut entry = file_entry(expected_bytes);
    let ManifestEntry::File(file) = &mut entry else {
        panic!("expected file entry");
    };
    file.remote_blob_id = Some(remote_blob_id.clone());
    manifest
        .insert("restored.txt".to_string(), entry)
        .expect("insert document");
    let manifest_plaintext = manifest_to_json_bytes(&manifest).expect("manifest bytes");
    let encrypted_manifest_bytes = encrypted_object_bytes(&workspace, &manifest_plaintext);
    let manifest_id = ManifestId::from_content(&encrypted_manifest_bytes);

    let malformed_binary = b"RSOB\x01malformed".to_vec();
    assert_malformed_binary_object(&malformed_binary);
    assert_ne!(BlobId::from_content(&malformed_binary), remote_blob_id);

    let remote = FakeRemote::with_head(
        workspace.workspace_id().clone(),
        5,
        Some(manifest_id.clone()),
    );
    {
        let mut state = remote.state.lock().expect("lock");
        state
            .manifest_bytes
            .insert(manifest_id, encrypted_manifest_bytes);
        state
            .blob_bytes
            .insert(remote_blob_id.clone(), malformed_binary);
    }

    let error = SyncWorkflow::new(engine, remote)
        .pull()
        .await
        .expect_err("pull should reject blob content id mismatch");
    let io_error = error
        .downcast_ref::<std::io::Error>()
        .expect("invalid data io error");

    assert_eq!(io_error.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        io_error
            .to_string()
            .contains("downloaded blob content id mismatch"),
        "error should mention blob content id mismatch: {io_error}"
    );
    assert!(
        !io_error.to_string().contains("binary"),
        "content ID validation must happen before binary decoding: {io_error}"
    );
    assert!(!temp.path().join("restored.txt").exists());
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
        message.contains("local working tree has changes"),
        "error should mention local changes: {message}"
    );
    assert!(
        message.contains("back them up"),
        "error should suggest protecting local changes: {message}"
    );
    assert!(
        !message.contains("rustsync add -A"),
        "error should not suggest staging changes as protection: {message}"
    );
    assert!(
        message.contains("rustsync sync --discard-local --yes"),
        "error should mention destructive recovery: {message}"
    );
    assert_eq!(
        fs::read(temp.path().join("document.txt")).expect("document"),
        b"unstaged local change"
    );
}

#[tokio::test]
async fn discard_local_overwrites_local_changes_without_advancing_remote_head() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    fs::write(temp.path().join("document.txt"), b"local baseline").expect("write baseline");
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    engine.stage_all().expect("stage baseline");
    fs::write(temp.path().join("document.txt"), b"unstaged local change").expect("write change");

    let bytes = b"remote contents";
    let encrypted_blob_bytes = encrypted_object_bytes(&workspace, bytes);
    let remote_blob_id = BlobId::from_content(&encrypted_blob_bytes);
    let mut manifest = Manifest::new(workspace.workspace_id().clone());
    let mut entry = file_entry(bytes);
    let ManifestEntry::File(file) = &mut entry else {
        panic!("expected file entry");
    };
    file.remote_blob_id = Some(remote_blob_id.clone());
    manifest
        .insert("document.txt".to_string(), entry)
        .expect("insert document");
    let manifest_plaintext = manifest_to_json_bytes(&manifest).expect("manifest bytes");
    let encrypted_manifest_bytes = encrypted_object_bytes(&workspace, &manifest_plaintext);
    let manifest_id = ManifestId::from_content(&encrypted_manifest_bytes);

    let remote = FakeRemote::with_head(
        workspace.workspace_id().clone(),
        13,
        Some(manifest_id.clone()),
    );
    {
        let mut state = remote.state.lock().expect("lock");
        state
            .manifest_bytes
            .insert(manifest_id.clone(), encrypted_manifest_bytes);
        state
            .blob_bytes
            .insert(remote_blob_id, encrypted_blob_bytes);
    }

    let remote_state = remote.state.clone();
    let report = SyncWorkflow::new(engine.clone(), remote)
        .sync(SyncMode::DiscardLocal)
        .await
        .expect("discard local");

    assert_eq!(report.mode, SyncMode::DiscardLocal);
    assert_eq!(report.observed_remote_revision, 13);
    assert!(!report.published);
    assert_eq!(
        fs::read(temp.path().join("document.txt")).expect("document"),
        bytes
    );
    assert!(
        remote_state
            .lock()
            .expect("lock")
            .head_update_calls
            .is_empty()
    );
    assert_eq!(
        engine
            .load_sync_state()
            .expect("sync state")
            .last_synced_manifest,
        Some(manifest)
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
    let encrypted_blob_bytes = encrypted_object_bytes(&workspace, bytes);
    let remote_blob_id = BlobId::from_content(&encrypted_blob_bytes);
    let mut manifest = Manifest::new(workspace.workspace_id().clone());
    let mut entry_a = file_entry(bytes);
    let ManifestEntry::File(file_a) = &mut entry_a else {
        panic!("expected file entry");
    };
    file_a.remote_blob_id = Some(remote_blob_id.clone());
    manifest
        .insert("a.txt".to_string(), entry_a)
        .expect("insert a");
    let mut entry_b = file_entry(bytes);
    let ManifestEntry::File(file_b) = &mut entry_b else {
        panic!("expected file entry");
    };
    file_b.remote_blob_id = Some(remote_blob_id.clone());
    manifest
        .insert("nested/b.txt".to_string(), entry_b)
        .expect("insert b");
    let manifest_plaintext = manifest_to_json_bytes(&manifest).expect("manifest bytes");
    let encrypted_manifest_bytes = encrypted_object_bytes(&workspace, &manifest_plaintext);
    let manifest_id = ManifestId::from_content(&encrypted_manifest_bytes);

    let remote = FakeRemote::with_head(
        workspace.workspace_id().clone(),
        11,
        Some(manifest_id.clone()),
    );
    {
        let mut state = remote.state.lock().expect("lock");
        state
            .manifest_bytes
            .insert(manifest_id.clone(), encrypted_manifest_bytes);
        state
            .blob_bytes
            .insert(remote_blob_id.clone(), encrypted_blob_bytes);
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

#[tokio::test]
async fn reconcile_no_op_does_not_update_the_remote_head() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let remote = FakeRemote::with_head(workspace.workspace_id().clone(), 5, None);
    let remote_state = remote.state.clone();

    let report = SyncWorkflow::new(engine, remote)
        .sync(SyncMode::Reconcile)
        .await
        .expect("no-op sync");

    assert!(!report.plan.has_changes());
    assert!(!report.published);
    let state = remote_state.lock().expect("lock");
    assert!(state.head_update_calls.is_empty());
    assert!(state.uploaded_blobs.is_empty());
    assert!(state.uploaded_manifests.is_empty());
}

#[tokio::test]
async fn dry_run_does_not_stage_persist_or_mutate_remote() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    fs::write(temp.path().join("local.txt"), b"local changes").expect("write local file");
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let remote = FakeRemote::with_head(workspace.workspace_id().clone(), 5, None);
    let remote_state = remote.state.clone();

    let report = SyncWorkflow::new(engine, remote)
        .sync(SyncMode::DryRun)
        .await
        .expect("dry run");

    assert!(report.plan.has_changes());
    assert!(
        !workspace
            .layout
            .manifest_path
            .try_exists()
            .expect("manifest exists")
    );
    assert!(
        !workspace
            .layout
            .sync_state_path
            .try_exists()
            .expect("sync state exists")
    );
    assert_eq!(
        fs::read(temp.path().join("local.txt")).expect("local file"),
        b"local changes"
    );
    let state = remote_state.lock().expect("lock");
    assert!(state.head_update_calls.is_empty());
    assert!(state.uploaded_blobs.is_empty());
    assert!(state.uploaded_manifests.is_empty());
}
