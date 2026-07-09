use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use rustsync_protocol::{
    AccessState, BlobId, DeviceId, DeviceJoinRequest, JoinRequestId, ManifestId, WorkspaceHead,
    WorkspaceId,
};
use tokio::{fs, sync::Mutex};

use super::{
    BoxStorageFuture, FsStorage, HeadUpdateResult, JoinRequestPutResult, PutResult, Storage,
    atomic, paths,
    sqlite::{StorageDb, StoredObjectKind, StoredObjectMetadata},
};
use crate::error::{ServerError, ServerResult};

#[derive(Debug, Clone)]
pub struct IndexedFsStorage {
    root: PathBuf,
    db: StorageDb,
    legacy: FsStorage,
    put_lock: Arc<Mutex<()>>,
}

impl IndexedFsStorage {
    pub async fn open(root: PathBuf) -> ServerResult<Self> {
        let db = StorageDb::open(&paths::database_path(&root)).await?;
        let legacy = FsStorage::new(root.clone());
        Ok(Self {
            root,
            db,
            legacy,
            put_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn put_blob(
        &self,
        workspace_id: &WorkspaceId,
        blob_id: &BlobId,
        bytes: &[u8],
    ) -> ServerResult<PutResult> {
        let actual_blob_id = BlobId::from_content(bytes);
        if &actual_blob_id != blob_id {
            return Err(ServerError::ObjectHashMismatch);
        }

        self.put_object(
            workspace_id.as_str(),
            blob_id.as_str(),
            StoredObjectKind::Blob,
            bytes,
            paths::blob_path(&self.root, workspace_id.as_str(), blob_id.as_str()),
        )
        .await
    }

    pub async fn get_blob(
        &self,
        workspace_id: &WorkspaceId,
        blob_id: &BlobId,
    ) -> ServerResult<Vec<u8>> {
        self.get_object(
            workspace_id.as_str(),
            blob_id.as_str(),
            StoredObjectKind::Blob,
            ServerError::BlobNotFound,
        )
        .await
    }

    pub async fn blob_exists(
        &self,
        workspace_id: &WorkspaceId,
        blob_id: &BlobId,
    ) -> ServerResult<bool> {
        self.object_exists(
            workspace_id.as_str(),
            blob_id.as_str(),
            StoredObjectKind::Blob,
        )
        .await
    }

    pub async fn put_manifest(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
        bytes: &[u8],
    ) -> ServerResult<PutResult> {
        let actual_manifest_id = ManifestId::from_content(bytes);
        if &actual_manifest_id != manifest_id {
            return Err(ServerError::ObjectHashMismatch);
        }

        self.put_object(
            workspace_id.as_str(),
            manifest_id.as_str(),
            StoredObjectKind::Manifest,
            bytes,
            paths::manifest_path(&self.root, workspace_id.as_str(), manifest_id.as_str()),
        )
        .await
    }

    pub async fn get_manifest(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
    ) -> ServerResult<Vec<u8>> {
        self.get_object(
            workspace_id.as_str(),
            manifest_id.as_str(),
            StoredObjectKind::Manifest,
            ServerError::ManifestNotFound,
        )
        .await
    }

    pub async fn manifest_exists(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
    ) -> ServerResult<bool> {
        self.object_exists(
            workspace_id.as_str(),
            manifest_id.as_str(),
            StoredObjectKind::Manifest,
        )
        .await
    }

    pub async fn get_head(&self, workspace_id: &WorkspaceId) -> ServerResult<WorkspaceHead> {
        self.legacy.get_head(workspace_id).await
    }

    pub async fn update_head(
        &self,
        workspace_id: &WorkspaceId,
        expected_revision: u64,
        manifest_id: ManifestId,
        updated_by: Option<DeviceId>,
    ) -> ServerResult<(HeadUpdateResult, WorkspaceHead)> {
        self.legacy
            .update_head(workspace_id, expected_revision, manifest_id, updated_by)
            .await
    }

    pub async fn get_access_state(&self, workspace_id: &WorkspaceId) -> ServerResult<AccessState> {
        self.legacy.get_access_state(workspace_id).await
    }

    pub async fn create_access_state(
        &self,
        workspace_id: &WorkspaceId,
        state: &AccessState,
    ) -> ServerResult<()> {
        self.legacy.create_access_state(workspace_id, state).await
    }

    pub async fn save_access_state(
        &self,
        workspace_id: &WorkspaceId,
        state: &AccessState,
    ) -> ServerResult<()> {
        self.legacy.save_access_state(workspace_id, state).await
    }

    pub async fn submit_join_request(
        &self,
        workspace_id: &WorkspaceId,
        request: &DeviceJoinRequest,
    ) -> ServerResult<JoinRequestPutResult> {
        self.legacy.submit_join_request(workspace_id, request).await
    }

    pub async fn list_join_requests(
        &self,
        workspace_id: &WorkspaceId,
    ) -> ServerResult<Vec<DeviceJoinRequest>> {
        self.legacy.list_join_requests(workspace_id).await
    }

    pub async fn get_join_request(
        &self,
        workspace_id: &WorkspaceId,
        join_request_id: &JoinRequestId,
    ) -> ServerResult<Option<DeviceJoinRequest>> {
        self.legacy
            .get_join_request(workspace_id, join_request_id)
            .await
    }

    pub async fn remove_join_request(
        &self,
        workspace_id: &WorkspaceId,
        join_request_id: &JoinRequestId,
    ) -> ServerResult<()> {
        self.legacy
            .remove_join_request(workspace_id, join_request_id)
            .await
    }

    async fn put_object(
        &self,
        workspace_id: &str,
        object_id: &str,
        kind: StoredObjectKind,
        bytes: &[u8],
        absolute_path: PathBuf,
    ) -> ServerResult<PutResult> {
        let _guard = self.put_lock.lock().await;
        let file_created = write_immutable_object(&absolute_path, bytes).await?;
        let relative_path = relative_storage_path(&self.root, &absolute_path);
        let metadata = StoredObjectMetadata {
            workspace_id: workspace_id.to_string(),
            object_id: object_id.to_string(),
            kind,
            hash_algorithm: "blake3".to_string(),
            encrypted_size: bytes.len() as u64,
            storage_path: relative_path,
            key_id: None,
            encryption_algorithm: None,
            nonce: None,
        };
        let row_inserted = self.db.insert_object_if_absent(&metadata).await?;

        if file_created || row_inserted {
            Ok(PutResult::Created)
        } else {
            Ok(PutResult::AlreadyExists)
        }
    }

    async fn get_object(
        &self,
        workspace_id: &str,
        object_id: &str,
        kind: StoredObjectKind,
        missing_error: ServerError,
    ) -> ServerResult<Vec<u8>> {
        let path = if let Some(storage_path) = self
            .db
            .get_object_storage_path(workspace_id, kind, object_id)
            .await?
        {
            self.root.join(storage_path)
        } else {
            self.object_path(workspace_id, object_id, kind)
        };

        read_object(&path, missing_error).await
    }

    async fn object_exists(
        &self,
        workspace_id: &str,
        object_id: &str,
        kind: StoredObjectKind,
    ) -> ServerResult<bool> {
        let path = if let Some(storage_path) = self
            .db
            .get_object_storage_path(workspace_id, kind, object_id)
            .await?
        {
            self.root.join(storage_path)
        } else {
            self.object_path(workspace_id, object_id, kind)
        };

        fs::try_exists(path).await.map_err(ServerError::Storage)
    }

    fn object_path(&self, workspace_id: &str, object_id: &str, kind: StoredObjectKind) -> PathBuf {
        match kind {
            StoredObjectKind::Blob => paths::blob_path(&self.root, workspace_id, object_id),
            StoredObjectKind::Manifest => paths::manifest_path(&self.root, workspace_id, object_id),
            StoredObjectKind::Chunk => paths::blob_path(&self.root, workspace_id, object_id),
        }
    }
}

impl Storage for IndexedFsStorage {
    fn put_blob<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        blob_id: &'a BlobId,
        bytes: &'a [u8],
    ) -> BoxStorageFuture<'a, PutResult> {
        Box::pin(Self::put_blob(self, workspace_id, blob_id, bytes))
    }

    fn get_blob<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        blob_id: &'a BlobId,
    ) -> BoxStorageFuture<'a, Vec<u8>> {
        Box::pin(Self::get_blob(self, workspace_id, blob_id))
    }

    fn blob_exists<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        blob_id: &'a BlobId,
    ) -> BoxStorageFuture<'a, bool> {
        Box::pin(Self::blob_exists(self, workspace_id, blob_id))
    }

    fn put_manifest<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        manifest_id: &'a ManifestId,
        bytes: &'a [u8],
    ) -> BoxStorageFuture<'a, PutResult> {
        Box::pin(Self::put_manifest(self, workspace_id, manifest_id, bytes))
    }

    fn get_manifest<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        manifest_id: &'a ManifestId,
    ) -> BoxStorageFuture<'a, Vec<u8>> {
        Box::pin(Self::get_manifest(self, workspace_id, manifest_id))
    }

    fn manifest_exists<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        manifest_id: &'a ManifestId,
    ) -> BoxStorageFuture<'a, bool> {
        Box::pin(Self::manifest_exists(self, workspace_id, manifest_id))
    }

    fn get_head<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
    ) -> BoxStorageFuture<'a, WorkspaceHead> {
        Box::pin(Self::get_head(self, workspace_id))
    }

    fn update_head<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        expected_revision: u64,
        manifest_id: ManifestId,
        updated_by: Option<DeviceId>,
    ) -> BoxStorageFuture<'a, (HeadUpdateResult, WorkspaceHead)> {
        Box::pin(Self::update_head(
            self,
            workspace_id,
            expected_revision,
            manifest_id,
            updated_by,
        ))
    }

    fn get_access_state<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
    ) -> BoxStorageFuture<'a, AccessState> {
        Box::pin(Self::get_access_state(self, workspace_id))
    }

    fn create_access_state<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        state: &'a AccessState,
    ) -> BoxStorageFuture<'a, ()> {
        Box::pin(Self::create_access_state(self, workspace_id, state))
    }

    fn save_access_state<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        state: &'a AccessState,
    ) -> BoxStorageFuture<'a, ()> {
        Box::pin(Self::save_access_state(self, workspace_id, state))
    }

    fn submit_join_request<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        request: &'a DeviceJoinRequest,
    ) -> BoxStorageFuture<'a, JoinRequestPutResult> {
        Box::pin(Self::submit_join_request(self, workspace_id, request))
    }

    fn list_join_requests<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
    ) -> BoxStorageFuture<'a, Vec<DeviceJoinRequest>> {
        Box::pin(Self::list_join_requests(self, workspace_id))
    }

    fn get_join_request<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        join_request_id: &'a JoinRequestId,
    ) -> BoxStorageFuture<'a, Option<DeviceJoinRequest>> {
        Box::pin(Self::get_join_request(self, workspace_id, join_request_id))
    }

    fn remove_join_request<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        join_request_id: &'a JoinRequestId,
    ) -> BoxStorageFuture<'a, ()> {
        Box::pin(Self::remove_join_request(
            self,
            workspace_id,
            join_request_id,
        ))
    }
}

async fn write_immutable_object(path: &Path, bytes: &[u8]) -> ServerResult<bool> {
    if fs::try_exists(path).await.map_err(ServerError::Storage)? {
        return Ok(false);
    }

    atomic::write_new(path, bytes).await
}

async fn read_object(path: &Path, missing_error: ServerError) -> ServerResult<Vec<u8>> {
    match fs::read(path).await {
        Ok(bytes) => Ok(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(missing_error),
        Err(err) => Err(ServerError::Storage(err)),
    }
}

fn relative_storage_path(root: &Path, absolute_path: &Path) -> String {
    absolute_path
        .strip_prefix(root)
        .unwrap_or(absolute_path)
        .to_string_lossy()
        .replace('\\', "/")
}
