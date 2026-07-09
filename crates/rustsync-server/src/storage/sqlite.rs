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

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
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
    use super::StorageDb;

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
}
