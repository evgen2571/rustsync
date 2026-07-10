use std::{
    path::Path,
    time::Duration,
    time::{SystemTime, UNIX_EPOCH},
};

use rustsync_protocol::{
    AccessState, DeviceId, DeviceJoinRequest, JoinRequestId, ManifestId, UnixTimestamp,
    WorkspaceHead, WorkspaceId,
};
use sqlx::{
    Row, SqlitePool,
    sqlite::{
        SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow, SqliteSynchronous,
    },
};

use crate::{
    error::{ServerError, ServerResult},
    storage::{HeadUpdateResult, JoinRequestPutResult},
};

const LATEST_SCHEMA_VERSION: i64 = 2;
pub(crate) const OBJECT_HASH_ALGORITHM: &str = "sha256";

const MIGRATION_001: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS objects (
    workspace_id TEXT NOT NULL,
    object_id TEXT NOT NULL,
    object_kind TEXT NOT NULL CHECK (object_kind IN ('blob', 'manifest', 'chunk')),
    hash_algorithm TEXT NOT NULL,
    encrypted_size INTEGER NOT NULL CHECK (encrypted_size >= 0),
    storage_path TEXT NOT NULL,
    key_id TEXT NULL,
    encryption_algorithm TEXT NULL,
    nonce BLOB NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, object_kind, object_id)
);

CREATE INDEX IF NOT EXISTS idx_objects_workspace_kind
    ON objects(workspace_id, object_kind);

CREATE UNIQUE INDEX IF NOT EXISTS idx_objects_storage_path
    ON objects(storage_path);
"#;

const MIGRATION_002: &str = r#"
DROP INDEX IF EXISTS idx_objects_storage_path;
CREATE INDEX IF NOT EXISTS idx_objects_storage_path ON objects(storage_path);
UPDATE objects SET hash_algorithm = 'sha256' WHERE hash_algorithm = 'blake3';

CREATE TABLE workspace_heads (
    workspace_id TEXT PRIMARY KEY,
    manifest_id TEXT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    updated_by TEXT NULL,
    updated_at INTEGER NULL
);
CREATE TABLE access_states (
    workspace_id TEXT PRIMARY KEY,
    access_state_json BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE join_requests (
    workspace_id TEXT NOT NULL,
    join_request_id TEXT NOT NULL,
    request_json BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, join_request_id)
);
CREATE INDEX idx_join_requests_workspace
    ON join_requests(workspace_id, created_at, join_request_id);
"#;

