use rustsync_protocol::{BlobId, DeviceId, ManifestId, WorkspaceId};
use rustsync_server::{
    FsStorage,
    error::ServerError,
    storage::{HeadUpdateResult, PutResult},
};

#[tokio::test]
async fn blob_objects_are_stored_under_their_workspace() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = FsStorage::new(temp.path().to_path_buf());
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
    )
}

#[tokio::test]
async fn blob_upload_is_idempotent_only_for_identical_bytes() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = FsStorage::new(temp.path().to_path_buf());
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
    let store = FsStorage::new(temp.path().to_path_buf());
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
}

#[tokio::test]
async fn workspace_heads_start_empty_and_update_with_revision_check() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let store = FsStorage::new(temp.path().to_path_buf());
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
