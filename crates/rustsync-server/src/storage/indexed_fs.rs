use super::{
    HeadUpdateResult, JoinRequestPutResult, PutResult, Storage, atomic, paths,
    sqlite::{ObjectInsertResult, StoredObjectKind, StoredObjectMetadata, WorkspaceDb},
};
use crate::error::{ServerError, ServerResult};
use async_trait::async_trait;
use fs4::fs_std::FileExt;
use rustsync_protocol::{
    AccessState, BlobId, DeviceId, DeviceJoinRequest, JoinRequestId, ManifestId, WorkspaceHead,
    WorkspaceId,
};
use std::{
    collections::HashMap,
    fs::{self as std_fs, File, OpenOptions},
    path::PathBuf,
    sync::Arc,
};
use tokio::{fs, sync::Mutex};

#[derive(Debug)]
pub(crate) struct WorkspaceDbRegistry {
    root: PathBuf,
    databases: Mutex<HashMap<WorkspaceId, Arc<WorkspaceDb>>>,
}
impl WorkspaceDbRegistry {
    async fn get_or_open(&self, workspace: &WorkspaceId) -> ServerResult<Arc<WorkspaceDb>> {
        let mut dbs = self.databases.lock().await;
        if let Some(db) = dbs.get(workspace) {
            return Ok(db.clone());
        }
        let path = paths::workspace_state_path(&self.root, workspace.as_str());
        let db = Arc::new(
            WorkspaceDb::open(&path)
                .await
                .map_err(|source| match source {
                    ServerError::Database(source) => ServerError::WorkspaceDatabase {
                        workspace: workspace.clone(),
                        path,
                        operation: "open",
                        source: Box::new(ServerError::Database(source)),
                    },
                    source => source,
                })?,
        ); // Intentionally unbounded pending an operational cache policy.
        dbs.insert(workspace.clone(), db.clone());
        Ok(db)
    }
}
#[derive(Debug, Clone)]
pub struct IndexedFsStorage {
    inner: Arc<IndexedFsStorageInner>,
}

#[derive(Debug)]
struct IndexedFsStorageInner {
    root: PathBuf,
    databases: Arc<WorkspaceDbRegistry>,
    put_lock: Arc<Mutex<()>>,
    _root_lock: File,
}

