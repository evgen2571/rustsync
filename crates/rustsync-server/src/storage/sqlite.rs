use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
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

const LATEST_SCHEMA_VERSION: i64 = 1;
pub(crate) const OBJECT_HASH_ALGORITHM: &str = "sha256";
type ColumnDefinition = (&'static str, &'static str, bool, i64);
type TableColumns = (&'static str, &'static [ColumnDefinition]);

const V1_OBJECTS: &[(&str, &str)] = &[
    ("table", "schema_migrations"),
    ("table", "objects"),
    ("index", "idx_objects_id"),
    ("table", "workspace_head"),
    ("table", "access_state"),
    ("table", "join_requests"),
    ("index", "idx_join_requests_order"),
];
const V1_COLUMNS: &[TableColumns] = &[
    (
        "schema_migrations",
        &[
            ("version", "INTEGER", false, 1),
            ("applied_at", "INTEGER", true, 0),
        ],
    ),
    (
        "objects",
        &[
            ("object_id", "TEXT", true, 2),
            ("object_kind", "TEXT", true, 1),
            ("hash_algorithm", "TEXT", true, 0),
            ("encrypted_size", "INTEGER", true, 0),
            ("created_at", "INTEGER", true, 0),
        ],
    ),
    (
        "workspace_head",
        &[
            ("singleton", "INTEGER", false, 1),
            ("manifest_id", "TEXT", false, 0),
            ("revision", "INTEGER", true, 0),
            ("updated_by", "TEXT", false, 0),
            ("updated_at", "INTEGER", false, 0),
        ],
    ),
    (
        "access_state",
        &[
            ("singleton", "INTEGER", false, 1),
            ("access_state_json", "BLOB", true, 0),
            ("created_at", "INTEGER", true, 0),
            ("updated_at", "INTEGER", true, 0),
        ],
    ),
    (
        "join_requests",
        &[
            ("join_request_id", "TEXT", false, 1),
            ("request_json", "BLOB", true, 0),
            ("created_at", "INTEGER", true, 0),
        ],
    ),
];
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS objects (
 object_id TEXT NOT NULL, object_kind TEXT NOT NULL CHECK(object_kind IN ('blob','manifest')),
 hash_algorithm TEXT NOT NULL CHECK(hash_algorithm = 'sha256'), encrypted_size INTEGER NOT NULL CHECK(encrypted_size >= 0),
 created_at INTEGER NOT NULL, PRIMARY KEY(object_kind, object_id));
