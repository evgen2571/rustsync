use super::{
    BoxStorageFuture, HeadUpdateResult, JoinRequestPutResult, PutResult, Storage, atomic, paths,
    sqlite::{StoredObjectKind, StoredObjectMetadata, WorkspaceDb},
};
use crate::error::{ServerError, ServerResult};
use rustsync_protocol::{
    AccessState, BlobId, DeviceId, DeviceJoinRequest, JoinRequestId, ManifestId, WorkspaceHead,
    WorkspaceId,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
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
        let db = Arc::new(
            WorkspaceDb::open(&paths::workspace_state_path(&self.root, workspace.as_str())).await?,
        ); // Intentionally unbounded pending an operational cache policy.
        dbs.insert(workspace.clone(), db.clone());
        Ok(db)
    }
}
#[derive(Debug, Clone)]
pub struct IndexedFsStorage {
    root: PathBuf,
    databases: Arc<WorkspaceDbRegistry>,
    put_lock: Arc<Mutex<()>>,
}
impl IndexedFsStorage {
    pub async fn open(root: PathBuf) -> ServerResult<Self> {
        Ok(Self {
            databases: Arc::new(WorkspaceDbRegistry {
                root: root.clone(),
                databases: Mutex::new(HashMap::new()),
            }),
            root,
            put_lock: Arc::new(Mutex::new(())),
        })
    }
    fn object_path(&self, w: &WorkspaceId, k: StoredObjectKind, id: &str) -> PathBuf {
        match k {
            StoredObjectKind::Blob => paths::blob_path(&self.root, w.as_str(), id),
            StoredObjectKind::Manifest => paths::manifest_path(&self.root, w.as_str(), id),
        }
    }
    async fn put_object(
        &self,
        w: &WorkspaceId,
        k: StoredObjectKind,
        id: &str,
        bytes: &[u8],
    ) -> ServerResult<PutResult> {
        let _g = self.put_lock.lock().await;
        let p = self.object_path(w, k, id);
        let created = match fs::read(&p).await {
            Ok(existing) => {
                if existing != bytes || !matches_id(k, id, &existing) {
                    return Err(ServerError::StorageCorruption(format!(
                        "existing {} `{id}` does not match its content ID",
                        k.as_str()
                    )));
                }
                false
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                atomic::write_new(&p, bytes).await?
            }
            Err(e) => return Err(ServerError::Storage(e)),
        };
        let db = self.databases.get_or_open(w).await?;
        let row = db
            .insert_object_if_absent(&StoredObjectMetadata {
                object_id: id.into(),
                kind: k,
                encrypted_size: bytes.len() as u64,
            })
            .await?;
        Ok(if created || row {
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
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(missing),
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
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
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
        self.databases
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
        self.databases.get_or_open(w).await?.get_head(w).await
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
        self.databases
            .get_or_open(w)
            .await?
            .update_head(w, e, m, d)
            .await
    }
    pub async fn get_access_state(&self, w: &WorkspaceId) -> ServerResult<AccessState> {
        self.databases
            .get_or_open(w)
            .await?
            .get_access_state(w)
            .await
    }
    pub async fn create_access_state(&self, w: &WorkspaceId, s: &AccessState) -> ServerResult<()> {
        self.databases
            .get_or_open(w)
            .await?
            .create_access_state(w, s)
            .await
    }
    pub async fn save_access_state(&self, w: &WorkspaceId, s: &AccessState) -> ServerResult<()> {
        self.databases
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
        self.databases
            .get_or_open(w)
            .await?
            .submit_join_request(w, r)
            .await
    }
    pub async fn list_join_requests(
        &self,
        w: &WorkspaceId,
    ) -> ServerResult<Vec<DeviceJoinRequest>> {
        self.databases
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
        self.databases
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
        self.databases
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
