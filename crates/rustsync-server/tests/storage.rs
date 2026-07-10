use std::path::{Path, PathBuf};

use rustsync_core::device::DeviceIdentity;
use rustsync_protocol::{
    AccessState, BlobId, ContentEncryptionAlgorithm, DeviceId, EncryptedObject, KeyId, ManifestId,
    WorkspaceHead, WorkspaceId,
};
use rustsync_server::{
    FsStorage, IndexedFsStorage,
    error::ServerError,
    storage::{HeadUpdateResult, JoinRequestPutResult, PutResult},
};

#[tokio::test]
async fn blob_objects_are_stored_under_their_workspace() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let other_workspace_id = WorkspaceId::parse("workspace_other").expect("valid workspace id");
    let bytes = b"test bytes";
    let blob_id = BlobId::from_content(bytes);

    assert_eq!(
        store
            .put_blob(&workspace_id, &blob_id, bytes)
            .await
            .expect("store blob"),
        PutResult::Created
    );
    assert_eq!(
        store
            .get_blob(&workspace_id, &blob_id)
            .await
            .expect("load stored blob"),
        bytes
    );
    assert!(
        store
            .blob_exists(&workspace_id, &blob_id)
            .await
            .expect("exists check")
    );
    assert!(
        !store
            .blob_exists(&other_workspace_id, &blob_id)
            .await
            .expect("other workspace exists check")
    );

    assert!(stored_blob_path(temp.path(), &workspace_id, &blob_id).exists());
    assert!(!old_prefixed_object_path(temp.path(), &workspace_id, blob_id.as_str()).exists());
}

#[tokio::test]
async fn blob_upload_is_idempotent_only_for_identical_bytes() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let bytes = b"test bytes";
    let blob_id = BlobId::from_content(bytes);

    assert_eq!(
        store
            .put_blob(&workspace_id, &blob_id, bytes)
            .await
            .expect("initial store"),
        PutResult::Created
    );
    assert_eq!(
        store
            .put_blob(&workspace_id, &blob_id, bytes)
            .await
            .expect("idempotent store"),
        PutResult::AlreadyExists,
    );

    assert!(matches!(
        store
            .put_blob(&workspace_id, &blob_id, b"different test bytes")
            .await
            .expect_err("different bytes must conflict"),
        ServerError::ObjectHashMismatch
    ));
}

#[tokio::test]
async fn manifest_objects_are_immutable_and_workspace_scoped() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let other_workspace_id = WorkspaceId::parse("workspace_other").expect("valid workspace id");
    let bytes = b"test manifest bytes";
    let manifest_id = ManifestId::from_content(bytes);

    assert_eq!(
        store
            .put_manifest(&workspace_id, &manifest_id, bytes)
            .await
            .expect("store manifest"),
        PutResult::Created
    );
    assert_eq!(
        store
            .put_manifest(&workspace_id, &manifest_id, bytes)
            .await
            .expect("idempotent manifest store"),
        PutResult::AlreadyExists
    );
    assert_eq!(
        store
            .get_manifest(&workspace_id, &manifest_id)
            .await
            .expect("load manifest"),
        bytes
    );
    assert!(
        !store
            .manifest_exists(&other_workspace_id, &manifest_id)
            .await
            .expect("other workspace exists check")
    );

    assert!(matches!(
        store
            .put_manifest(&workspace_id, &manifest_id, b"different encrypted manifest")
            .await
            .expect_err("different manifest bytes must conflict"),
        ServerError::ObjectHashMismatch
    ));

    assert!(stored_manifest_path(temp.path(), &workspace_id, &manifest_id).exists());
    assert!(!old_prefixed_object_path(temp.path(), &workspace_id, manifest_id.as_str()).exists());
}

#[tokio::test]
async fn blob_and_manifest_objects_share_object_store_with_unprefixed_filenames() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let blob_bytes = b"test blob bytes";
    let manifest_bytes = b"test manifest bytes";
    let blob_id = BlobId::from_content(blob_bytes);
    let manifest_id = ManifestId::from_content(manifest_bytes);

    store
        .put_blob(&workspace_id, &blob_id, blob_bytes)
        .await
        .expect("store blob");
    store
        .put_manifest(&workspace_id, &manifest_id, manifest_bytes)
        .await
        .expect("store manifest");

    assert!(stored_blob_path(temp.path(), &workspace_id, &blob_id).exists());
    assert!(stored_manifest_path(temp.path(), &workspace_id, &manifest_id).exists());
    assert!(!old_prefixed_object_path(temp.path(), &workspace_id, blob_id.as_str()).exists());
    assert!(!old_prefixed_object_path(temp.path(), &workspace_id, manifest_id.as_str()).exists());
}