#[derive(Debug, Clone)]
pub(crate) struct StorageDb {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredObjectKind {
    Blob,
    Manifest,
    #[allow(dead_code)]
    Chunk,
}

impl StoredObjectKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::Manifest => "manifest",
            Self::Chunk => "chunk",
        }
    }

    fn parse(value: &str) -> ServerResult<Self> {
        match value {
            "blob" => Ok(Self::Blob),
            "manifest" => Ok(Self::Manifest),
            "chunk" => Ok(Self::Chunk),
            _ => Err(ServerError::CorruptDatabase(format!(
                "unknown object kind `{value}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredObjectMetadata {
    pub(crate) workspace_id: String,
    pub(crate) object_id: String,
    pub(crate) kind: StoredObjectKind,
    pub(crate) hash_algorithm: String,
    pub(crate) encrypted_size: u64,
    pub(crate) storage_path: String,
    pub(crate) key_id: Option<String>,
    pub(crate) encryption_algorithm: Option<String>,
    pub(crate) nonce: Option<Vec<u8>>,
}

impl StorageDb {
    pub(crate) async fn open(path: &Path) -> ServerResult<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5))
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        let db = Self { pool };
        db.migrate().await?;
        Ok(db)
    }

    #[cfg(test)]
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub(crate) async fn insert_object_if_absent(
        &self,
        metadata: &StoredObjectMetadata,
    ) -> ServerResult<bool> {
        let mut canonical = metadata.clone();
        canonical.hash_algorithm = OBJECT_HASH_ALGORITHM.to_string();
        let encrypted_size = to_i64(metadata.encrypted_size, "encrypted object size")?;
        let result = sqlx::query(
            r#"
            INSERT INTO objects (
                workspace_id, object_id, object_kind, hash_algorithm, encrypted_size,
                storage_path, key_id, encryption_algorithm, nonce, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(workspace_id, object_kind, object_id) DO NOTHING
            "#,
        )
        .bind(&metadata.workspace_id)
        .bind(&metadata.object_id)
        .bind(metadata.kind.as_str())
        .bind(OBJECT_HASH_ALGORITHM)
        .bind(encrypted_size)
        .bind(&metadata.storage_path)
        .bind(&metadata.key_id)
        .bind(&metadata.encryption_algorithm)
        .bind(&metadata.nonce)
        .bind(now_unix_seconds())
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 1 {
            return Ok(true);
        }
        match self
            .get_object_metadata(&metadata.workspace_id, metadata.kind, &metadata.object_id)
            .await?
        {
            Some(stored) if stored == canonical => Ok(false),
            Some(_) => Err(ServerError::ObjectMetadataMismatch),
            None => Err(ServerError::CorruptDatabase(
                "conflicting object row disappeared".to_string(),
            )),
        }
    }

    pub(crate) async fn get_object_metadata(
        &self,
        workspace_id: &str,
        kind: StoredObjectKind,
        object_id: &str,
    ) -> ServerResult<Option<StoredObjectMetadata>> {
        let row = sqlx::query(
            "SELECT workspace_id, object_id, object_kind, hash_algorithm, encrypted_size, storage_path, key_id, encryption_algorithm, nonce FROM objects WHERE workspace_id = ?1 AND object_kind = ?2 AND object_id = ?3",
        )
        .bind(workspace_id)
        .bind(kind.as_str())
        .bind(object_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            let size: i64 = row.try_get("encrypted_size")?;
            Ok(StoredObjectMetadata {
                workspace_id: row.try_get("workspace_id")?,
                object_id: row.try_get("object_id")?,
                kind: StoredObjectKind::parse(row.try_get("object_kind")?)?,
                hash_algorithm: row.try_get("hash_algorithm")?,
                encrypted_size: to_u64(size, "encrypted object size")?,
                storage_path: row.try_get("storage_path")?,
                key_id: row.try_get("key_id")?,
                encryption_algorithm: row.try_get("encryption_algorithm")?,
                nonce: row.try_get("nonce")?,
            })
        })
        .transpose()
    }

    pub(crate) async fn get_head(&self, workspace_id: &WorkspaceId) -> ServerResult<WorkspaceHead> {
        let row = sqlx::query("SELECT manifest_id, revision, updated_by, updated_at FROM workspace_heads WHERE workspace_id = ?1")
            .bind(workspace_id.as_str()).fetch_optional(&self.pool).await?;
        row.map_or_else(
            || Ok(WorkspaceHead::empty(workspace_id.clone())),
            |row| head_from_row(workspace_id, &row),
        )
    }

    pub(crate) async fn update_head(
        &self,
        workspace_id: &WorkspaceId,
        expected_revision: u64,
        manifest_id: ManifestId,
        updated_by: Option<DeviceId>,
    ) -> ServerResult<(HeadUpdateResult, WorkspaceHead)> {
        if self
            .get_object_metadata(
                workspace_id.as_str(),
                StoredObjectKind::Manifest,
                manifest_id.as_str(),
            )
            .await?
            .is_none()
        {
            return Err(ServerError::ManifestNotFound);
        }

        let expected = to_i64(expected_revision, "workspace head revision")?;
        let next_revision = expected_revision
            .checked_add(1)
            .ok_or(ServerError::HeadRevisionOverflow)?;
        let next = to_i64(next_revision, "workspace head revision")
            .map_err(|_| ServerError::HeadRevisionOverflow)?;

        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query(
            r#"
            INSERT INTO workspace_heads(workspace_id, manifest_id, revision, updated_by, updated_at)
            SELECT ?1, ?2, ?3, ?4, ?5 WHERE ?6 = 0
            ON CONFLICT(workspace_id) DO UPDATE SET manifest_id = excluded.manifest_id,
                revision = excluded.revision, updated_by = excluded.updated_by,
                updated_at = excluded.updated_at
            WHERE workspace_heads.revision = ?6
        "#,
        )
        .bind(workspace_id.as_str())
        .bind(manifest_id.as_str())
        .bind(next)
        .bind(updated_by.as_ref().map(DeviceId::as_str))
        .bind(now_unix_seconds())
        .bind(expected)
        .execute(&mut *transaction)
        .await?;

        let row = sqlx::query(
            "SELECT manifest_id, revision, updated_by, updated_at FROM workspace_heads WHERE workspace_id = ?1",
        )
        .bind(workspace_id.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        let head = row.map_or_else(
            || Ok(WorkspaceHead::empty(workspace_id.clone())),
            |row| head_from_row(workspace_id, &row),
        )?;
        transaction.commit().await?;

        Ok((
            if result.rows_affected() == 1 {
                HeadUpdateResult::Updated
            } else {
                HeadUpdateResult::Conflict
            },
            head,
        ))
    }

    pub(crate) async fn get_access_state(
        &self,
        workspace_id: &WorkspaceId,
    ) -> ServerResult<AccessState> {
        let bytes: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT access_state_json FROM access_states WHERE workspace_id = ?1",
        )
        .bind(workspace_id.as_str())
        .fetch_optional(&self.pool)
        .await?;
        let Some(bytes) = bytes else {
            return Ok(AccessState::empty(workspace_id.clone()));
        };
        let state =
            serde_json::from_slice(&bytes).map_err(ServerError::InvalidStoredAccessStateJson)?;
        validate_access_state(workspace_id, &state)?;
        Ok(state)
    }

    pub(crate) async fn create_access_state(
        &self,
        workspace_id: &WorkspaceId,
        state: &AccessState,
    ) -> ServerResult<()> {
        validate_access_state(workspace_id, state)?;
        let bytes = serde_json::to_vec(state).map_err(ServerError::InvalidStoredAccessStateJson)?;
        let result = sqlx::query("INSERT INTO access_states(workspace_id, access_state_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?3) ON CONFLICT(workspace_id) DO NOTHING")
            .bind(workspace_id.as_str()).bind(bytes).bind(now_unix_seconds()).execute(&self.pool).await?;
        if result.rows_affected() == 1 {
            Ok(())
        } else {
            Err(ServerError::WorkspaceAlreadyExists)
        }
    }

    pub(crate) async fn save_access_state(
        &self,
        workspace_id: &WorkspaceId,
        state: &AccessState,
    ) -> ServerResult<()> {
        validate_access_state(workspace_id, state)?;
        let bytes = serde_json::to_vec(state).map_err(ServerError::InvalidStoredAccessStateJson)?;
        sqlx::query("INSERT INTO access_states(workspace_id, access_state_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?3) ON CONFLICT(workspace_id) DO UPDATE SET access_state_json = excluded.access_state_json, updated_at = excluded.updated_at")
            .bind(workspace_id.as_str()).bind(bytes).bind(now_unix_seconds()).execute(&self.pool).await?;
        Ok(())
    }

    pub(crate) async fn submit_join_request(
        &self,
        workspace_id: &WorkspaceId,
        request: &DeviceJoinRequest,
    ) -> ServerResult<JoinRequestPutResult> {
        validate_join_request_workspace(workspace_id, request)?;
        let bytes = serialize_join_request(request)?;
        let created_at = to_i64(request.created_at.as_secs(), "join request timestamp")?;
        let result = sqlx::query("INSERT INTO join_requests(workspace_id, join_request_id, request_json, created_at) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(workspace_id, join_request_id) DO NOTHING")
            .bind(workspace_id.as_str()).bind(request.request_id.as_str()).bind(bytes).bind(created_at)
            .execute(&self.pool).await?;
        Ok(if result.rows_affected() == 1 {
            JoinRequestPutResult::Submitted
        } else {
            JoinRequestPutResult::AlreadyPending
        })
    }

    pub(crate) async fn list_join_requests(
        &self,
        workspace_id: &WorkspaceId,
    ) -> ServerResult<Vec<DeviceJoinRequest>> {
        let rows = sqlx::query("SELECT request_json FROM join_requests WHERE workspace_id = ?1 ORDER BY join_request_id")
            .bind(workspace_id.as_str()).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                let bytes: Vec<u8> = row.try_get("request_json")?;
                deserialize_join_request(workspace_id, &bytes)
            })
            .collect()
    }

    pub(crate) async fn get_join_request(
        &self,
        workspace_id: &WorkspaceId,
        join_request_id: &JoinRequestId,
    ) -> ServerResult<Option<DeviceJoinRequest>> {
        let bytes: Option<Vec<u8>> = sqlx::query_scalar("SELECT request_json FROM join_requests WHERE workspace_id = ?1 AND join_request_id = ?2")
            .bind(workspace_id.as_str()).bind(join_request_id.as_str()).fetch_optional(&self.pool).await?;
        bytes
            .map(|bytes| deserialize_join_request(workspace_id, &bytes))
            .transpose()
    }

    pub(crate) async fn remove_join_request(
        &self,
        workspace_id: &WorkspaceId,
        join_request_id: &JoinRequestId,
    ) -> ServerResult<()> {
        sqlx::query("DELETE FROM join_requests WHERE workspace_id = ?1 AND join_request_id = ?2")
            .bind(workspace_id.as_str())
            .bind(join_request_id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn migrate(&self) -> ServerResult<()> {
        let has_migrations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'")
            .fetch_one(&self.pool).await?;
        if has_migrations == 0 {
            self.apply_migration(1, MIGRATION_001).await?;
        }
        let current: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM schema_migrations")
                .fetch_one(&self.pool)
                .await?;
        if current > LATEST_SCHEMA_VERSION {
            return Err(ServerError::UnsupportedSchemaVersion {
                found: current,
                supported: LATEST_SCHEMA_VERSION,
            });
        }
        if current < 1 {
            self.apply_migration(1, MIGRATION_001).await?;
        }
        if current < 2 {
            self.apply_migration(2, MIGRATION_002).await?;
        }
        Ok(())
    }

    async fn apply_migration(&self, version: i64, sql: &str) -> ServerResult<()> {
        let mut transaction = self.pool.begin().await?;
        sqlx::raw_sql(sql).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)")
            .bind(version)
            .bind(now_unix_seconds())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}

fn head_from_row(workspace_id: &WorkspaceId, row: &SqliteRow) -> ServerResult<WorkspaceHead> {
    let manifest_id: Option<String> = row.try_get("manifest_id")?;
    let updated_by: Option<String> = row.try_get("updated_by")?;
    let updated_at: Option<i64> = row.try_get("updated_at")?;
    let revision: i64 = row.try_get("revision")?;
    Ok(WorkspaceHead {
        workspace_id: workspace_id.clone(),
        manifest_id: manifest_id
            .map(|value| {
                ManifestId::parse(value)
                    .map_err(|error| ServerError::CorruptDatabase(error.to_string()))
            })
            .transpose()?,
        revision: to_u64(revision, "workspace head revision")?,
        updated_by: updated_by
            .map(|value| {
                DeviceId::parse(value)
                    .map_err(|error| ServerError::CorruptDatabase(error.to_string()))
            })
            .transpose()?,
        updated_at: updated_at
            .map(|value| to_u64(value, "workspace head timestamp").map(UnixTimestamp::from_secs))
            .transpose()?,
    })
}

fn validate_access_state(workspace_id: &WorkspaceId, state: &AccessState) -> ServerResult<()> {
    if state.workspace_id() != workspace_id {
        return Err(ServerError::AccessStateWorkspaceMismatch {
            expected: workspace_id.clone(),
            actual: state.workspace_id().clone(),
        });
    }
    state.validate().map_err(ServerError::InvalidAccessState)
}

fn validate_join_request_workspace(
    workspace_id: &WorkspaceId,
    request: &DeviceJoinRequest,
) -> ServerResult<()> {
    if &request.workspace_id != workspace_id {
        return Err(ServerError::AccessStateWorkspaceMismatch {
            expected: workspace_id.clone(),
            actual: request.workspace_id.clone(),
        });
    }
    Ok(())
}

fn serialize_join_request(request: &DeviceJoinRequest) -> ServerResult<Vec<u8>> {
    serde_json::to_vec(request)
        .map_err(|error| ServerError::InvalidRequest(format!("invalid join request: {error}")))
}

fn deserialize_join_request(
    workspace_id: &WorkspaceId,
    bytes: &[u8],
) -> ServerResult<DeviceJoinRequest> {
    let request = serde_json::from_slice(bytes).map_err(|error| {
        ServerError::CorruptDatabase(format!("invalid stored join request: {error}"))
    })?;
    validate_join_request_workspace(workspace_id, &request)?;
    Ok(request)
}

fn to_i64(value: u64, field: &'static str) -> ServerResult<i64> {
    i64::try_from(value).map_err(|_| ServerError::IntegerOutOfRange(field))
}

fn to_u64(value: i64, field: &'static str) -> ServerResult<u64> {
    u64::try_from(value).map_err(|_| ServerError::CorruptDatabase(format!("negative {field}")))
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use rustsync_protocol::{
        AccessState, DeviceId, DeviceJoinRequest, DeviceRecord, DeviceStatus, JoinRequestId,
        ManifestId, UnixTimestamp, WorkspaceId,
    };

    use crate::{
        error::ServerError,
        storage::{HeadUpdateResult, JoinRequestPutResult},
    };

    use super::{MIGRATION_001, StorageDb, StoredObjectKind, StoredObjectMetadata};

    fn metadata(kind: StoredObjectKind, object_id: &str) -> StoredObjectMetadata {
        StoredObjectMetadata {
            workspace_id: "workspace_test".to_string(),
            object_id: object_id.to_string(),
            kind,
            hash_algorithm: "sha256".to_string(),
            encrypted_size: 42,
            storage_path: format!("workspaces/workspace_test/objects/{object_id}.enc"),
            key_id: None,
            encryption_algorithm: None,
            nonce: None,
        }
    }

    fn join_request(workspace_id: &WorkspaceId, id: &str, created_at: u64) -> DeviceJoinRequest {
        DeviceJoinRequest::new_unsigned(
            JoinRequestId::parse(id).expect("valid request id"),
            workspace_id.clone(),
            DeviceRecord {
                device_id: DeviceId::parse(format!("device_{id}")).expect("valid device id"),
                device_name: id.to_string(),
                signing_public_key: [1; 32],
                exchange_public_key: [2; 32],
                fingerprint: "fingerprint".to_string(),
                status: DeviceStatus::Pending,
            },
            UnixTimestamp::from_secs(created_at),
        )
        .with_signature(vec![1])
    }

    #[tokio::test]
    async fn migrates_v1_to_v2_and_corrects_catalog() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("rustsync.sqlite3");
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open v1 db");
        sqlx::raw_sql(MIGRATION_001)
            .execute(&pool)
            .await
            .expect("create v1 schema");
        sqlx::query("INSERT INTO schema_migrations(version, applied_at) VALUES (1, 0)")
            .execute(&pool)
            .await
            .expect("record v1");
        sqlx::query("INSERT INTO objects VALUES ('workspace_test', 'blob_old', 'blob', 'blake3', 1, 'same/path', NULL, NULL, NULL, 0)")
            .execute(&pool)
            .await
            .expect("insert legacy object");
        pool.close().await;

        let db = StorageDb::open(&path).await.expect("migrate db");
        let algorithm: String = sqlx::query_scalar("SELECT hash_algorithm FROM objects")
            .fetch_one(db.pool())
            .await
            .expect("read algorithm");
        let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
            .fetch_one(db.pool())
            .await
            .expect("read version");
        assert_eq!(algorithm, "sha256");
        assert_eq!(version, 2);
        assert!(
            db.insert_object_if_absent(&StoredObjectMetadata {
                storage_path: "same/path".to_string(),
                ..metadata(StoredObjectKind::Blob, "blob_new")
            })
            .await
            .expect("storage paths are not unique")
        );
    }

    #[tokio::test]
    async fn rejects_future_schema_version() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("rustsync.sqlite3");
        let db = StorageDb::open(&path).await.expect("open db");
        sqlx::query("INSERT INTO schema_migrations(version, applied_at) VALUES (99, 0)")
            .execute(db.pool())
            .await
            .expect("insert future version");
        db.pool().close().await;

        assert!(matches!(
            StorageDb::open(&path).await,
            Err(ServerError::UnsupportedSchemaVersion {
                found: 99,
                supported: 2
            })
        ));
    }

    #[tokio::test]
    async fn duplicate_object_requires_identical_metadata() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let db = StorageDb::open(&temp.path().join("rustsync.sqlite3"))
            .await
            .expect("open db");
        let original = metadata(StoredObjectKind::Blob, "blob_a");
        assert!(db.insert_object_if_absent(&original).await.expect("insert"));
        assert!(!db.insert_object_if_absent(&original).await.expect("repeat"));
        let mut conflicting = original;
        conflicting.encrypted_size += 1;
        assert!(matches!(
            db.insert_object_if_absent(&conflicting).await,
            Err(ServerError::ObjectMetadataMismatch)
        ));
    }

    #[tokio::test]
    async fn head_update_conflicts_across_independent_databases() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("rustsync.sqlite3");
        let first = StorageDb::open(&path).await.expect("open first");
        let second = StorageDb::open(&path).await.expect("open second");
        let workspace = WorkspaceId::parse("workspace_test").expect("workspace");
        let manifest = ManifestId::from_content(b"manifest");
        first
            .insert_object_if_absent(&metadata(StoredObjectKind::Manifest, manifest.as_str()))
            .await
            .expect("insert manifest");

        let (result, head) = first
            .update_head(&workspace, 0, manifest.clone(), None)
            .await
            .expect("first update");
        assert_eq!(result, HeadUpdateResult::Updated);
        assert_eq!(head.revision, 1);
        let (result, head) = second
            .update_head(&workspace, 0, manifest, None)
            .await
            .expect("conflicting update");
        assert_eq!(result, HeadUpdateResult::Conflict);
        assert_eq!(head.revision, 1);
    }

    #[tokio::test]
    async fn access_state_persists_and_create_is_once_only() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("rustsync.sqlite3");
        let db = StorageDb::open(&path).await.expect("open db");
        let workspace = WorkspaceId::parse("workspace_test").expect("workspace");
        let state = AccessState::empty(workspace.clone());
        assert_eq!(db.get_access_state(&workspace).await.expect("empty"), state);
        db.create_access_state(&workspace, &state)
            .await
            .expect("create");
        assert!(matches!(
            db.create_access_state(&workspace, &state).await,
            Err(ServerError::WorkspaceAlreadyExists)
        ));
        db.save_access_state(&workspace, &state)
            .await
            .expect("save");
        drop(db);
        let reopened = StorageDb::open(&path).await.expect("reopen");
        assert_eq!(
            reopened.get_access_state(&workspace).await.expect("load"),
            state
        );
    }

    #[tokio::test]
    async fn join_requests_are_idempotent_ordered_and_removable() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let db = StorageDb::open(&temp.path().join("rustsync.sqlite3"))
            .await
            .expect("open db");
        let workspace = WorkspaceId::parse("workspace_test").expect("workspace");
        let later = join_request(&workspace, "join_b", 20);
        let earlier = join_request(&workspace, "join_a", 10);
        assert_eq!(
            db.submit_join_request(&workspace, &later)
                .await
                .expect("submit"),
            JoinRequestPutResult::Submitted
        );
        assert_eq!(
            db.submit_join_request(&workspace, &later)
                .await
                .expect("repeat"),
            JoinRequestPutResult::AlreadyPending
        );
        db.submit_join_request(&workspace, &earlier)
            .await
            .expect("submit earlier");
        assert_eq!(
            db.list_join_requests(&workspace).await.expect("list"),
            vec![earlier.clone(), later.clone()]
        );
        assert_eq!(
            db.get_join_request(&workspace, &earlier.request_id)
                .await
                .expect("get"),
            Some(earlier.clone())
        );
        db.remove_join_request(&workspace, &earlier.request_id)
            .await
            .expect("remove");
        db.remove_join_request(&workspace, &earlier.request_id)
            .await
            .expect("remove idempotently");
        assert_eq!(
            db.list_join_requests(&workspace).await.expect("list"),
            vec![later]
        );
    }
}
