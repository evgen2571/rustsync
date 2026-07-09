use std::{
    path::Path,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

use crate::error::ServerResult;

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

        let url = format!("sqlite://{}", path.display());
        let options = SqliteConnectOptions::from_str(&url)?
            .create_if_missing(true)
            .foreign_keys(true);
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
        let encrypted_size = i64::try_from(metadata.encrypted_size).unwrap_or(i64::MAX);
        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO objects (
                workspace_id,
                object_id,
                object_kind,
                hash_algorithm,
                encrypted_size,
                storage_path,
                key_id,
                encryption_algorithm,
                nonce,
                created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
        )
        .bind(&metadata.workspace_id)
        .bind(&metadata.object_id)
        .bind(metadata.kind.as_str())
        .bind(&metadata.hash_algorithm)
        .bind(encrypted_size)
        .bind(&metadata.storage_path)
        .bind(&metadata.key_id)
        .bind(&metadata.encryption_algorithm)
        .bind(&metadata.nonce)
        .bind(now_unix_seconds())
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() == 1)
    }

    #[cfg(test)]
    pub(crate) async fn object_exists(
        &self,
        workspace_id: &str,
        kind: StoredObjectKind,
        object_id: &str,
    ) -> ServerResult<bool> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM objects WHERE workspace_id = ?1 AND object_kind = ?2 AND object_id = ?3",
        )
        .bind(workspace_id)
        .bind(kind.as_str())
        .bind(object_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(count > 0)
    }

    pub(crate) async fn get_object_storage_path(
        &self,
        workspace_id: &str,
        kind: StoredObjectKind,
        object_id: &str,
    ) -> ServerResult<Option<String>> {
        let storage_path = sqlx::query_scalar(
            "SELECT storage_path FROM objects WHERE workspace_id = ?1 AND object_kind = ?2 AND object_id = ?3",
        )
        .bind(workspace_id)
        .bind(kind.as_str())
        .bind(object_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(storage_path)
    }

    async fn migrate(&self) -> ServerResult<()> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(MIGRATION_001)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (1, ?1)")
            .bind(now_unix_seconds())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }
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
    use super::{StorageDb, StoredObjectKind, StoredObjectMetadata};

    #[tokio::test]
    async fn open_initializes_schema() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let db = StorageDb::open(&temp.path().join("rustsync.sqlite3"))
            .await
            .expect("open db");

        let object_table_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'objects'",
        )
        .fetch_one(db.pool())
        .await
        .expect("query schema");

        let migration_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations WHERE version = 1")
                .fetch_one(db.pool())
                .await
                .expect("query migration");

        assert_eq!(object_table_count, 1);
        assert_eq!(migration_count, 1);
    }

    #[tokio::test]
    async fn object_insert_and_lookup_are_idempotent() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let db = StorageDb::open(&temp.path().join("rustsync.sqlite3"))
            .await
            .expect("open db");
        let metadata = StoredObjectMetadata {
            workspace_id: "workspace_test".to_string(),
            object_id: "blob_abc123".to_string(),
            kind: StoredObjectKind::Blob,
            hash_algorithm: "blake3".to_string(),
            encrypted_size: 42,
            storage_path: "workspaces/workspace_test/objects/ab/c1/abc123.enc".to_string(),
            key_id: None,
            encryption_algorithm: None,
            nonce: None,
        };

        assert!(
            db.insert_object_if_absent(&metadata)
                .await
                .expect("insert metadata")
        );
        assert!(
            !db.insert_object_if_absent(&metadata)
                .await
                .expect("idempotent metadata insert")
        );
        assert!(
            db.object_exists("workspace_test", StoredObjectKind::Blob, "blob_abc123")
                .await
                .expect("object exists")
        );
        assert!(
            !db.object_exists("workspace_test", StoredObjectKind::Manifest, "blob_abc123",)
                .await
                .expect("manifest object does not exist")
        );
        assert_eq!(
            db.get_object_storage_path("workspace_test", StoredObjectKind::Blob, "blob_abc123")
                .await
                .expect("get object path"),
            Some(metadata.storage_path)
        );
    }
}