#[tokio::test]
async fn workspace_heads_start_empty_and_update_with_revision_check() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let device_id = DeviceId::parse("device_test").expect("valid device id");
    let manifest_bytes = b"test manifest bytes";
    let manifest_id = ManifestId::from_content(manifest_bytes);

    let empty = store
        .get_head(&workspace_id)
        .await
        .expect("load empty head");
    assert_eq!(empty.workspace_id, workspace_id);
    assert_eq!(empty.manifest_id, None);
    assert_eq!(empty.revision, 0);
    assert_eq!(empty.updated_by, None);
    assert_eq!(empty.updated_at, None);

    store
        .put_manifest(&workspace_id, &manifest_id, manifest_bytes)
        .await
        .expect("store manifest");

    let (result, updated) = store
        .update_head(
            &workspace_id,
            0,
            manifest_id.clone(),
            Some(device_id.clone()),
        )
        .await
        .expect("update head");

    assert_eq!(result, HeadUpdateResult::Updated);
    assert_eq!(updated.manifest_id, Some(manifest_id.clone()));
    assert_eq!(updated.revision, 1);
    assert_eq!(updated.updated_by, Some(device_id));
    assert!(updated.updated_at.is_some());

    let persisted = store
        .get_head(&workspace_id)
        .await
        .expect("load persisted head");
    assert_eq!(persisted, updated);

    let (result, current) = store
        .update_head(&workspace_id, 0, manifest_id, None)
        .await
        .expect("stale update returns conflict result");

    assert_eq!(result, HeadUpdateResult::Conflict);
    assert_eq!(current, persisted);
}

#[tokio::test]
async fn head_updates_require_a_verified_manifest_file() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let workspace = WorkspaceId::parse("workspace_test").expect("workspace");
    let bytes = b"manifest for head verification";
    let manifest = ManifestId::from_content(bytes);
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");

    store
        .put_manifest(&workspace, &manifest, bytes)
        .await
        .expect("store manifest");
    std::fs::remove_file(stored_manifest_path(temp.path(), &workspace, &manifest))
        .expect("remove indexed manifest file");

    assert!(matches!(
        store
            .update_head(&workspace, 0, manifest.clone(), None)
            .await,
        Err(ServerError::ManifestNotFound)
    ));
    assert_eq!(
        store.get_head(&workspace).await.expect("unchanged head"),
        WorkspaceHead::empty(workspace.clone())
    );
}

#[tokio::test]
async fn head_updates_verify_and_backfill_a_legacy_manifest() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let workspace = WorkspaceId::parse("workspace_test").expect("workspace");
    let bytes = b"legacy manifest for head backfill";
    let manifest = ManifestId::from_content(bytes);

    FsStorage::new(temp.path().to_path_buf())
        .put_manifest(&workspace, &manifest, bytes)
        .await
        .expect("store legacy manifest without catalog metadata");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    assert!(
        object_row(temp.path(), &workspace, "manifest", manifest.as_str())
            .await
            .is_none()
    );

    let (result, head) = store
        .update_head(&workspace, 0, manifest.clone(), None)
        .await
        .expect("legacy manifest is verified and backfilled");
    assert_eq!(result, HeadUpdateResult::Updated);
    assert_eq!(head.manifest_id, Some(manifest.clone()));
    assert!(
        object_row(temp.path(), &workspace, "manifest", manifest.as_str())
            .await
            .is_some()
    );
}

#[tokio::test]
async fn workspace_access_state_starts_empty_and_persists() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");

    let empty = store
        .get_access_state(&workspace_id)
        .await
        .expect("load empty access state");
    assert_eq!(empty, AccessState::empty(workspace_id.clone()));

    store
        .save_access_state(&workspace_id, &empty)
        .await
        .expect("save access state");

    let persisted = store
        .get_access_state(&workspace_id)
        .await
        .expect("load persisted access state");
    assert_eq!(persisted, empty);
}