impl IndexedFsStorage {
    pub async fn open(root: PathBuf) -> ServerResult<Self> {
        match std_fs::metadata(&root) {
            Ok(metadata) if !metadata.is_dir() => {
                return Err(ServerError::StorageRootNotDirectory { root });
            }
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ServerError::StorageRoot {
                    root,
                    operation: "inspect",
                    source,
                });
            }
        }
        std_fs::create_dir_all(&root).map_err(|source| ServerError::StorageRoot {
            root: root.clone(),
            operation: "create",
            source,
        })?;
        let canonical_root =
            std_fs::canonicalize(&root).map_err(|source| ServerError::StorageRoot {
                root: root.clone(),
                operation: "resolve",
                source,
            })?;
        let metadata =
            std_fs::metadata(&canonical_root).map_err(|source| ServerError::StorageRoot {
                root: canonical_root.clone(),
                operation: "inspect",
                source,
            })?;
        if !metadata.is_dir() {
            return Err(ServerError::StorageRootNotDirectory {
                root: canonical_root,
            });
        }

        let root_lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(canonical_root.join(".rustsync-server.lock"))
            .map_err(|source| ServerError::StorageRoot {
                root: canonical_root.clone(),
                operation: "open lock file for",
                source,
            })?;
        match root_lock.try_lock_exclusive() {
            Ok(true) => {}
            Ok(false) => {
                return Err(ServerError::StorageRootAlreadyInUse {
                    root: canonical_root,
                });
            }
            Err(source) => {
                return Err(ServerError::StorageRoot {
                    root: canonical_root,
                    operation: "lock",
                    source,
                });
            }
        }

        Ok(Self {
            inner: Arc::new(IndexedFsStorageInner {
                databases: Arc::new(WorkspaceDbRegistry {
                    root: canonical_root.clone(),
                    databases: Mutex::new(HashMap::new()),
                }),
                root: canonical_root,
                put_lock: Arc::new(Mutex::new(())),
                _root_lock: root_lock,
            }),
        })
    }
    fn object_path(&self, w: &WorkspaceId, k: StoredObjectKind, id: &str) -> PathBuf {
        match k {
            StoredObjectKind::Blob => paths::blob_path(&self.inner.root, w.as_str(), id),
            StoredObjectKind::Manifest => paths::manifest_path(&self.inner.root, w.as_str(), id),
        }
    }
    async fn put_object(
        &self,
        w: &WorkspaceId,
        k: StoredObjectKind,
        id: &str,
        bytes: &[u8],
    ) -> ServerResult<PutResult> {
        let _g = self.inner.put_lock.lock().await;
        let p = self.object_path(w, k, id);
        let canonical = match fs::read(&p).await {
            Ok(existing) => existing,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Re-read regardless of whether we published or lost a race:
                // only canonical bytes may be catalogued.
                let _ = atomic::write_new(&p, bytes).await?;
                fs::read(&p).await?
            }
            Err(e) => return Err(ServerError::Storage(e)),
        };
        if canonical != bytes || !matches_id(k, id, &canonical) {
            return Err(ServerError::StorageCorruption(format!(
                "canonical {} `{id}` does not exactly match submitted bytes and content ID",
                k.as_str()
            )));
        }
        let db = self.inner.databases.get_or_open(w).await?;
        let row = db
            .insert_object_if_absent(&StoredObjectMetadata {
                object_id: id.into(),
                kind: k,
                encrypted_size: canonical.len() as u64,
            })
            .await?;
        Ok(if row == ObjectInsertResult::Inserted {
            PutResult::Created
        } else {
            PutResult::AlreadyExists
        })
    }
    async fn get_object(
        &self,
        w: &WorkspaceId,
        k: StoredObjectKind,
        id: &str,
        missing: ServerError,
    ) -> ServerResult<Vec<u8>> {
        let p = self.object_path(w, k, id);
        let b = match fs::read(&p).await {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.report_missing_catalog_row(w, k, id).await?;
                return Err(missing);
            }
            Err(e) => return Err(ServerError::Storage(e)),
        };
        self.verify_backfill(w, k, id, &b).await?;
        Ok(b)
    }
    async fn exists_object(
        &self,
        w: &WorkspaceId,
        k: StoredObjectKind,
        id: &str,
    ) -> ServerResult<bool> {
        let b = match fs::read(self.object_path(w, k, id)).await {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.report_missing_catalog_row(w, k, id).await?;
                return Ok(false);
            }
            Err(e) => return Err(ServerError::Storage(e)),
        };
        self.verify_backfill(w, k, id, &b).await?;
        Ok(true)
    }
    async fn verify_backfill(
        &self,
        w: &WorkspaceId,
        k: StoredObjectKind,
        id: &str,
        b: &[u8],
    ) -> ServerResult<()> {
        if !matches_id(k, id, b) {
            return Err(ServerError::StorageCorruption(format!(
                "stored {} `{id}` does not match its content ID",
                k.as_str()
            )));
        }
        self.inner
            .databases
            .get_or_open(w)
            .await?
            .insert_object_if_absent(&StoredObjectMetadata {
                object_id: id.into(),
                kind: k,
                encrypted_size: b.len() as u64,
            })
            .await?;
        Ok(())
    }
    async fn report_missing_catalog_row(
        &self,
        w: &WorkspaceId,
        k: StoredObjectKind,
        id: &str,
    ) -> ServerResult<()> {
        if self
            .inner
            .databases
            .get_or_open(w)
            .await?
            .get_object_metadata(k, id)
            .await?
            .is_some()
        {
            return Err(ServerError::StorageCorruption(format!(
                "catalog metadata for {} `{id}` exists but its canonical file is missing",
                k.as_str()
            )));
        }
        Ok(())
    }
    pub async fn put_blob(
        &self,
        w: &WorkspaceId,
        id: &BlobId,
        b: &[u8],
    ) -> ServerResult<PutResult> {
        if &BlobId::from_content(b) != id {
            return Err(ServerError::ObjectHashMismatch);
        }
        self.put_object(w, StoredObjectKind::Blob, id.as_str(), b)
            .await
    }
    pub async fn get_blob(&self, w: &WorkspaceId, id: &BlobId) -> ServerResult<Vec<u8>> {
        self.get_object(
            w,
            StoredObjectKind::Blob,
            id.as_str(),
            ServerError::BlobNotFound,
        )
        .await
    }
    pub async fn blob_exists(&self, w: &WorkspaceId, id: &BlobId) -> ServerResult<bool> {
        self.exists_object(w, StoredObjectKind::Blob, id.as_str())
            .await
    }
    pub async fn put_manifest(
        &self,
        w: &WorkspaceId,
        id: &ManifestId,
        b: &[u8],
    ) -> ServerResult<PutResult> {
        if &ManifestId::from_content(b) != id {
            return Err(ServerError::ObjectHashMismatch);
        }
        self.put_object(w, StoredObjectKind::Manifest, id.as_str(), b)
            .await
    }
    pub async fn get_manifest(&self, w: &WorkspaceId, id: &ManifestId) -> ServerResult<Vec<u8>> {
        self.get_object(
            w,
            StoredObjectKind::Manifest,
            id.as_str(),
            ServerError::ManifestNotFound,
        )
        .await
    }
    pub async fn manifest_exists(&self, w: &WorkspaceId, id: &ManifestId) -> ServerResult<bool> {
        self.exists_object(w, StoredObjectKind::Manifest, id.as_str())
            .await
    }
    pub async fn get_head(&self, w: &WorkspaceId) -> ServerResult<WorkspaceHead> {
        let head = self
            .inner
            .databases
            .get_or_open(w)
            .await?
            .get_head(w)
            .await?;
        if let Some(manifest_id) = head.manifest_id.as_ref() {
            self.get_manifest(w, manifest_id).await?;
        }
        Ok(head)
    }
    pub async fn update_head(
        &self,
        w: &WorkspaceId,
        e: u64,
        m: ManifestId,
        d: Option<DeviceId>,
    ) -> ServerResult<(HeadUpdateResult, WorkspaceHead)> {
        if !self.manifest_exists(w, &m).await? {
            return Err(ServerError::ManifestNotFound);
        }
        self.inner
            .databases
            .get_or_open(w)
            .await?
            .update_head(w, e, m, d)
            .await
    }
    pub async fn get_access_state(&self, w: &WorkspaceId) -> ServerResult<AccessState> {
        self.inner
            .databases
            .get_or_open(w)
            .await?
            .get_access_state(w)
            .await
    }
    pub async fn create_access_state(&self, w: &WorkspaceId, s: &AccessState) -> ServerResult<()> {
        self.inner
            .databases
            .get_or_open(w)
            .await?
            .create_access_state(w, s)
            .await
    }
    pub async fn save_access_state(&self, w: &WorkspaceId, s: &AccessState) -> ServerResult<()> {
        self.inner
            .databases
            .get_or_open(w)
            .await?
            .save_access_state(w, s)
            .await
    }
    pub async fn submit_join_request(
        &self,
        w: &WorkspaceId,
        r: &DeviceJoinRequest,
    ) -> ServerResult<JoinRequestPutResult> {
        self.inner
            .databases
            .get_or_open(w)
            .await?
            .submit_join_request(w, r)
            .await
    }
    pub async fn list_join_requests(
        &self,
        w: &WorkspaceId,
    ) -> ServerResult<Vec<DeviceJoinRequest>> {
        self.inner
            .databases
            .get_or_open(w)
            .await?
            .list_join_requests(w)
            .await
    }
    pub async fn get_join_request(
        &self,
        w: &WorkspaceId,
        id: &JoinRequestId,
    ) -> ServerResult<Option<DeviceJoinRequest>> {
        self.inner
            .databases
            .get_or_open(w)
            .await?
            .get_join_request(w, id)
            .await
    }
    pub async fn remove_join_request(
        &self,
        w: &WorkspaceId,
        id: &JoinRequestId,
    ) -> ServerResult<()> {
        self.inner
            .databases
            .get_or_open(w)
            .await?
            .remove_join_request(w, id)
            .await
    }
}
fn matches_id(k: StoredObjectKind, id: &str, b: &[u8]) -> bool {
    match k {
        StoredObjectKind::Blob => BlobId::from_content(b).as_str() == id,
        StoredObjectKind::Manifest => ManifestId::from_content(b).as_str() == id,
    }
}
#[async_trait]
impl Storage for IndexedFsStorage {
    async fn put_blob(&self, w: &WorkspaceId, id: &BlobId, b: &[u8]) -> ServerResult<PutResult> {
        let (this, w, id, b) = (self.clone(), w.clone(), id.clone(), b.to_vec());
        IndexedFsStorage::put_blob(&this, &w, &id, &b).await
    }
    async fn get_blob(&self, w: &WorkspaceId, id: &BlobId) -> ServerResult<Vec<u8>> {
        let (this, w, id) = (self.clone(), w.clone(), id.clone());
        IndexedFsStorage::get_blob(&this, &w, &id).await
    }
    async fn blob_exists(&self, w: &WorkspaceId, id: &BlobId) -> ServerResult<bool> {
        let (this, w, id) = (self.clone(), w.clone(), id.clone());
        IndexedFsStorage::blob_exists(&this, &w, &id).await
    }
    async fn put_manifest(
        &self,
        w: &WorkspaceId,
        id: &ManifestId,
        b: &[u8],
    ) -> ServerResult<PutResult> {
        let (this, w, id, b) = (self.clone(), w.clone(), id.clone(), b.to_vec());
        IndexedFsStorage::put_manifest(&this, &w, &id, &b).await
    }
    async fn get_manifest(&self, w: &WorkspaceId, id: &ManifestId) -> ServerResult<Vec<u8>> {
        let (this, w, id) = (self.clone(), w.clone(), id.clone());
        IndexedFsStorage::get_manifest(&this, &w, &id).await
    }
    async fn manifest_exists(&self, w: &WorkspaceId, id: &ManifestId) -> ServerResult<bool> {
        let (this, w, id) = (self.clone(), w.clone(), id.clone());
        IndexedFsStorage::manifest_exists(&this, &w, &id).await
    }
    async fn get_head(&self, w: &WorkspaceId) -> ServerResult<WorkspaceHead> {
        let (this, w) = (self.clone(), w.clone());
        IndexedFsStorage::get_head(&this, &w).await
    }
    async fn update_head(
        &self,
        w: &WorkspaceId,
        e: u64,
        m: ManifestId,
        d: Option<DeviceId>,
    ) -> ServerResult<(HeadUpdateResult, WorkspaceHead)> {
        let (this, w) = (self.clone(), w.clone());
        IndexedFsStorage::update_head(&this, &w, e, m, d).await
    }
    async fn get_access_state(&self, w: &WorkspaceId) -> ServerResult<AccessState> {
        let (this, w) = (self.clone(), w.clone());
        IndexedFsStorage::get_access_state(&this, &w).await
    }
    async fn create_access_state(&self, w: &WorkspaceId, s: &AccessState) -> ServerResult<()> {
        let (this, w, s) = (self.clone(), w.clone(), s.clone());
        IndexedFsStorage::create_access_state(&this, &w, &s).await
    }
    async fn save_access_state(&self, w: &WorkspaceId, s: &AccessState) -> ServerResult<()> {
        let (this, w, s) = (self.clone(), w.clone(), s.clone());
        IndexedFsStorage::save_access_state(&this, &w, &s).await
    }
    async fn submit_join_request(
        &self,
        w: &WorkspaceId,
        r: &DeviceJoinRequest,
    ) -> ServerResult<JoinRequestPutResult> {
        let (this, w, r) = (self.clone(), w.clone(), r.clone());
        IndexedFsStorage::submit_join_request(&this, &w, &r).await
    }
    async fn list_join_requests(&self, w: &WorkspaceId) -> ServerResult<Vec<DeviceJoinRequest>> {
        let (this, w) = (self.clone(), w.clone());
        IndexedFsStorage::list_join_requests(&this, &w).await
    }
    async fn get_join_request(
        &self,
        w: &WorkspaceId,
        id: &JoinRequestId,
    ) -> ServerResult<Option<DeviceJoinRequest>> {
        let (this, w, id) = (self.clone(), w.clone(), id.clone());
        IndexedFsStorage::get_join_request(&this, &w, &id).await
    }
    async fn remove_join_request(&self, w: &WorkspaceId, id: &JoinRequestId) -> ServerResult<()> {
        let (this, w, id) = (self.clone(), w.clone(), id.clone());
        IndexedFsStorage::remove_join_request(&this, &w, &id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_revalidates_canonical_file_after_losing_publication_race() {
        let temp = tempfile::tempdir().expect("create temporary storage root");
        let workspace = WorkspaceId::parse("workspace_publication_race").expect("valid workspace");
        let storage = IndexedFsStorage::open(temp.path().to_path_buf())
            .await
            .expect("open storage");
        let submitted = b"submitted immutable object bytes";
        let blob = BlobId::from_content(submitted);

        atomic::set_before_publish_hook(|path| {
            std::fs::write(path, b"competing canonical bytes")
                .expect("publish competing canonical bytes");
        });

        assert!(matches!(
            storage.put_blob(&workspace, &blob, submitted).await,
            Err(ServerError::StorageCorruption(_))
        ));
    }
}
