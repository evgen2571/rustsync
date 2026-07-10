use std::path::{Path, PathBuf};

use rustsync_core::device::DeviceIdentity;
use rustsync_protocol::{
    AccessState, BlobId, JoinRequestId, ManifestId, UnixTimestamp, WorkspaceHead, WorkspaceId,
};
use rustsync_server::{
    error::ServerError,
    storage::{HeadUpdateResult, IndexedFsStorage, JoinRequestPutResult, PutResult},
};
use sqlx::{Row, SqlitePool};

fn workspace(value: &str) -> WorkspaceId {
    WorkspaceId::parse(value).unwrap()
}

fn state_path(root: &Path, workspace: &WorkspaceId) -> PathBuf {
    root.join("workspaces")
        .join(workspace.as_str())
        .join("state.sqlite3")
}

fn object_path(root: &Path, workspace: &WorkspaceId, id: &str) -> PathBuf {
    let hash = id
        .strip_prefix("blob_")
        .or_else(|| id.strip_prefix("manifest_"))
        .unwrap();
    root.join("workspaces")
        .join(workspace.as_str())
        .join("objects")
        .join(&hash[..2])
        .join(&hash[2..4])
        .join(format!("{hash}.enc"))
}

async fn pool(root: &Path, workspace: &WorkspaceId) -> SqlitePool {
    SqlitePool::connect(&format!(
        "sqlite://{}",
        state_path(root, workspace).display()
    ))
    .await
    .unwrap()
}

#[tokio::test]
async fn schema_is_minimal_and_rejects_future_versions() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace("workspace_schema");
    let storage = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .unwrap();
    storage.get_head(&workspace).await.unwrap();

    let db = pool(temp.path(), &workspace).await;
    let columns: Vec<String> = sqlx::query("PRAGMA table_info(objects)")
        .fetch_all(&db)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get("name"))
        .collect();
    assert_eq!(
        columns,
        [
            "object_id",
            "object_kind",
            "hash_algorithm",
            "encrypted_size",
            "created_at"
        ]
    );
    for forbidden in [
        "workspace_id",
        "storage_path",
        "key_id",
        "encryption_algorithm",
        "nonce",
        "ciphertext",
    ] {
        assert!(!columns.iter().any(|column| column == forbidden));
    }
    let imported_columns: Vec<String> = sqlx::query("PRAGMA table_info(imported_legacy_sources)")
        .fetch_all(&db)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get("name"))
        .collect();
    assert_eq!(imported_columns, ["source_fingerprint", "imported_at"]);

    sqlx::query("INSERT INTO schema_migrations(version, applied_at) VALUES(2, 0)")
        .execute(&db)
        .await
        .unwrap();
    drop(db);
    drop(storage);
    assert!(matches!(
        IndexedFsStorage::open(temp.path().to_path_buf())
            .await
            .unwrap()
            .get_head(&workspace)
            .await,
        Err(ServerError::UnsupportedSchemaVersion {
            found: 2,
            supported: 1
        })
    ));
}

#[tokio::test]
async fn typed_object_metadata_is_idempotent_and_conflicts_on_size() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace("workspace_metadata");
    let storage = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .unwrap();
    let bytes = br#"{"key_id":"key_old","encryption_algorithm":"xchacha20poly1305","nonce":[1],"ciphertext":[2]}"#;
    let blob = BlobId::from_content(bytes);
    let manifest = ManifestId::from_content(bytes);
    assert_eq!(
        storage.put_blob(&workspace, &blob, bytes).await.unwrap(),
        PutResult::Created
    );
    assert_eq!(
        storage.put_blob(&workspace, &blob, bytes).await.unwrap(),
        PutResult::AlreadyExists
    );
    assert_eq!(
        storage
            .put_manifest(&workspace, &manifest, bytes)
            .await
            .unwrap(),
        PutResult::Created
    );

    let db = pool(temp.path(), &workspace).await;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM objects")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 2, "blob and manifest retain independent typed rows");
    sqlx::query("UPDATE objects SET encrypted_size = encrypted_size + 1 WHERE object_kind = 'blob' AND object_id = ?1")
        .bind(blob.as_str())
        .execute(&db)
        .await
        .unwrap();
    assert!(matches!(
        storage.put_blob(&workspace, &blob, bytes).await,
        Err(ServerError::ObjectMetadataMismatch)
    ));
}

#[tokio::test]
async fn workspace_state_is_fresh_and_head_cas_is_shared_between_handles() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace("workspace_cas");
    let first = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .unwrap();
    let second = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .unwrap();
    assert_eq!(
        first.get_head(&workspace).await.unwrap(),
        WorkspaceHead::empty(workspace.clone())
    );

    let bytes = b"manifest needed before head update";
    let manifest = ManifestId::from_content(bytes);
    first
        .put_manifest(&workspace, &manifest, bytes)
        .await
        .unwrap();
    assert_eq!(
        first
            .update_head(&workspace, 0, manifest.clone(), None)
            .await
            .unwrap()
            .0,
        HeadUpdateResult::Updated
    );
    assert_eq!(
        second
            .update_head(&workspace, 0, manifest, None)
            .await
            .unwrap()
            .0,
        HeadUpdateResult::Conflict
    );
}