#[tokio::test]
async fn create_access_state_succeeds_once_and_rejects_duplicates() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let state = AccessState::empty(workspace_id.clone());

    store
        .create_access_state(&workspace_id, &state)
        .await
        .expect("create access state");

    assert_eq!(
        store
            .get_access_state(&workspace_id)
            .await
            .expect("load created state"),
        state
    );
    assert!(matches!(
        store
            .create_access_state(&workspace_id, &state)
            .await
            .expect_err("duplicate create must be rejected"),
        ServerError::WorkspaceAlreadyExists
    ));
}

#[tokio::test]
async fn pending_join_requests_are_stored_listed_and_removed() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let joining_device = DeviceIdentity::generate("phone").expect("generate joining device");
    let request = joining_device
        .create_join_request(workspace_id.clone())
        .expect("create join request");

    assert_eq!(
        store
            .submit_join_request(&workspace_id, &request)
            .await
            .expect("submit join request"),
        JoinRequestPutResult::Submitted
    );
    assert_eq!(
        store
            .submit_join_request(&workspace_id, &request)
            .await
            .expect("idempotent submit"),
        JoinRequestPutResult::AlreadyPending
    );

    let listed = store
        .list_join_requests(&workspace_id)
        .await
        .expect("list join requests");
    assert_eq!(listed, vec![request.clone()]);
    assert_eq!(
        store
            .get_join_request(&workspace_id, &request.request_id)
            .await
            .expect("get join request"),
        Some(request.clone())
    );

    store
        .remove_join_request(&workspace_id, &request.request_id)
        .await
        .expect("remove join request");
    assert!(
        store
            .get_join_request(&workspace_id, &request.request_id)
            .await
            .expect("get removed join request")
            .is_none()
    );
}

#[tokio::test]
async fn create_access_state_must_match_workspace() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let other_workspace_id = WorkspaceId::parse("workspace_other").expect("valid workspace id");
    let state = AccessState::empty(other_workspace_id);

    assert!(matches!(
        store
            .create_access_state(&workspace_id, &state)
            .await
            .expect_err("mismatched access state must be rejected"),
        ServerError::AccessStateWorkspaceMismatch { .. }
    ));
}

#[tokio::test]
async fn workspace_access_state_must_match_workspace() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let other_workspace_id = WorkspaceId::parse("workspace_other").expect("valid workspace id");
    let state = AccessState::empty(other_workspace_id);

    assert!(matches!(
        store
            .save_access_state(&workspace_id, &state)
            .await
            .expect_err("mismatched access state must be rejected"),
        ServerError::AccessStateWorkspaceMismatch { .. }
    ));
}

#[tokio::test]
async fn blob_upload_records_idempotent_object_metadata() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let bytes = b"test bytes";
    let blob_id = BlobId::from_content(bytes);

    store
        .put_blob(&workspace_id, &blob_id, bytes)
        .await
        .expect("store blob");
    store
        .put_blob(&workspace_id, &blob_id, bytes)
        .await
        .expect("store blob idempotently");

    let row = object_row(temp.path(), &workspace_id, "blob", blob_id.as_str())
        .await
        .expect("object row exists");
    assert_eq!(row.object_count, 1);
    assert_eq!(row.encrypted_size, bytes.len() as i64);
    assert_eq!(
        row.storage_path,
        relative_path(
            temp.path(),
            &stored_blob_path(temp.path(), &workspace_id, &blob_id),
        )
    );
}

#[tokio::test]
async fn manifest_upload_records_object_metadata() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let bytes = b"test manifest bytes";
    let manifest_id = ManifestId::from_content(bytes);

    store
        .put_manifest(&workspace_id, &manifest_id, bytes)
        .await
        .expect("store manifest");

    let row = object_row(temp.path(), &workspace_id, "manifest", manifest_id.as_str())
        .await
        .expect("object row exists");
    assert_eq!(row.object_count, 1);
    assert_eq!(row.encrypted_size, bytes.len() as i64);
    assert_eq!(
        row.storage_path,
        relative_path(
            temp.path(),
            &stored_manifest_path(temp.path(), &workspace_id, &manifest_id)
        )
    );
}