CREATE INDEX IF NOT EXISTS idx_objects_id ON objects(object_id);
CREATE TABLE IF NOT EXISTS workspace_head (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), manifest_id TEXT NULL, revision INTEGER NOT NULL CHECK(revision >= 0), updated_by TEXT NULL, updated_at INTEGER NULL);
CREATE TABLE IF NOT EXISTS access_state (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), access_state_json BLOB NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS join_requests (join_request_id TEXT PRIMARY KEY, request_json BLOB NOT NULL, created_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS idx_join_requests_order ON join_requests(created_at, join_request_id);
"#;

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceDb {
    pool: SqlitePool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredObjectKind {
    Blob,
    Manifest,
}
impl StoredObjectKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::Manifest => "manifest",
        }
    }
    fn parse(value: &str) -> ServerResult<Self> {
        match value {
            "blob" => Ok(Self::Blob),
            "manifest" => Ok(Self::Manifest),
            _ => Err(ServerError::CorruptDatabase(format!(
                "unknown object kind `{value}`"
            ))),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredObjectMetadata {
    pub(crate) object_id: String,
    pub(crate) kind: StoredObjectKind,
    pub(crate) encrypted_size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectInsertResult {
    Inserted,
    ExactExisting,
}

#[derive(Debug)]
struct StoredObjectRow {
    kind: StoredObjectKind,
    hash_algorithm: String,
    encrypted_size: u64,
}

impl WorkspaceDb {
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
    pub(crate) async fn insert_object_if_absent(
        &self,
        metadata: &StoredObjectMetadata,
    ) -> ServerResult<ObjectInsertResult> {
        let size = to_i64(metadata.encrypted_size, "encrypted object size")?;
        let existing = self
            .object_rows_by_id(metadata.kind, &metadata.object_id)
            .await?;
        if !existing.is_empty() {
            return Self::validate_object_rows(metadata, &existing);
        }
        let conflicting = self.object_rows_by_object_id(&metadata.object_id).await?;
        if !conflicting.is_empty() {
            return Self::validate_object_rows(metadata, &conflicting);
        }

        let result = sqlx::query("INSERT INTO objects(object_id,object_kind,hash_algorithm,encrypted_size,created_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(object_kind,object_id) DO NOTHING")
   .bind(&metadata.object_id).bind(metadata.kind.as_str()).bind(OBJECT_HASH_ALGORITHM).bind(size).bind(now()).execute(&self.pool).await?;
        let outcome = Self::validate_object_rows(
            metadata,
            &self
                .object_rows_by_id(metadata.kind, &metadata.object_id)
                .await?,
        )?;
        Ok(if result.rows_affected() == 1 {
            ObjectInsertResult::Inserted
        } else {
            outcome
        })
    }
    fn validate_object_rows(
        metadata: &StoredObjectMetadata,
        rows: &[StoredObjectRow],
    ) -> ServerResult<ObjectInsertResult> {
        if rows.len() == 1
            && rows.iter().all(|row| {
                row.kind == metadata.kind
                    && row.encrypted_size == metadata.encrypted_size
                    && row.hash_algorithm == OBJECT_HASH_ALGORITHM
            })
        {
            return Ok(ObjectInsertResult::ExactExisting);
        }
        Err(ServerError::StorageCorruption(format!(
            "catalog metadata for {} `{}` does not match its physical object",
            metadata.kind.as_str(),
            metadata.object_id
        )))
    }
    pub(crate) async fn get_object_metadata(
        &self,
        kind: StoredObjectKind,
        object_id: &str,
    ) -> ServerResult<Option<StoredObjectMetadata>> {
        let rows = self.object_rows_by_id(kind, object_id).await?;
        match rows.as_slice() {
            [] => {
                let conflicting = self.object_rows_by_object_id(object_id).await?;
                if conflicting.is_empty() {
                    Ok(None)
                } else {
                    Err(ServerError::StorageCorruption(format!(
                        "catalog metadata for {} `{object_id}` is stored under the wrong object kind",
                        kind.as_str()
                    )))
                }
            }
            [row] if row.kind == kind && row.hash_algorithm == OBJECT_HASH_ALGORITHM => {
                Ok(Some(StoredObjectMetadata {
                    object_id: object_id.into(),
                    kind: row.kind,
                    encrypted_size: row.encrypted_size,
                }))
            }
            _ => Err(ServerError::StorageCorruption(format!(
                "catalog metadata for {} `{object_id}` is invalid",
                kind.as_str()
            ))),
        }
    }
    async fn object_rows_by_id(
        &self,
        kind: StoredObjectKind,
        object_id: &str,
    ) -> ServerResult<Vec<StoredObjectRow>> {
        sqlx::query(
            "SELECT object_kind,hash_algorithm,encrypted_size FROM objects WHERE object_kind=?1 AND object_id=?2",
        )
        .bind(kind.as_str())
        .bind(object_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| {
            Ok(StoredObjectRow {
                kind: StoredObjectKind::parse(row.try_get("object_kind")?)?,
                hash_algorithm: row.try_get("hash_algorithm")?,
                encrypted_size: to_u64(row.try_get("encrypted_size")?, "encrypted object size")?,
            })
        })
        .collect()
    }
    async fn object_rows_by_object_id(
        &self,
        object_id: &str,
    ) -> ServerResult<Vec<StoredObjectRow>> {
        sqlx::query(
            "SELECT object_kind,hash_algorithm,encrypted_size FROM objects WHERE object_id=?1",
        )
        .bind(object_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| {
            Ok(StoredObjectRow {
                kind: StoredObjectKind::parse(row.try_get("object_kind")?)?,
                hash_algorithm: row.try_get("hash_algorithm")?,
                encrypted_size: to_u64(row.try_get("encrypted_size")?, "encrypted object size")?,
            })
        })
        .collect()
    }
    pub(crate) async fn get_head(&self, workspace: &WorkspaceId) -> ServerResult<WorkspaceHead> {
        let row=sqlx::query("SELECT manifest_id,revision,updated_by,updated_at FROM workspace_head WHERE singleton=1").fetch_optional(&self.pool).await?;
        row.map_or_else(
            || Ok(WorkspaceHead::empty(workspace.clone())),
            |r| head(workspace, &r),
        )
    }
    pub(crate) async fn update_head(
        &self,
        workspace: &WorkspaceId,
        expected_revision: u64,
        manifest_id: ManifestId,
        updated_by: Option<DeviceId>,
    ) -> ServerResult<(HeadUpdateResult, WorkspaceHead)> {
        let expected = to_i64(expected_revision, "workspace head revision")?;
        let next = to_i64(
            expected_revision
                .checked_add(1)
                .ok_or(ServerError::HeadRevisionOverflow)?,
            "workspace head revision",
        )
        .map_err(|_| ServerError::HeadRevisionOverflow)?;
        let mut tx = self.pool.begin().await?;
        let r=sqlx::query("INSERT INTO workspace_head(singleton,manifest_id,revision,updated_by,updated_at) SELECT 1,?1,?2,?3,?4 WHERE ?5=0 ON CONFLICT(singleton) DO UPDATE SET manifest_id=excluded.manifest_id,revision=excluded.revision,updated_by=excluded.updated_by,updated_at=excluded.updated_at WHERE workspace_head.revision=?5").bind(manifest_id.as_str()).bind(next).bind(updated_by.as_ref().map(DeviceId::as_str)).bind(now()).bind(expected).execute(&mut *tx).await?;
        let row=sqlx::query("SELECT manifest_id,revision,updated_by,updated_at FROM workspace_head WHERE singleton=1").fetch_optional(&mut *tx).await?;
        let h = row.map_or_else(
            || Ok(WorkspaceHead::empty(workspace.clone())),
            |x| head(workspace, &x),
        )?;
        tx.commit().await?;
        Ok((
            if r.rows_affected() == 1 {
                HeadUpdateResult::Updated
            } else {
                HeadUpdateResult::Conflict
            },
            h,
        ))
    }
    pub(crate) async fn get_access_state(
        &self,
        workspace: &WorkspaceId,
    ) -> ServerResult<AccessState> {
        let bytes: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT access_state_json FROM access_state WHERE singleton=1")
                .fetch_optional(&self.pool)
                .await?;
        let Some(bytes) = bytes else {
            return Ok(AccessState::empty(workspace.clone()));
        };
        let s =
            serde_json::from_slice(&bytes).map_err(ServerError::InvalidStoredAccessStateJson)?;
        validate_state(workspace, &s)?;
        Ok(s)
    }
    pub(crate) async fn create_access_state(
        &self,
        workspace: &WorkspaceId,
        state: &AccessState,
    ) -> ServerResult<()> {
        validate_state(workspace, state)?;
        let bytes = serde_json::to_vec(state).map_err(ServerError::InvalidStoredAccessStateJson)?;
        let r=sqlx::query("INSERT INTO access_state(singleton,access_state_json,created_at,updated_at) VALUES(1,?1,?2,?2) ON CONFLICT(singleton) DO NOTHING").bind(bytes).bind(now()).execute(&self.pool).await?;
        if r.rows_affected() == 1 {
            Ok(())
        } else {
            Err(ServerError::WorkspaceAlreadyExists)
        }
    }
    pub(crate) async fn save_access_state(
        &self,
        workspace: &WorkspaceId,
        state: &AccessState,
    ) -> ServerResult<()> {
        validate_state(workspace, state)?;
        let bytes = serde_json::to_vec(state).map_err(ServerError::InvalidStoredAccessStateJson)?;
        sqlx::query("INSERT INTO access_state(singleton,access_state_json,created_at,updated_at) VALUES(1,?1,?2,?2) ON CONFLICT(singleton) DO UPDATE SET access_state_json=excluded.access_state_json,updated_at=excluded.updated_at").bind(bytes).bind(now()).execute(&self.pool).await?;
        Ok(())
    }
    pub(crate) async fn submit_join_request(
        &self,
        workspace: &WorkspaceId,
        request: &DeviceJoinRequest,
    ) -> ServerResult<JoinRequestPutResult> {
        validate_request(workspace, request)?;
        let b =
            serde_json::to_vec(request).map_err(|e| ServerError::InvalidRequest(e.to_string()))?;
        let r=sqlx::query("INSERT INTO join_requests(join_request_id,request_json,created_at) VALUES(?1,?2,?3) ON CONFLICT(join_request_id) DO NOTHING").bind(request.request_id.as_str()).bind(b).bind(to_i64(request.created_at.as_secs(),"join request timestamp")?).execute(&self.pool).await?;
        if r.rows_affected() == 1 {
            return Ok(JoinRequestPutResult::Submitted);
        }

        let Some(existing_bytes): Option<Vec<u8>> =
            sqlx::query_scalar("SELECT request_json FROM join_requests WHERE join_request_id = ?1")
                .bind(request.request_id.as_str())
                .fetch_optional(&self.pool)
                .await?
        else {
            return Err(ServerError::StorageCorruption(format!(
                "join request `{}` conflicted during insert but no stored request exists",
                request.request_id
            )));
        };
        let existing: DeviceJoinRequest = serde_json::from_slice(&existing_bytes).map_err(|e| {
            ServerError::StorageCorruption(format!(
                "stored join request `{}` is invalid JSON: {e}",
                request.request_id
            ))
        })?;
        validate_request(workspace, &existing)?;
        if existing != *request {
            return Err(ServerError::StorageCorruption(format!(
                "join request `{}` conflicts with its stored payload",
                request.request_id
            )));
        }
        Ok(JoinRequestPutResult::AlreadyPending)
    }
    pub(crate) async fn list_join_requests(
        &self,
        workspace: &WorkspaceId,
    ) -> ServerResult<Vec<DeviceJoinRequest>> {
        let rows = sqlx::query(
            "SELECT request_json FROM join_requests ORDER BY created_at,join_request_id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| deserialize_request(workspace, &r.try_get::<Vec<u8>, _>("request_json")?))
            .collect()
    }
    pub(crate) async fn get_join_request(
        &self,
        workspace: &WorkspaceId,
        id: &JoinRequestId,
    ) -> ServerResult<Option<DeviceJoinRequest>> {
        let b: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT request_json FROM join_requests WHERE join_request_id=?1")
                .bind(id.as_str())
                .fetch_optional(&self.pool)
                .await?;
        b.map(|x| deserialize_request(workspace, &x)).transpose()
    }
    pub(crate) async fn remove_join_request(
        &self,
        _workspace: &WorkspaceId,
        id: &JoinRequestId,
    ) -> ServerResult<()> {
        sqlx::query("DELETE FROM join_requests WHERE join_request_id=?1")
            .bind(id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    async fn migrate(&self) -> ServerResult<()> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await?;
        let result = async {
            let objects = schema_objects(&mut connection).await?;
            if objects.is_empty() {
                for statement in SCHEMA
                    .split(';')
                    .filter(|statement| !statement.trim().is_empty())
                {
                    sqlx::query(statement).execute(&mut *connection).await?;
                }
                sqlx::query("INSERT INTO schema_migrations(version, applied_at) VALUES(1, ?1)")
                    .bind(now())
                    .execute(&mut *connection)
                    .await?;
                let objects = schema_objects(&mut connection).await?;
                return validate_v1_schema(&mut connection, &objects).await;
            }

            validate_v1_schema(&mut connection, &objects).await?;
            let versions: Vec<i64> =
                sqlx::query_scalar("SELECT version FROM schema_migrations ORDER BY version")
                    .fetch_all(&mut *connection)
                    .await?;
            match versions.as_slice() {
                [] => {
                    sqlx::query("INSERT INTO schema_migrations(version, applied_at) VALUES(1, ?1)")
                        .bind(now())
                        .execute(&mut *connection)
                        .await?;
                }
                [LATEST_SCHEMA_VERSION] => {}
                _ if versions
                    .iter()
                    .any(|&version| version > LATEST_SCHEMA_VERSION) =>
                {
                    return Err(ServerError::UnsupportedSchemaVersion {
                        found: *versions.last().expect("non-empty migration history"),
                        supported: LATEST_SCHEMA_VERSION,
                    });
                }
                _ => {
                    return Err(ServerError::CorruptDatabase(
                        "invalid schema migration history".into(),
                    ));
                }
            }
            Ok(())
        }
        .await;

        match result {
            Ok(()) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(())
            }
            Err(error) => {
                sqlx::query("ROLLBACK").execute(&mut *connection).await?;
                Err(error)
            }
        }
    }
}

async fn schema_objects(
    connection: &mut sqlx::SqliteConnection,
) -> ServerResult<Vec<(String, String, Option<String>)>> {
    Ok(sqlx::query_as(
        "SELECT type, name, sql FROM sqlite_master \
         WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
    )
    .fetch_all(connection)
    .await?)
}

async fn validate_v1_schema(
    connection: &mut sqlx::SqliteConnection,
    objects: &[(String, String, Option<String>)],
) -> ServerResult<()> {
    let mut expected_objects: Vec<_> = V1_OBJECTS
        .iter()
        .map(|(object_type, name)| ((*object_type).to_owned(), (*name).to_owned()))
        .collect();
    expected_objects.sort_unstable();
    let actual_objects: Vec<_> = objects
        .iter()
        .map(|(object_type, name, _)| (object_type.clone(), name.clone()))
        .collect();
    if actual_objects != expected_objects {
        return Err(ServerError::CorruptDatabase(
            "unexpected V1 schema objects".into(),
        ));
    }

    for (object_type, name, sql) in objects {
        let expected = expected_object_sql(name).ok_or_else(|| {
            ServerError::CorruptDatabase(format!("unexpected {object_type} `{name}`"))
        })?;
        if normalize_sql(sql.as_deref().unwrap_or_default()) != normalize_sql(expected) {
            return Err(ServerError::CorruptDatabase(format!(
                "unexpected definition for {object_type} `{name}`"
            )));
        }
    }

    for (table, expected_columns) in V1_COLUMNS {
        let actual_columns: Vec<(String, String, i64, i64)> = sqlx::query_as(&format!(
            "SELECT name, type, \"notnull\", pk FROM pragma_table_info('{table}') ORDER BY cid"
        ))
        .fetch_all(&mut *connection)
        .await?;
        let expected_columns: Vec<_> = expected_columns
            .iter()
            .map(|(name, column_type, not_null, primary_key)| {
                (
                    (*name).to_owned(),
                    (*column_type).to_owned(),
                    i64::from(*not_null),
                    *primary_key,
                )
            })
            .collect();
        if actual_columns != expected_columns {
            return Err(ServerError::CorruptDatabase(format!(
                "unexpected columns for table `{table}`"
            )));
        }
    }

    let quick_check: Vec<String> = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_all(&mut *connection)
        .await?;
    if quick_check.as_slice() != ["ok"] {
        return Err(ServerError::CorruptDatabase(format!(
            "SQLite quick_check failed: {}",
            quick_check.join("; ")
        )));
    }
    Ok(())
}

fn expected_object_sql(name: &str) -> Option<&'static str> {
    let statement = match name {
        "schema_migrations" => 0,
        "objects" => 1,
        "idx_objects_id" => 2,
        "workspace_head" => 3,
        "access_state" => 4,
        "join_requests" => 5,
        "idx_join_requests_order" => 6,
        _ => return None,
    };
    SCHEMA.split(';').nth(statement)
}

fn normalize_sql(sql: &str) -> String {
    sql.chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .replace("ifnotexists", "")
}

fn head(w: &WorkspaceId, r: &SqliteRow) -> ServerResult<WorkspaceHead> {
    Ok(WorkspaceHead {
        workspace_id: w.clone(),
        manifest_id: r
            .try_get::<Option<String>, _>("manifest_id")?
            .map(|s| ManifestId::parse(s).map_err(|e| ServerError::CorruptDatabase(e.to_string())))
            .transpose()?,
        revision: to_u64(r.try_get("revision")?, "workspace head revision")?,
        updated_by: r
            .try_get::<Option<String>, _>("updated_by")?
            .map(|s| DeviceId::parse(s).map_err(|e| ServerError::CorruptDatabase(e.to_string())))
            .transpose()?,
        updated_at: r
            .try_get::<Option<i64>, _>("updated_at")?
            .map(|x| to_u64(x, "workspace head timestamp").map(UnixTimestamp::from_secs))
            .transpose()?,
    })
}
fn validate_state(w: &WorkspaceId, s: &AccessState) -> ServerResult<()> {
    if s.workspace_id() != w {
        return Err(ServerError::AccessStateWorkspaceMismatch {
            expected: w.clone(),
            actual: s.workspace_id().clone(),
        });
    }
    s.validate().map_err(ServerError::InvalidAccessState)
}
fn validate_request(w: &WorkspaceId, r: &DeviceJoinRequest) -> ServerResult<()> {
    if &r.workspace_id != w {
        return Err(ServerError::AccessStateWorkspaceMismatch {
            expected: w.clone(),
            actual: r.workspace_id.clone(),
        });
    }
    Ok(())
}
fn deserialize_request(w: &WorkspaceId, b: &[u8]) -> ServerResult<DeviceJoinRequest> {
    let r = serde_json::from_slice(b)
        .map_err(|e| ServerError::CorruptDatabase(format!("invalid stored join request: {e}")))?;
    validate_request(w, &r)?;
    Ok(r)
}
fn to_i64(v: u64, f: &'static str) -> ServerResult<i64> {
    i64::try_from(v).map_err(|_| ServerError::IntegerOutOfRange(f))
}
fn to_u64(v: i64, f: &'static str) -> ServerResult<u64> {
    u64::try_from(v).map_err(|_| ServerError::CorruptDatabase(format!("negative {f}")))
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    async fn raw_pool(path: &Path) -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(path)
                    .create_if_missing(true),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn open_recovers_completed_v1_schema_without_migration_record() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("workspace.db");
        let pool = raw_pool(&path).await;
        sqlx::raw_sql(SCHEMA).execute(&pool).await.unwrap();
        pool.close().await;

        let db = WorkspaceDb::open(&path).await.unwrap();
        let metadata = StoredObjectMetadata {
            object_id: "object-id".into(),
            kind: StoredObjectKind::Blob,
            encrypted_size: 42,
        };
        assert_eq!(
            db.insert_object_if_absent(&metadata).await.unwrap(),
            ObjectInsertResult::Inserted
        );
        assert_eq!(
            db.get_object_metadata(StoredObjectKind::Blob, "object-id")
                .await
                .unwrap(),
            Some(metadata)
        );
        drop(db);
        let pool = raw_pool(&path).await;
        let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
        pool.close().await;
    }

    #[tokio::test]
    async fn open_rejects_unrelated_user_tables() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("workspace.db");
        let pool = raw_pool(&path).await;
        sqlx::query("CREATE TABLE unrelated (value INTEGER)")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        assert!(matches!(
            WorkspaceDb::open(&path).await,
            Err(ServerError::CorruptDatabase(_))
        ));
    }

    #[tokio::test]
    async fn open_rejects_invalid_migration_versions() {
        for versions in [&[0][..], &[-1][..], &[0, 1][..]] {
            let directory = tempdir().unwrap();
            let path = directory.path().join("workspace.db");
            let pool = raw_pool(&path).await;
            sqlx::raw_sql(SCHEMA).execute(&pool).await.unwrap();
            for version in versions {
                sqlx::query("INSERT INTO schema_migrations VALUES(?1, 0)")
                    .bind(version)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
            pool.close().await;

            assert!(matches!(
                WorkspaceDb::open(&path).await,
                Err(ServerError::CorruptDatabase(_))
            ));
        }
    }

    #[tokio::test]
    async fn open_rejects_incomplete_v1_schema() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("workspace.db");
        let pool = raw_pool(&path).await;
        sqlx::raw_sql(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);\
             CREATE TABLE objects (object_id TEXT PRIMARY KEY);\
             INSERT INTO schema_migrations VALUES(1, 0);",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        assert!(matches!(
            WorkspaceDb::open(&path).await,
            Err(ServerError::CorruptDatabase(_))
        ));
    }

    #[tokio::test]
    async fn open_rejects_newer_schema_version() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("workspace.db");
        let pool = raw_pool(&path).await;
        sqlx::raw_sql(SCHEMA).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO schema_migrations VALUES(?1, 0)")
            .bind(LATEST_SCHEMA_VERSION + 1)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        assert!(matches!(
            WorkspaceDb::open(&path).await,
            Err(ServerError::UnsupportedSchemaVersion {
                found: 2,
                supported: LATEST_SCHEMA_VERSION,
            })
        ));
    }

    #[tokio::test]
    async fn open_rejects_missing_or_unexpected_indexes() {
        for statement in [
            "DROP INDEX idx_objects_id",
            "CREATE INDEX unexpected_objects_index ON objects(created_at)",
        ] {
            let directory = tempdir().unwrap();
            let path = directory.path().join("workspace.db");
            let pool = raw_pool(&path).await;
            sqlx::raw_sql(SCHEMA).execute(&pool).await.unwrap();
            sqlx::query(statement).execute(&pool).await.unwrap();
            pool.close().await;

            assert!(matches!(
                WorkspaceDb::open(&path).await,
                Err(ServerError::CorruptDatabase(_))
            ));
        }
    }

    #[tokio::test]
    async fn object_lookup_uses_the_typed_primary_key_index() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("workspace.db");
        let db = WorkspaceDb::open(&path).await.unwrap();

        let (_, _, _, plan): (i64, i64, i64, String) = sqlx::query_as(
            "EXPLAIN QUERY PLAN SELECT object_kind,hash_algorithm,encrypted_size FROM objects WHERE object_kind=?1 AND object_id=?2",
        )
        .bind(StoredObjectKind::Blob.as_str())
        .bind("blob_example")
        .fetch_one(&db.pool)
        .await
        .unwrap();

        assert!(
            plan.contains("SEARCH objects USING INDEX sqlite_autoindex_objects_1"),
            "typed object lookup must use the composite primary-key index, got: {plan}"
        );
    }

    #[tokio::test]
    async fn open_rejects_views_and_triggers() {
        for statement in [
            "CREATE VIEW object_view AS SELECT object_id FROM objects",
            "CREATE TRIGGER object_trigger AFTER INSERT ON objects BEGIN SELECT 1; END",
        ] {
            let directory = tempdir().unwrap();
            let path = directory.path().join("workspace.db");
            let pool = raw_pool(&path).await;
            sqlx::raw_sql(SCHEMA).execute(&pool).await.unwrap();
            sqlx::query(statement).execute(&pool).await.unwrap();
            pool.close().await;

            assert!(matches!(
                WorkspaceDb::open(&path).await,
                Err(ServerError::CorruptDatabase(_))
            ));
        }
    }

    #[tokio::test]
    async fn open_rejects_missing_v1_semantic_constraints() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("workspace.db");
        let pool = raw_pool(&path).await;
        sqlx::raw_sql(SCHEMA).execute(&pool).await.unwrap();
        sqlx::raw_sql(
            "DROP TABLE workspace_head;\
             CREATE TABLE workspace_head (\
                 singleton INTEGER PRIMARY KEY,\
                 manifest_id TEXT NULL,\
                 revision INTEGER NOT NULL,\
                 updated_by TEXT NULL,\
                 updated_at INTEGER NULL\
             );",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        assert!(matches!(
            WorkspaceDb::open(&path).await,
            Err(ServerError::CorruptDatabase(_))
        ));
    }
}
