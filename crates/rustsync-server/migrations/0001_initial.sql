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
