use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use rustsync_protocol::{
    AccessState, BlobId, ContentEncryptionAlgorithm, DeviceId, DeviceJoinRequest, EncryptedObject,
    JoinRequestId, ManifestId, WorkspaceHead, WorkspaceId,
};
use tokio::{fs, sync::Mutex};

use super::{
    BoxStorageFuture, HeadUpdateResult, JoinRequestPutResult, PutResult, Storage, atomic, paths,
    sqlite::{OBJECT_HASH_ALGORITHM, StorageDb, StoredObjectKind, StoredObjectMetadata},
};
use crate::error::{ServerError, ServerResult};

#[derive(Debug, Clone)]
pub struct IndexedFsStorage {
    root: PathBuf,
    db: StorageDb,
    put_lock: Arc<Mutex<()>>,
}

impl IndexedFsStorage {
    pub async fn open(root: PathBuf) -> ServerResult<Self> {
        let db = StorageDb::open(&paths::database_path(&root)).await?;
        Ok(Self {
            root,
            db,
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
        self.db.get_head(workspace_id).await
    }

    pub async fn update_head(
        &self,
        workspace_id: &WorkspaceId,
        expected_revision: u64,
        manifest_id: ManifestId,
        updated_by: Option<DeviceId>,
    ) -> ServerResult<(HeadUpdateResult, WorkspaceHead)> {
        if !self.manifest_exists(workspace_id, &manifest_id).await? {
            return Err(ServerError::ManifestNotFound);
        }

        self.db
            .update_head(workspace_id, expected_revision, manifest_id, updated_by)
            .await
    }

    pub async fn get_access_state(&self, workspace_id: &WorkspaceId) -> ServerResult<AccessState> {
        self.db.get_access_state(workspace_id).await
    }

    pub async fn create_access_state(
        &self,
        workspace_id: &WorkspaceId,
        state: &AccessState,
    ) -> ServerResult<()> {
        self.db.create_access_state(workspace_id, state).await
    }

    pub async fn save_access_state(
        &self,
        workspace_id: &WorkspaceId,
        state: &AccessState,
    ) -> ServerResult<()> {
        self.db.save_access_state(workspace_id, state).await
    }

    pub async fn submit_join_request(
        &self,
        workspace_id: &WorkspaceId,
        request: &DeviceJoinRequest,
    ) -> ServerResult<JoinRequestPutResult> {
        self.db.submit_join_request(workspace_id, request).await
    }

    pub async fn list_join_requests(
        &self,
        workspace_id: &WorkspaceId,
    ) -> ServerResult<Vec<DeviceJoinRequest>> {
        self.db.list_join_requests(workspace_id).await
    }

    pub async fn get_join_request(
        &self,
        workspace_id: &WorkspaceId,
        join_request_id: &JoinRequestId,
    ) -> ServerResult<Option<DeviceJoinRequest>> {
        self.db
            .get_join_request(workspace_id, join_request_id)
            .await
    }

    pub async fn remove_join_request(
        &self,
        workspace_id: &WorkspaceId,
        join_request_id: &JoinRequestId,
    ) -> ServerResult<()> {
        self.db
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
        let relative_path = relative_storage_path(&self.root, &absolute_path)?;

        if let Some(stored) = self
            .db
            .get_object_metadata(workspace_id, kind, object_id)
            .await?
        {
            self.validate_catalog_path(&stored, &absolute_path)?;
        }

        let file_created = if fs::try_exists(&absolute_path)
            .await
            .map_err(ServerError::Storage)?
        {
            let existing = fs::read(&absolute_path).await?;
            if existing != bytes || !object_matches(kind, object_id, &existing) {
                return Err(ServerError::StorageCorruption(format!(
                    "existing {} `{object_id}` does not match its content ID",
                    kind.as_str()
                )));
            }
            false
        } else {
            atomic::write_new(&absolute_path, bytes).await?
        };

        let metadata = object_metadata(workspace_id, object_id, kind, bytes, relative_path);
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
        let canonical = self.object_path(workspace_id, object_id, kind);
        if let Some(stored) = self
            .db
            .get_object_metadata(workspace_id, kind, object_id)
            .await?
        {
            self.validate_catalog_path(&stored, &canonical)?;
        }

        let bytes = read_object(&canonical, missing_error).await?;
        self.verify_and_backfill(workspace_id, object_id, kind, &canonical, &bytes)
            .await?;
        Ok(bytes)
    }

    async fn object_exists(
        &self,
        workspace_id: &str,
        object_id: &str,
        kind: StoredObjectKind,
    ) -> ServerResult<bool> {
        let canonical = self.object_path(workspace_id, object_id, kind);
        if let Some(stored) = self
            .db
            .get_object_metadata(workspace_id, kind, object_id)
            .await?
        {
            self.validate_catalog_path(&stored, &canonical)?;
        }
        let bytes = match fs::read(&canonical).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(ServerError::Storage(error)),
        };
        self.verify_and_backfill(workspace_id, object_id, kind, &canonical, &bytes)
            .await?;
        Ok(true)
    }

    async fn verify_and_backfill(
        &self,
        workspace_id: &str,
        object_id: &str,
        kind: StoredObjectKind,
        canonical: &Path,
        bytes: &[u8],
    ) -> ServerResult<()> {
        if !object_matches(kind, object_id, bytes) {
            return Err(ServerError::StorageCorruption(format!(
                "stored {} `{object_id}` does not match its content ID",
                kind.as_str()
            )));
        }
        if self
            .db
            .get_object_metadata(workspace_id, kind, object_id)
            .await?
            .is_none()
        {
            let metadata = object_metadata(
                workspace_id,
                object_id,
                kind,
                bytes,
                relative_storage_path(&self.root, canonical)?,
            );
            self.db.insert_object_if_absent(&metadata).await?;
        }
        Ok(())
    }

    fn validate_catalog_path(
        &self,
        metadata: &StoredObjectMetadata,
        canonical: &Path,
    ) -> ServerResult<()> {
        let expected = relative_storage_path(&self.root, canonical)?;
        let stored = Path::new(&metadata.storage_path);
        if stored.is_absolute()
            || stored
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
            || stored != Path::new(&expected)
        {
            return Err(ServerError::StorageCorruption(format!(
                "catalog path `{}` does not match canonical path `{expected}`",
                metadata.storage_path
            )));
        }
        Ok(())
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

fn object_matches(kind: StoredObjectKind, object_id: &str, bytes: &[u8]) -> bool {
    match kind {
        StoredObjectKind::Blob | StoredObjectKind::Chunk => {
            BlobId::from_content(bytes).as_str() == object_id
        }
        StoredObjectKind::Manifest => ManifestId::from_content(bytes).as_str() == object_id,
    }
}

fn object_metadata(
    workspace_id: &str,
    object_id: &str,
    kind: StoredObjectKind,
    bytes: &[u8],
    storage_path: String,
) -> StoredObjectMetadata {
    let envelope = serde_json::from_slice::<EncryptedObject>(bytes)
        .ok()
        .filter(|object| object.validate().is_ok());
    let (key_id, encryption_algorithm, nonce) = envelope.map_or((None, None, None), |object| {
        let algorithm = match object.algorithm {
            ContentEncryptionAlgorithm::XChaCha20Poly1305 => "xchacha20poly1305",
        };
        (
            Some(object.key_id.into_inner()),
            Some(algorithm.to_string()),
            Some(object.nonce),
        )
    });
    StoredObjectMetadata {
        workspace_id: workspace_id.to_string(),
        object_id: object_id.to_string(),
        kind,
        hash_algorithm: OBJECT_HASH_ALGORITHM.to_string(),
        encrypted_size: bytes.len() as u64,
        storage_path,
        key_id,
        encryption_algorithm,
        nonce,
    }
}

async fn read_object(path: &Path, missing_error: ServerError) -> ServerResult<Vec<u8>> {
    match fs::read(path).await {
        Ok(bytes) => Ok(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(missing_error),
        Err(err) => Err(ServerError::Storage(err)),
    }
}

fn relative_storage_path(root: &Path, absolute_path: &Path) -> ServerResult<String> {
    absolute_path
        .strip_prefix(root)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .map_err(|_| {
            ServerError::StorageCorruption(format!(
                "object path `{}` is outside storage root `{}`",
                absolute_path.display(),
                root.display()
            ))
        })
}