#[tokio::test]
async fn object_metadata_survives_storage_reopen() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let bytes = b"test bytes";
    let blob_id = BlobId::from_content(bytes);

    {
        let store = IndexedFsStorage::open(temp.path().to_path_buf())
            .await
            .expect("open indexed storage");
        store
            .put_blob(&workspace_id, &blob_id, bytes)
            .await
            .expect("store blob");
    }

    let reopened = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("reopen indexed storage");
    assert!(
        reopened
            .blob_exists(&workspace_id, &blob_id)
            .await
            .expect("blob exists after reopen")
    );
    assert_eq!(
        reopened
            .get_blob(&workspace_id, &blob_id)
            .await
            .expect("read blob after reopen"),
        bytes
    );
}

#[tokio::test]
async fn metadata_row_without_file_does_not_make_blob_read_succeed() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let bytes = b"test bytes";
    let blob_id = BlobId::from_content(bytes);

    store
        .put_blob(&workspace_id, &blob_id, bytes)
        .await
        .expect("store blob");
    std::fs::remove_file(stored_blob_path(temp.path(), &workspace_id, &blob_id))
        .expect("remove stored blob file");

    assert!(
        !store
            .blob_exists(&workspace_id, &blob_id)
            .await
            .expect("blob existence checks file")
    );
    assert!(matches!(
        store
            .get_blob(&workspace_id, &blob_id)
            .await
            .expect_err("missing file should not read successfully"),
        ServerError::BlobNotFound
    ));
}

#[tokio::test]
async fn indexed_storage_reads_legacy_files_without_metadata() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let legacy = FsStorage::new(temp.path().to_path_buf());
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let bytes = b"legacy bytes";
    let blob_id = BlobId::from_content(bytes);

    legacy
        .put_blob(&workspace_id, &blob_id, bytes)
        .await
        .expect("store legacy blob");

    let indexed = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    assert!(
        indexed
            .blob_exists(&workspace_id, &blob_id)
            .await
            .expect("legacy blob exists")
    );
    assert_eq!(
        indexed
            .get_blob(&workspace_id, &blob_id)
            .await
            .expect("read legacy blob"),
        bytes
    );
    let row = object_row(temp.path(), &workspace_id, "blob", blob_id.as_str())
        .await
        .expect("legacy read backfills catalog row");
    assert_eq!(row.hash_algorithm, "sha256");
}

#[tokio::test]
async fn identical_blob_and_manifest_bytes_create_two_typed_rows() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open");
    let workspace = WorkspaceId::parse("workspace_test").expect("workspace");
    let bytes = b"same encrypted bytes";
    let blob = BlobId::from_content(bytes);
    let manifest = ManifestId::from_content(bytes);
    store
        .put_blob(&workspace, &blob, bytes)
        .await
        .expect("blob");
    store
        .put_manifest(&workspace, &manifest, bytes)
        .await
        .expect("manifest");
    assert_eq!(
        stored_blob_path(temp.path(), &workspace, &blob),
        stored_manifest_path(temp.path(), &workspace, &manifest)
    );
    assert!(
        object_row(temp.path(), &workspace, "blob", blob.as_str())
            .await
            .is_some()
    );
    assert!(
        object_row(temp.path(), &workspace, "manifest", manifest.as_str())
            .await
            .is_some()
    );
}

#[tokio::test]
async fn corrupt_legacy_and_existing_files_are_rejected() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let workspace = WorkspaceId::parse("workspace_test").expect("workspace");
    let expected = b"expected bytes";
    let blob = BlobId::from_content(expected);
    let path = stored_blob_path(temp.path(), &workspace, &blob);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(&path, b"corrupt bytes").expect("write corrupt file");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open");
    assert!(matches!(
        store.get_blob(&workspace, &blob).await,
        Err(ServerError::StorageCorruption(_))
    ));
    assert!(matches!(
        store.put_blob(&workspace, &blob, expected).await,
        Err(ServerError::StorageCorruption(_))
    ));
    assert!(
        object_row(temp.path(), &workspace, "blob", blob.as_str())
            .await
            .is_none()
    );
}