#[tokio::test]
async fn workspaces_isolate_state_objects_heads_access_state_and_join_requests() {
    let temp = tempfile::tempdir().unwrap();
    let workspace_a = workspace("workspace_isolation_a");
    let workspace_b = workspace("workspace_isolation_b");
    let storage = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .unwrap();

    let blob_a_bytes = b"blob belonging only to workspace A";
    let blob_b_bytes = b"blob belonging only to workspace B";
    let blob_a = BlobId::from_content(blob_a_bytes);
    let blob_b = BlobId::from_content(blob_b_bytes);
    storage
        .put_blob(&workspace_a, &blob_a, blob_a_bytes)
        .await
        .unwrap();
    storage
        .put_blob(&workspace_b, &blob_b, blob_b_bytes)
        .await
        .unwrap();
    assert!(state_path(temp.path(), &workspace_a).is_file());
    assert!(state_path(temp.path(), &workspace_b).is_file());
    assert!(!temp.path().join("rustsync.sqlite3").exists());
    assert_eq!(
        storage.get_blob(&workspace_a, &blob_a).await.unwrap(),
        blob_a_bytes
    );
    assert_eq!(
        storage.get_blob(&workspace_b, &blob_b).await.unwrap(),
        blob_b_bytes
    );
    assert!(matches!(
        storage.get_blob(&workspace_a, &blob_b).await,
        Err(ServerError::BlobNotFound)
    ));
    assert!(matches!(
        storage.get_blob(&workspace_b, &blob_a).await,
        Err(ServerError::BlobNotFound)
    ));

    let manifest_a_bytes = b"manifest belonging only to workspace A";
    let manifest_b_bytes = b"manifest belonging only to workspace B";
    let manifest_a = ManifestId::from_content(manifest_a_bytes);
    let manifest_b = ManifestId::from_content(manifest_b_bytes);
    storage
        .put_manifest(&workspace_a, &manifest_a, manifest_a_bytes)
        .await
        .unwrap();
    storage
        .put_manifest(&workspace_b, &manifest_b, manifest_b_bytes)
        .await
        .unwrap();
    assert_eq!(
        storage
            .update_head(&workspace_a, 0, manifest_a.clone(), None)
            .await
            .unwrap()
            .0,
        HeadUpdateResult::Updated
    );
    assert_eq!(
        storage
            .update_head(&workspace_b, 0, manifest_b.clone(), None)
            .await
            .unwrap()
            .0,
        HeadUpdateResult::Updated
    );
    assert_eq!(
        storage.get_head(&workspace_a).await.unwrap().manifest_id,
        Some(manifest_a)
    );
    assert_eq!(storage.get_head(&workspace_a).await.unwrap().revision, 1);
    assert_eq!(
        storage.get_head(&workspace_b).await.unwrap().manifest_id,
        Some(manifest_b)
    );
    assert_eq!(storage.get_head(&workspace_b).await.unwrap().revision, 1);

    let state_a = AccessState::empty(workspace_a.clone());
    storage
        .create_access_state(&workspace_a, &state_a)
        .await
        .unwrap();
    assert_eq!(
        storage.get_access_state(&workspace_a).await.unwrap(),
        state_a
    );
    assert_eq!(
        storage.get_access_state(&workspace_b).await.unwrap(),
        AccessState::empty(workspace_b.clone())
    );

    let identity = DeviceIdentity::generate("isolation test device").unwrap();
    let request = identity
        .create_join_request_at(
            JoinRequestId::parse("join_isolation_a").unwrap(),
            workspace_a.clone(),
            UnixTimestamp::from_secs(10),
        )
        .unwrap();
    assert_eq!(
        storage
            .submit_join_request(&workspace_a, &request)
            .await
            .unwrap(),
        JoinRequestPutResult::Submitted
    );
    assert_eq!(
        storage.list_join_requests(&workspace_a).await.unwrap(),
        vec![request.clone()]
    );
    assert!(
        storage
            .list_join_requests(&workspace_b)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        storage
            .get_join_request(&workspace_b, &request.request_id)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn access_state_validates_workspace_create_save_reload_and_persisted_data() {
    let temp = tempfile::tempdir().unwrap();
    let other = workspace("workspace_other");
    let workspace = workspace("workspace_access");
    let storage = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .unwrap();
    let state = AccessState::empty(workspace.clone());
    assert_eq!(storage.get_access_state(&workspace).await.unwrap(), state);
    assert!(matches!(
        storage
            .save_access_state(&workspace, &AccessState::empty(other.clone()))
            .await,
        Err(ServerError::AccessStateWorkspaceMismatch { .. })
    ));
    storage
        .create_access_state(&workspace, &state)
        .await
        .unwrap();
    assert!(matches!(
        storage.create_access_state(&workspace, &state).await,
        Err(ServerError::WorkspaceAlreadyExists)
    ));
    storage.save_access_state(&workspace, &state).await.unwrap();
    drop(storage);
    let storage = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .unwrap();
    assert_eq!(storage.get_access_state(&workspace).await.unwrap(), state);

    let db = pool(temp.path(), &workspace).await;
    sqlx::query("UPDATE access_state SET access_state_json = ?1 WHERE singleton = 1")
        .bind(serde_json::to_vec(&AccessState::empty(other)).unwrap())
        .execute(&db)
        .await
        .unwrap();
    assert!(matches!(
        storage.get_access_state(&workspace).await,
        Err(ServerError::AccessStateWorkspaceMismatch { .. })
    ));
}

#[tokio::test]
async fn join_requests_validate_workspace_are_idempotent_ordered_and_removable() {
    let temp = tempfile::tempdir().unwrap();
    let other = workspace("workspace_elsewhere");
    let workspace = workspace("workspace_requests");
    let storage = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .unwrap();
    let identity = DeviceIdentity::generate("test device").unwrap();
    let early = identity
        .create_join_request_at(
            JoinRequestId::parse("join_early").unwrap(),
            workspace.clone(),
            UnixTimestamp::from_secs(10),
        )
        .unwrap();
    let late = identity
        .create_join_request_at(
            JoinRequestId::parse("join_late").unwrap(),
            workspace.clone(),
            UnixTimestamp::from_secs(20),
        )
        .unwrap();
    let mismatch = identity.create_join_request(other).unwrap();
    assert!(matches!(
        storage.submit_join_request(&workspace, &mismatch).await,
        Err(ServerError::AccessStateWorkspaceMismatch { .. })
    ));
    assert_eq!(
        storage
            .submit_join_request(&workspace, &late)
            .await
            .unwrap(),
        JoinRequestPutResult::Submitted
    );
    assert_eq!(
        storage
            .submit_join_request(&workspace, &early)
            .await
            .unwrap(),
        JoinRequestPutResult::Submitted
    );
    assert_eq!(
        storage
            .submit_join_request(&workspace, &early)
            .await
            .unwrap(),
        JoinRequestPutResult::AlreadyPending
    );
    assert_eq!(
        storage.list_join_requests(&workspace).await.unwrap(),
        vec![early.clone(), late]
    );
    assert_eq!(
        storage
            .get_join_request(&workspace, &early.request_id)
            .await
            .unwrap(),
        Some(early.clone())
    );
    storage
        .remove_join_request(&workspace, &early.request_id)
        .await
        .unwrap();
    assert_eq!(
        storage
            .get_join_request(&workspace, &early.request_id)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn opaque_files_require_valid_physical_content_and_backfill_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace("workspace_objects");
    let storage = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .unwrap();
    let bytes = b"arbitrary opaque bytes; not an encrypted envelope";
    let blob = BlobId::from_content(bytes);
    storage.get_head(&workspace).await.unwrap();
    let db = pool(temp.path(), &workspace).await;

    sqlx::query("INSERT INTO objects(object_id, object_kind, hash_algorithm, encrypted_size, created_at) VALUES(?1, 'blob', 'sha256', ?2, 0)")
        .bind(blob.as_str())
        .bind(bytes.len() as i64)
        .execute(&db)
        .await
        .unwrap();
    assert!(matches!(
        storage.get_blob(&workspace, &blob).await,
        Err(ServerError::BlobNotFound)
    ));
    assert!(!storage.blob_exists(&workspace, &blob).await.unwrap());

    let path = object_path(temp.path(), &workspace, blob.as_str());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"corrupt physical data").unwrap();
    assert!(matches!(
        storage.get_blob(&workspace, &blob).await,
        Err(ServerError::StorageCorruption(_))
    ));
    assert!(matches!(
        storage.blob_exists(&workspace, &blob).await,
        Err(ServerError::StorageCorruption(_))
    ));
    assert!(matches!(
        storage.put_blob(&workspace, &blob, bytes).await,
        Err(ServerError::StorageCorruption(_))
    ));

    std::fs::remove_file(&path).unwrap();
    sqlx::query("DELETE FROM objects WHERE object_kind = 'blob' AND object_id = ?1")
        .bind(blob.as_str())
        .execute(&db)
        .await
        .unwrap();
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(storage.get_blob(&workspace, &blob).await.unwrap(), bytes);
    let row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM objects WHERE object_kind = 'blob' AND object_id = ?1",
    )
    .bind(blob.as_str())
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(row_count, 1, "canonical file is lazily backfilled");
}
