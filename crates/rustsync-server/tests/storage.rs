use rustsync_protocol::{BlobId, ManifestId, WorkspaceId};
use rustsync_server::{FsStorage, error::ServerError, storage::PutResult};

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