#[tokio::test]
async fn invalid_catalog_paths_are_rejected_before_file_access() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open");
    let workspace = WorkspaceId::parse("workspace_test").expect("workspace");
    let bytes = b"catalog path bytes";
    let blob = BlobId::from_content(bytes);
    store.put_blob(&workspace, &blob, bytes).await.expect("put");
    let outside = temp
        .path()
        .parent()
        .expect("parent")
        .join("rustsync-outside-object");
    std::fs::write(&outside, bytes).expect("outside marker");
    for invalid in [
        outside.to_string_lossy().into_owned(),
        "../rustsync-outside-object".to_string(),
        "workspaces/wrong/object.enc".to_string(),
    ] {
        update_object_path(temp.path(), &workspace, "blob", blob.as_str(), &invalid).await;
        assert!(matches!(
            store.get_blob(&workspace, &blob).await,
            Err(ServerError::StorageCorruption(_))
        ));
    }
    std::fs::remove_file(outside).expect("remove marker");
}

#[tokio::test]
async fn put_repairs_a_catalog_row_whose_file_is_missing() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open");
    let workspace = WorkspaceId::parse("workspace_test").expect("workspace");
    let bytes = b"repair bytes";
    let blob = BlobId::from_content(bytes);
    store.put_blob(&workspace, &blob, bytes).await.expect("put");
    std::fs::remove_file(stored_blob_path(temp.path(), &workspace, &blob)).expect("remove");
    assert_eq!(
        store
            .put_blob(&workspace, &blob, bytes)
            .await
            .expect("repair"),
        PutResult::Created
    );
    assert_eq!(
        store
            .get_blob(&workspace, &blob)
            .await
            .expect("read repaired"),
        bytes
    );
}

#[tokio::test]
async fn mutable_state_persists_in_sqlite_across_reopen() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let workspace = WorkspaceId::parse("workspace_test").expect("workspace");
    let manifest_bytes = b"persistent manifest";
    let manifest = ManifestId::from_content(manifest_bytes);
    let identity = DeviceIdentity::generate("phone").expect("identity");
    let request = identity
        .create_join_request(workspace.clone())
        .expect("request");
    let state = AccessState::empty(workspace.clone());
    {
        let store = IndexedFsStorage::open(temp.path().to_path_buf())
            .await
            .expect("open");
        store
            .put_manifest(&workspace, &manifest, manifest_bytes)
            .await
            .expect("manifest");
        store
            .update_head(&workspace, 0, manifest.clone(), None)
            .await
            .expect("head");
        store
            .save_access_state(&workspace, &state)
            .await
            .expect("access");
        store
            .submit_join_request(&workspace, &request)
            .await
            .expect("join");
    }
    let reopened = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("reopen");
    assert_eq!(
        reopened
            .get_head(&workspace)
            .await
            .expect("head")
            .manifest_id,
        Some(manifest)
    );
    assert_eq!(
        reopened.get_access_state(&workspace).await.expect("access"),
        state
    );
    assert_eq!(
        reopened
            .get_join_request(&workspace, &request.request_id)
            .await
            .expect("join"),
        Some(request)
    );
}

#[tokio::test]
async fn independent_indexed_stores_enforce_head_cas() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let first = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("first");
    let second = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("second");
    let workspace = WorkspaceId::parse("workspace_test").expect("workspace");
    let bytes = b"cas manifest";
    let manifest = ManifestId::from_content(bytes);
    first
        .put_manifest(&workspace, &manifest, bytes)
        .await
        .expect("put");
    assert_eq!(
        first
            .update_head(&workspace, 0, manifest.clone(), None)
            .await
            .expect("first update")
            .0,
        HeadUpdateResult::Updated
    );
    assert_eq!(
        second
            .update_head(&workspace, 0, manifest, None)
            .await
            .expect("second update")
            .0,
        HeadUpdateResult::Conflict
    );
}

