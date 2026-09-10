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
    publication_failures_remaining: usize,
    edit_on_manifest_download: Option<(std::path::PathBuf, Vec<u8>)>,
    edit_on_blob_download: Option<(std::path::PathBuf, Vec<u8>)>,
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
        if let Some((path, bytes)) = state.edit_on_blob_download.take() {
            fs::write(path, bytes)?;
        }
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
        let mut state = self.state.lock().expect("lock");
        if let Some((path, bytes)) = state.edit_on_manifest_download.take() {
            fs::write(path, bytes)?;
        }
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
        if state.publication_failures_remaining > 0 {
            state.publication_failures_remaining -= 1;
            return Err(std::io::Error::other("connection interrupted"));
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
async fn repeated_sync_reuses_remote_blobs_for_unchanged_and_renamed_files() {
    let temp = tempdir().unwrap();
    let workspace = init_workspace(temp.path());
    let remote = FakeRemote::with_head(workspace.workspace_id().clone(), 0, None);
    fs::write(temp.path().join("keep"), b"unchanged").unwrap();
    fs::write(temp.path().join("edit"), b"before").unwrap();
    SyncWorkflow::new(LocalWorkspaceEngine::new(workspace.clone()), remote.clone())
        .sync(SyncMode::Reconcile)
        .await
        .unwrap();
    let first_id = {
        let state = remote.state.lock().unwrap();
        assert_eq!(state.uploaded_blobs.len(), 2);
        let manifest = manifest_from_json_bytes(&decrypt_object_bytes(
            &workspace,
            &state.manifest_bytes[state.head.as_ref().unwrap().manifest_id.as_ref().unwrap()],
        ))
        .unwrap();
        let ManifestEntry::File(file) = &manifest.entries["keep"] else {
            panic!("file");
        };
        file.remote_blob_id.clone().unwrap()
    };
    fs::rename(temp.path().join("keep"), temp.path().join("renamed")).unwrap();
    fs::write(temp.path().join("edit"), b"after").unwrap();
    SyncWorkflow::new(
        LocalWorkspaceEngine::open(temp.path()).unwrap(),
        remote.clone(),
    )
    .sync(SyncMode::Reconcile)
    .await
    .unwrap();
    let state = remote.state.lock().unwrap();
    assert_eq!(
        state.uploaded_blobs.len(),
        3,
        "only changed content should be uploaded"
    );
    let manifest = manifest_from_json_bytes(&decrypt_object_bytes(
        &workspace,
        &state.manifest_bytes[state.head.as_ref().unwrap().manifest_id.as_ref().unwrap()],
    ))
    .unwrap();
    let ManifestEntry::File(file) = &manifest.entries["renamed"] else {
        panic!("file");
    };
    assert_eq!(file.remote_blob_id.as_ref(), Some(&first_id));
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

fn publish_files(remote: &FakeRemote, workspace: &Workspace, files: &[(&str, &[u8])]) {
    let mut manifest = Manifest::new(workspace.config.workspace_id.clone());
    let mut state = remote.state.lock().unwrap();
    for (path, bytes) in files {
        for (index, _) in path.match_indices('/') {
            manifest
                .insert(path[..index].to_string(), ManifestEntry::directory())
                .unwrap();
        }
        let encrypted = encrypted_object_bytes(workspace, bytes);
        let id = BlobId::from_content(&encrypted);
        state.blob_bytes.insert(id.clone(), encrypted);
        let mut entry = file_entry(bytes);
        if let ManifestEntry::File(file) = &mut entry {
            file.remote_blob_id = Some(id);
        }
        manifest.insert((*path).to_string(), entry).unwrap();
    }
    let bytes = encrypted_object_bytes(workspace, &manifest_to_json_bytes(&manifest).unwrap());
    let id = ManifestId::from_content(&bytes);
    state.manifest_bytes.insert(id.clone(), bytes);
    let head = state.head.as_mut().unwrap();
    head.manifest_id = Some(id);
    head.revision += 1;
}

#[tokio::test]
async fn sync_downloads_only_changed_remote_content_and_preserves_unchanged_mtime() {
    for mode in [SyncMode::Reconcile, SyncMode::DiscardLocal] {
        let temp = tempdir().unwrap();
        let workspace = init_workspace(temp.path());
        let remote = FakeRemote::with_head(workspace.workspace_id().clone(), 0, None);
        let workflow =
            SyncWorkflow::new(LocalWorkspaceEngine::new(workspace.clone()), remote.clone());
        publish_files(
            &remote,
            &workspace,
            &[("keep", b"stable"), ("edit", b"before")],
        );
        workflow.sync(SyncMode::Reconcile).await.unwrap();
        let keep = temp.path().join("keep");
        let modified = fs::metadata(&keep).unwrap().modified().unwrap();
        publish_files(
            &remote,
            &workspace,
            &[("keep", b"stable"), ("edit", b"after")],
        );
        remote.state.lock().unwrap().downloaded_blobs.clear();
        workflow.sync(mode).await.unwrap();
        let state = remote.state.lock().unwrap();
        assert_eq!(state.downloaded_blobs.len(), 1, "mode {mode:?}");
        assert_eq!(
            decrypt_object_bytes(&workspace, &state.blob_bytes[&state.downloaded_blobs[0]]),
            b"after"
        );
        assert_eq!(fs::read(temp.path().join("edit")).unwrap(), b"after");
        assert_eq!(fs::metadata(keep).unwrap().modified().unwrap(), modified);
    }
}

#[tokio::test]
async fn local_only_sync_does_not_download_remote_blobs() {
    let temp = tempdir().unwrap();
    let workspace = init_workspace(temp.path());
    let remote = FakeRemote::with_head(workspace.workspace_id().clone(), 0, None);
    let workflow = SyncWorkflow::new(LocalWorkspaceEngine::new(workspace.clone()), remote.clone());
    publish_files(&remote, &workspace, &[("keep", b"stable")]);
    workflow.sync(SyncMode::Reconcile).await.unwrap();
    remote.state.lock().unwrap().downloaded_blobs.clear();
    fs::write(temp.path().join("new"), b"local addition").unwrap();
    workflow.sync(SyncMode::Reconcile).await.unwrap();
    assert!(remote.state.lock().unwrap().downloaded_blobs.is_empty());
}

#[tokio::test]
async fn reconcile_merges_edits_using_cached_base_content() {
    let temp = tempdir().unwrap();
    let workspace = init_workspace(temp.path());
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let remote = FakeRemote::with_head(workspace.config.workspace_id.clone(), 0, None);
    let workflow = SyncWorkflow::new(engine.clone(), remote.clone());
    publish_files(&remote, &workspace, &[("note", b"one\ntwo\nthree\n")]);
    workflow.sync(SyncMode::Reconcile).await.unwrap();
    fs::write(temp.path().join("note"), b"ONE\ntwo\nthree\n").unwrap();
    publish_files(&remote, &workspace, &[("note", b"one\ntwo\nTHREE\n")]);
    let report = workflow.sync(SyncMode::Reconcile).await.unwrap();
    assert_eq!(
        fs::read(temp.path().join("note")).unwrap(),
        b"ONE\ntwo\nTHREE\n"
    );
    assert!(report.conflicts.is_empty());
    assert!(report.published);
}

#[tokio::test]
async fn resolve_remote_deletion_can_keep_either_side() {
    for keep_local in [true, false] {
        let temp = tempdir().unwrap();
        let workspace = init_workspace(temp.path());
        let engine = LocalWorkspaceEngine::new(workspace.clone());
        let remote = FakeRemote::with_head(workspace.config.workspace_id.clone(), 0, None);
        let workflow = SyncWorkflow::new(engine.clone(), remote.clone());
        publish_files(&remote, &workspace, &[("note", b"base")]);
        workflow.sync(SyncMode::Reconcile).await.unwrap();
        fs::write(temp.path().join("note"), b"local").unwrap();
        publish_files(&remote, &workspace, &[]);
        workflow.sync(SyncMode::Reconcile).await.unwrap();
        rustsync_cli::commands::sync::resolve(
            temp.path().to_path_buf(),
            "note".into(),
            keep_local,
            !keep_local,
        )
        .unwrap();
        assert_eq!(temp.path().join("note").exists(), keep_local);
        assert!(engine.load_sync_state().unwrap().conflicts.is_empty());
        workflow.sync(SyncMode::Reconcile).await.unwrap();
        assert_eq!(temp.path().join("note").exists(), keep_local);
    }
}

#[tokio::test]
async fn conflict_copy_does_not_overwrite_existing_user_file() {
    let temp = tempdir().unwrap();
    let workspace = init_workspace(temp.path());
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let remote = FakeRemote::with_head(workspace.config.workspace_id.clone(), 0, None);
    let workflow = SyncWorkflow::new(engine.clone(), remote.clone());
    fs::write(temp.path().join("note"), b"local").unwrap();
    fs::write(
        temp.path().join("note.rustsync-conflict-remote-r1"),
        b"precious",
    )
    .unwrap();
    publish_files(&remote, &workspace, &[("note", b"remote")]);
    workflow.sync(SyncMode::Reconcile).await.unwrap();
    assert_eq!(
        fs::read(temp.path().join("note.rustsync-conflict-remote-r1")).unwrap(),
        b"precious"
    );
    let state = engine.load_sync_state().unwrap();
    let copy = &state.conflicts["note"].remote_copy_path;
    assert_eq!(fs::read(temp.path().join(copy)).unwrap(), b"remote");
}

#[tokio::test]
async fn no_op_sync_records_common_base_and_clears_pending_operation() {
    let temp = tempdir().unwrap();
    let workspace = init_workspace(temp.path());
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let remote = FakeRemote::with_head(workspace.config.workspace_id.clone(), 0, None);
    fs::write(temp.path().join("note"), b"same").unwrap();
    publish_files(&remote, &workspace, &[("note", b"same")]);
    let mut state = engine.load_sync_state().unwrap();
    state.begin_pending(rustsync_core::reconciliation::SyncPhase::Publication, 0);
    engine.save_sync_state(&state).unwrap();
    let workflow = SyncWorkflow::new(engine.clone(), remote.clone());
    workflow.sync(SyncMode::Reconcile).await.unwrap();
    let state = engine.load_sync_state().unwrap();
    assert!(state.pending_operation.is_none());
    assert!(state.last_synced_manifest.is_some());
    fs::write(temp.path().join("note"), b"edited").unwrap();
    let report = workflow.sync(SyncMode::Reconcile).await.unwrap();
    assert!(report.conflicts.is_empty());
}

#[tokio::test]
async fn directory_conflicts_preserve_both_trees_and_resolve_either_side() {
    for remote_is_directory in [true, false] {
        for keep_local in [true, false] {
            let temp = tempdir().unwrap();
            let workspace = init_workspace(temp.path());
            let engine = LocalWorkspaceEngine::new(workspace.clone());
            let remote = FakeRemote::with_head(workspace.config.workspace_id.clone(), 0, None);
            let workflow = SyncWorkflow::new(engine.clone(), remote.clone());
            publish_files(&remote, &workspace, &[("item/child", b"base")]);
            workflow.sync(SyncMode::Reconcile).await.unwrap();
            if remote_is_directory {
                fs::remove_dir_all(temp.path().join("item")).unwrap();
                fs::write(temp.path().join("item"), b"local file").unwrap();
                publish_files(&remote, &workspace, &[("item/child", b"remote child")]);
            } else {
                fs::write(temp.path().join("item/child"), b"local child").unwrap();
                publish_files(&remote, &workspace, &[("item", b"remote file")]);
            }
            let report = workflow.sync(SyncMode::Reconcile).await.unwrap();
            assert_eq!(report.conflicts, ["item"]);
            let state = engine.load_sync_state().unwrap();
            let remote_path = temp.path().join(&state.conflicts["item"].remote_copy_path);
            if remote_is_directory {
                assert_eq!(
                    fs::read(remote_path.join("child")).unwrap(),
                    b"remote child"
                );
            } else {
                assert_eq!(fs::read(&remote_path).unwrap(), b"remote file");
            }
            rustsync_cli::commands::sync::resolve(
                temp.path().to_path_buf(),
                "item".into(),
                keep_local,
                !keep_local,
            )
            .unwrap();
            let expected_directory = keep_local != remote_is_directory;
            assert_eq!(temp.path().join("item").is_dir(), expected_directory);
            assert!(!remote_path.exists());
            workflow.sync(SyncMode::Reconcile).await.unwrap();
            assert!(engine.load_sync_state().unwrap().conflicts.is_empty());
        }
    }
}

#[tokio::test]
async fn local_deletion_conflict_can_keep_either_side() {
    for keep_local in [true, false] {
        let temp = tempdir().unwrap();
        let workspace = init_workspace(temp.path());
        let engine = LocalWorkspaceEngine::new(workspace.clone());
        let remote = FakeRemote::with_head(workspace.config.workspace_id.clone(), 0, None);
        let workflow = SyncWorkflow::new(engine.clone(), remote.clone());
        publish_files(&remote, &workspace, &[("note", b"base")]);
        workflow.sync(SyncMode::Reconcile).await.unwrap();
        fs::remove_file(temp.path().join("note")).unwrap();
        publish_files(&remote, &workspace, &[("note", b"remote")]);
        workflow.sync(SyncMode::Reconcile).await.unwrap();
        rustsync_cli::commands::sync::resolve(
            temp.path().to_path_buf(),
            "note".into(),
            keep_local,
            !keep_local,
        )
        .unwrap();
        assert_eq!(temp.path().join("note").exists(), !keep_local);
        workflow.sync(SyncMode::Reconcile).await.unwrap();
        assert!(engine.load_sync_state().unwrap().conflicts.is_empty());
    }
}

#[tokio::test]
async fn interrupted_publication_resumes_without_duplicating_conflict_copies() {
    let temp = tempdir().unwrap();
    let workspace = init_workspace(temp.path());
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let remote = FakeRemote::with_head(workspace.config.workspace_id.clone(), 0, None);
    let workflow = SyncWorkflow::new(engine.clone(), remote.clone());
    publish_files(&remote, &workspace, &[("note", b"base")]);
    workflow.sync(SyncMode::Reconcile).await.unwrap();
    fs::write(temp.path().join("note"), b"local").unwrap();
    publish_files(&remote, &workspace, &[("note", b"remote")]);
    remote.state.lock().unwrap().publication_failures_remaining = 1;
    assert!(workflow.sync(SyncMode::Reconcile).await.is_err());
    let conflict = engine.load_sync_state().unwrap().conflicts["note"].clone();
    SyncWorkflow::new(LocalWorkspaceEngine::open(temp.path()).unwrap(), remote)
        .sync(SyncMode::Reconcile)
        .await
        .unwrap();
    let state = engine.load_sync_state().unwrap();
    assert_eq!(state.conflicts["note"], conflict);
    assert!(state.pending_operation.is_none());
    let files = fs::read_dir(temp.path()).unwrap().count();
    assert_eq!(
        files, 3,
        "metadata, local file, and exactly one remote copy"
    );
}

#[tokio::test]
async fn publication_races_retry_once_then_allow_a_later_sync() {
    for failures in [1, 2] {
        let temp = tempdir().unwrap();
        let workspace = init_workspace(temp.path());
        let engine = LocalWorkspaceEngine::new(workspace.clone());
        let remote = FakeRemote::with_head(workspace.config.workspace_id.clone(), 0, None);
        fs::write(temp.path().join("note"), b"local").unwrap();
        remote.state.lock().unwrap().stale_head_failures_remaining = failures;
        let workflow = SyncWorkflow::new(engine.clone(), remote.clone());
        let result = workflow.sync(SyncMode::Reconcile).await;
        if failures == 1 {
            assert_eq!(result.unwrap().retry_count, 1);
        } else {
            assert!(
                result
                    .unwrap_err()
                    .downcast_ref::<rustsync_cli::sync_workflow::SyncPublicationRace>()
                    .is_some()
            );
            workflow.sync(SyncMode::Reconcile).await.unwrap();
        }
        assert_eq!(fs::read(temp.path().join("note")).unwrap(), b"local");
        assert!(
            engine
                .load_sync_state()
                .unwrap()
                .pending_operation
                .is_none()
        );
    }
}

#[test]
fn resolve_rejects_persisted_paths_outside_the_workspace() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("workspace");
    let workspace = init_workspace(&root);
    let engine = LocalWorkspaceEngine::new(workspace);
    fs::write(temp.path().join("precious"), b"keep me").unwrap();
    let mut state = engine.load_sync_state().unwrap();
    state.conflicts.insert(
        "note".into(),
        rustsync_core::reconciliation::ConflictRecord {
            local_copy_path: "note".into(),
            remote_copy_path: "../precious".into(),
            remote_deleted: false,
        },
    );
    engine.save_sync_state(&state).unwrap();
    assert!(rustsync_cli::commands::sync::resolve(root, "note".into(), true, false).is_err());
    assert_eq!(fs::read(temp.path().join("precious")).unwrap(), b"keep me");
    assert_eq!(engine.load_sync_state().unwrap(), state);
}

#[tokio::test]
async fn sync_preserves_edits_made_while_downloading() {
    for during_manifest in [true, false] {
        let temp = tempdir().unwrap();
        let workspace = init_workspace(temp.path());
        let engine = LocalWorkspaceEngine::new(workspace.clone());
        let remote = FakeRemote::with_head(workspace.config.workspace_id.clone(), 0, None);
        let workflow = SyncWorkflow::new(engine.clone(), remote.clone());
        publish_files(&remote, &workspace, &[("note", b"base")]);
        workflow.sync(SyncMode::Reconcile).await.unwrap();
        publish_files(&remote, &workspace, &[("note", b"remote")]);
        let edit = Some((temp.path().join("note"), b"new local edit".to_vec()));
        if during_manifest {
            remote.state.lock().unwrap().edit_on_manifest_download = edit;
        } else {
            remote.state.lock().unwrap().edit_on_blob_download = edit;
        }
        assert!(workflow.sync(SyncMode::Reconcile).await.is_err());
        assert_eq!(
            fs::read(temp.path().join("note")).unwrap(),
            b"new local edit"
        );
        workflow.sync(SyncMode::Reconcile).await.unwrap();
        assert_eq!(
            fs::read(temp.path().join("note")).unwrap(),
            b"new local edit"
        );
    }
}
