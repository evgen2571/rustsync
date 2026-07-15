use super::{
    BoxStorageFuture, HeadUpdateResult, JoinRequestPutResult, PutResult, Storage, atomic, paths,
    sqlite::{ObjectInsertResult, StoredObjectKind, StoredObjectMetadata, WorkspaceDb},
};
use crate::error::{ServerError, ServerResult};
use fs4::fs_std::FileExt;
use rustsync_protocol::{
    AccessState, BlobId, DeviceId, DeviceJoinRequest, JoinRequestId, KeyId, ManifestId,
    WorkspaceHead, WorkspaceId,
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
        {
            let dbs = self.databases.lock().await;
            if let Some(db) = dbs.get(workspace) {
                return Ok(db.clone());
            }
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
        let mut dbs = self.databases.lock().await;
        Ok(dbs.entry(workspace.clone()).or_insert(db).clone())
    }

    async fn get_existing(
        &self,
        workspace: &WorkspaceId,
    ) -> ServerResult<Option<Arc<WorkspaceDb>>> {
        {
            let dbs = self.databases.lock().await;
            if let Some(db) = dbs.get(workspace) {
                return Ok(Some(db.clone()));
            }
        }

        let path = paths::workspace_state_path(&self.root, workspace.as_str());
        if !fs::try_exists(&path).await? {
            return Ok(None);
        }

        self.get_or_open(workspace).await.map(Some)
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
    pub async fn put_key_envelope(
        &self,
        w: &WorkspaceId,
        key_id: &KeyId,
        recipient_device_id: &DeviceId,
        bytes: &[u8],
    ) -> ServerResult<PutResult> {
        self.inner
            .databases
            .get_or_open(w)
            .await?
            .put_key_envelope(key_id, recipient_device_id, bytes)
            .await
    }
    pub async fn get_key_envelope(
        &self,
        w: &WorkspaceId,
        key_id: &KeyId,
        recipient_device_id: &DeviceId,
    ) -> ServerResult<Option<Vec<u8>>> {
        self.inner
            .databases
            .get_or_open(w)
            .await?
            .get_key_envelope(key_id, recipient_device_id)
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
        match self.inner.databases.get_existing(w).await? {
            Some(db) => db.get_access_state(w).await,
            None => Ok(AccessState::empty(w.clone())),
        }
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
impl Storage for IndexedFsStorage {
    fn put_blob<'a>(
        &'a self,
        w: &'a WorkspaceId,
        id: &'a BlobId,
        b: &'a [u8],
    ) -> BoxStorageFuture<'a, PutResult> {
        Box::pin(Self::put_blob(self, w, id, b))
    }
    fn get_blob<'a>(&'a self, w: &'a WorkspaceId, id: &'a BlobId) -> BoxStorageFuture<'a, Vec<u8>> {
        Box::pin(Self::get_blob(self, w, id))
    }
    fn blob_exists<'a>(&'a self, w: &'a WorkspaceId, id: &'a BlobId) -> BoxStorageFuture<'a, bool> {
        Box::pin(Self::blob_exists(self, w, id))
    }
    fn put_manifest<'a>(
        &'a self,
        w: &'a WorkspaceId,
        id: &'a ManifestId,
        b: &'a [u8],
    ) -> BoxStorageFuture<'a, PutResult> {
        Box::pin(Self::put_manifest(self, w, id, b))
    }
    fn get_manifest<'a>(
        &'a self,
        w: &'a WorkspaceId,
        id: &'a ManifestId,
    ) -> BoxStorageFuture<'a, Vec<u8>> {
        Box::pin(Self::get_manifest(self, w, id))
    }
    fn manifest_exists<'a>(
        &'a self,
        w: &'a WorkspaceId,
        id: &'a ManifestId,
    ) -> BoxStorageFuture<'a, bool> {
        Box::pin(Self::manifest_exists(self, w, id))
    }
    fn put_key_envelope<'a>(
        &'a self,
        w: &'a WorkspaceId,
        key_id: &'a KeyId,
        recipient_device_id: &'a DeviceId,
        bytes: &'a [u8],
    ) -> BoxStorageFuture<'a, PutResult> {
        Box::pin(Self::put_key_envelope(
            self,
            w,
            key_id,
            recipient_device_id,
            bytes,
        ))
    }
    fn get_key_envelope<'a>(
        &'a self,
        w: &'a WorkspaceId,
        key_id: &'a KeyId,
        recipient_device_id: &'a DeviceId,
    ) -> BoxStorageFuture<'a, Option<Vec<u8>>> {
        Box::pin(Self::get_key_envelope(self, w, key_id, recipient_device_id))
    }
    fn get_head<'a>(&'a self, w: &'a WorkspaceId) -> BoxStorageFuture<'a, WorkspaceHead> {
        Box::pin(Self::get_head(self, w))
    }
    fn update_head<'a>(
        &'a self,
        w: &'a WorkspaceId,
        e: u64,
        m: ManifestId,
        d: Option<DeviceId>,
    ) -> BoxStorageFuture<'a, (HeadUpdateResult, WorkspaceHead)> {
        Box::pin(Self::update_head(self, w, e, m, d))
    }
    fn get_access_state<'a>(&'a self, w: &'a WorkspaceId) -> BoxStorageFuture<'a, AccessState> {
        Box::pin(Self::get_access_state(self, w))
    }
    fn create_access_state<'a>(
        &'a self,
        w: &'a WorkspaceId,
        s: &'a AccessState,
    ) -> BoxStorageFuture<'a, ()> {
        Box::pin(Self::create_access_state(self, w, s))
    }
    fn save_access_state<'a>(
        &'a self,
        w: &'a WorkspaceId,
        s: &'a AccessState,
    ) -> BoxStorageFuture<'a, ()> {
        Box::pin(Self::save_access_state(self, w, s))
    }
    fn submit_join_request<'a>(
        &'a self,
        w: &'a WorkspaceId,
        r: &'a DeviceJoinRequest,
    ) -> BoxStorageFuture<'a, JoinRequestPutResult> {
        Box::pin(Self::submit_join_request(self, w, r))
    }
    fn list_join_requests<'a>(
        &'a self,
        w: &'a WorkspaceId,
    ) -> BoxStorageFuture<'a, Vec<DeviceJoinRequest>> {
        Box::pin(Self::list_join_requests(self, w))
    }
    fn get_join_request<'a>(
        &'a self,
        w: &'a WorkspaceId,
        id: &'a JoinRequestId,
    ) -> BoxStorageFuture<'a, Option<DeviceJoinRequest>> {
        Box::pin(Self::get_join_request(self, w, id))
    }
    fn remove_join_request<'a>(
        &'a self,
        w: &'a WorkspaceId,
        id: &'a JoinRequestId,
    ) -> BoxStorageFuture<'a, ()> {
        Box::pin(Self::remove_join_request(self, w, id))
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