#[tokio::test]
async fn valid_envelope_metadata_is_recorded() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open");
    let workspace = WorkspaceId::parse("workspace_test").expect("workspace");
    let envelope = EncryptedObject::new(
        KeyId::parse("key_test").expect("key"),
        ContentEncryptionAlgorithm::XChaCha20Poly1305,
        vec![7; 24],
        vec![9],
    );
    let bytes = serde_json::to_vec(&envelope).expect("serialize");
    let blob = BlobId::from_content(&bytes);
    store
        .put_blob(&workspace, &blob, &bytes)
        .await
        .expect("put envelope");
    let row = object_row(temp.path(), &workspace, "blob", blob.as_str())
        .await
        .expect("row");
    assert_eq!(row.key_id.as_deref(), Some("key_test"));
    assert_eq!(
        row.encryption_algorithm.as_deref(),
        Some("xchacha20poly1305")
    );
    assert_eq!(row.nonce, Some(vec![7; 24]));
}

fn stored_blob_path(root: &Path, workspace_id: &WorkspaceId, blob_id: &BlobId) -> PathBuf {
    let object_hash = blob_id
        .as_str()
        .strip_prefix("blob_")
        .expect("blob id uses blob_ prefix");
    sharded_object_path(root, workspace_id, object_hash)
}

fn stored_manifest_path(
    root: &Path,
    workspace_id: &WorkspaceId,
    manifest_id: &ManifestId,
) -> PathBuf {
    let object_hash = manifest_id
        .as_str()
        .strip_prefix("manifest_")
        .expect("manifest id uses manifest_ prefix");
    sharded_object_path(root, workspace_id, object_hash)
}

fn old_prefixed_object_path(root: &Path, workspace_id: &WorkspaceId, object_id: &str) -> PathBuf {
    sharded_object_path(root, workspace_id, object_id)
}

fn sharded_object_path(root: &Path, workspace_id: &WorkspaceId, object_id: &str) -> PathBuf {
    let first = object_id.get(0..2).unwrap_or("_");
    let second = object_id.get(2..4).unwrap_or("_");

    root.join("workspaces")
        .join(workspace_id.as_str())
        .join("objects")
        .join(first)
        .join(second)
        .join(format!("{object_id}.enc"))
}

#[derive(Debug)]
struct ObjectRow {
    object_count: i64,
    hash_algorithm: String,
    encrypted_size: i64,
    storage_path: String,
    key_id: Option<String>,
    encryption_algorithm: Option<String>,
    nonce: Option<Vec<u8>>,
}

type ObjectRowTuple = (
    i64,
    String,
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<Vec<u8>>,
);

async fn object_row(
    root: &Path,
    workspace_id: &WorkspaceId,
    object_kind: &str,
    object_id: &str,
) -> Option<ObjectRow> {
    let db_url = format!("sqlite://{}", root.join("rustsync.sqlite3").display());
    let pool = sqlx::SqlitePool::connect(&db_url)
        .await
        .expect("connect to storage db");
    let (
        object_count,
        hash_algorithm,
        encrypted_size,
        storage_path,
        key_id,
        encryption_algorithm,
        nonce,
    ): ObjectRowTuple = sqlx::query_as(
        r#"
        SELECT COUNT(*), COALESCE(MAX(hash_algorithm), ''),
               COALESCE(MAX(encrypted_size), -1), COALESCE(MAX(storage_path), ''),
               MAX(key_id), MAX(encryption_algorithm), MAX(nonce)
        FROM objects
        WHERE workspace_id = ?1 AND object_kind = ?2 AND object_id = ?3
        "#,
    )
    .bind(workspace_id.as_str())
    .bind(object_kind)
    .bind(object_id)
    .fetch_one(&pool)
    .await
    .expect("query object row");

    (object_count > 0).then_some(ObjectRow {
        object_count,
        hash_algorithm,
        encrypted_size,
        storage_path,
        key_id,
        encryption_algorithm,
        nonce,
    })
}

async fn update_object_path(
    root: &Path,
    workspace: &WorkspaceId,
    kind: &str,
    id: &str,
    path: &str,
) {
    let db_url = format!("sqlite://{}", root.join("rustsync.sqlite3").display());
    let pool = sqlx::SqlitePool::connect(&db_url).await.expect("connect");
    sqlx::query("UPDATE objects SET storage_path = ?1 WHERE workspace_id = ?2 AND object_kind = ?3 AND object_id = ?4")
        .bind(path)
        .bind(workspace.as_str())
        .bind(kind)
        .bind(id)
        .execute(&pool)
        .await
        .expect("update path");
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("path is under root")
        .to_string_lossy()
        .replace('\\', "/")
}
