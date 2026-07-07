use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use rustsync_protocol::{
    AccessState, BlobId, DeviceId, DeviceJoinRequest, JoinRequestId, ManifestId, UnixTimestamp,
    WorkspaceHead, WorkspaceId,
};
use tokio::{fs, sync::Mutex};

use crate::{
    error::{ServerError, ServerResult},
    storage::{
        BoxStorageFuture, HeadUpdateResult, JoinRequestPutResult, PutResult, Storage, atomic, paths,
    },
};

#[derive(Debug, Clone)]
pub struct FsStorage {
    root: PathBuf,
    head_lock: Arc<Mutex<()>>,
    access_state_lock: Arc<Mutex<()>>,
}

impl FsStorage {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            head_lock: Arc::new(Mutex::new(())),
            access_state_lock: Arc::new(Mutex::new(())),
        }
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

        let path = paths::blob_path(&self.root, workspace_id.as_str(), blob_id.as_str());
        put_immutable_object(&path, bytes).await
    }

    pub async fn get_blob(
        &self,
        workspace_id: &WorkspaceId,
        blob_id: &BlobId,
    ) -> ServerResult<Vec<u8>> {
        let path = paths::blob_path(&self.root, workspace_id.as_str(), blob_id.as_str());
        read_object(&path, ServerError::BlobNotFound).await
    }

    pub async fn blob_exists(
        &self,
        workspace_id: &WorkspaceId,
        blob_id: &BlobId,
    ) -> ServerResult<bool> {
        let path = paths::blob_path(&self.root, workspace_id.as_str(), blob_id.as_str());
        object_exists(&path).await
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

        let path = paths::manifest_path(&self.root, workspace_id.as_str(), manifest_id.as_str());
        put_immutable_object(&path, bytes).await
    }

    pub async fn get_manifest(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
    ) -> ServerResult<Vec<u8>> {
        let path = paths::manifest_path(&self.root, workspace_id.as_str(), manifest_id.as_str());
        read_object(&path, ServerError::ManifestNotFound).await
    }

    pub async fn manifest_exists(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
    ) -> ServerResult<bool> {
        let path = paths::manifest_path(&self.root, workspace_id.as_str(), manifest_id.as_str());
        object_exists(&path).await
    }

    pub async fn get_head(&self, workspace_id: &WorkspaceId) -> ServerResult<WorkspaceHead> {
        let path = paths::head_path(&self.root, workspace_id.as_str());

        match fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(ServerError::InvalidStoredHead),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(WorkspaceHead::empty(workspace_id.clone()))
            }
            Err(err) => Err(ServerError::Storage(err)),
        }
    }

    pub async fn update_head(
        &self,
        workspace_id: &WorkspaceId,
        expected_revision: u64,
        manifest_id: ManifestId,
        updated_by: Option<DeviceId>,
    ) -> ServerResult<(HeadUpdateResult, WorkspaceHead)> {
        let _guard = self.head_lock.lock().await;

        if !self.manifest_exists(workspace_id, &manifest_id).await? {
            return Err(ServerError::ManifestNotFound);
        }

        let current = self.get_head(workspace_id).await?;
        if current.revision != expected_revision {
            return Ok((HeadUpdateResult::Conflict, current));
        }

        let next_revision = current
            .revision
            .checked_add(1)
            .ok_or(ServerError::HeadRevisionOverflow)?;
        let next = WorkspaceHead {
            workspace_id: workspace_id.clone(),
            manifest_id: Some(manifest_id),
            revision: next_revision,
            updated_by,
            updated_at: Some(UnixTimestamp::now()),
        };

        let path = paths::head_path(&self.root, workspace_id.as_str());
        let bytes = serde_json::to_vec(&next).map_err(ServerError::InvalidStoredHead)?;
        atomic::write_replace(&path, &bytes).await?;

        Ok((HeadUpdateResult::Updated, next))
    }

    pub async fn get_access_state(&self, workspace_id: &WorkspaceId) -> ServerResult<AccessState> {
        let path = paths::access_state_path(&self.root, workspace_id.as_str());

        match fs::read(&path).await {
            Ok(bytes) => {
                let state: AccessState = serde_json::from_slice(&bytes)
                    .map_err(ServerError::InvalidStoredAccessStateJson)?;
                Ok(state)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(AccessState::empty(workspace_id.clone()))
            }
            Err(err) => Err(ServerError::Storage(err)),
        }
    }

    pub async fn create_access_state(
        &self,
        workspace_id: &WorkspaceId,
        state: &AccessState,
    ) -> ServerResult<()> {
        let _guard = self.access_state_lock.lock().await;

        validate_access_state(workspace_id, state)?;

        let path = paths::access_state_path(&self.root, workspace_id.as_str());
        if object_exists(&path).await? {
            return Err(ServerError::WorkspaceAlreadyExists);
        }

        let bytes = serde_json::to_vec(state).map_err(ServerError::InvalidStoredAccessStateJson)?;
        if atomic::write_new(&path, &bytes).await? {
            Ok(())
        } else {
            Err(ServerError::WorkspaceAlreadyExists)
        }
    }

    pub async fn save_access_state(
        &self,
        workspace_id: &WorkspaceId,
        state: &AccessState,
    ) -> ServerResult<()> {
        let _guard = self.access_state_lock.lock().await;

        validate_access_state(workspace_id, state)?;

        let path = paths::access_state_path(&self.root, workspace_id.as_str());
        let bytes = serde_json::to_vec(state).map_err(ServerError::InvalidStoredAccessStateJson)?;
        atomic::write_replace(&path, &bytes).await
    }

    pub async fn submit_join_request(
        &self,
        workspace_id: &WorkspaceId,
        request: &DeviceJoinRequest,
    ) -> ServerResult<JoinRequestPutResult> {
        validate_join_request_workspace(workspace_id, request)?;

        let path = paths::join_request_path(
            &self.root,
            workspace_id.as_str(),
            request.request_id.as_str(),
        );
        let bytes = serde_json::to_vec(request).map_err(|error| {
            ServerError::InvalidRequest(format!("invalid join request: {error}"))
        })?;

        if atomic::write_new(&path, &bytes).await? {
            Ok(JoinRequestPutResult::Submitted)
        } else {
            Ok(JoinRequestPutResult::AlreadyPending)
        }
    }

    pub async fn list_join_requests(
        &self,
        workspace_id: &WorkspaceId,
    ) -> ServerResult<Vec<DeviceJoinRequest>> {
        let dir = paths::join_requests_dir(&self.root, workspace_id.as_str());
        let mut entries = match fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(ServerError::Storage(err)),
        };

        let mut paths = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
                paths.push(path);
            }
        }
        paths.sort();

        let mut requests = Vec::with_capacity(paths.len());
        for path in paths {
            let bytes = fs::read(&path).await?;
            let request: DeviceJoinRequest = serde_json::from_slice(&bytes).map_err(|error| {
                ServerError::InvalidRequest(format!("invalid stored join request: {error}"))
            })?;
            validate_join_request_workspace(workspace_id, &request)?;
            requests.push(request);
        }

        Ok(requests)
    }

    pub async fn get_join_request(
        &self,
        workspace_id: &WorkspaceId,
        join_request_id: &JoinRequestId,
    ) -> ServerResult<Option<DeviceJoinRequest>> {
        let path =
            paths::join_request_path(&self.root, workspace_id.as_str(), join_request_id.as_str());

        match fs::read(&path).await {
            Ok(bytes) => {
                let request: DeviceJoinRequest =
                    serde_json::from_slice(&bytes).map_err(|error| {
                        ServerError::InvalidRequest(format!("invalid stored join request: {error}"))
                    })?;
                validate_join_request_workspace(workspace_id, &request)?;
                Ok(Some(request))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(ServerError::Storage(err)),
        }
    }

    pub async fn remove_join_request(
        &self,
        workspace_id: &WorkspaceId,
        join_request_id: &JoinRequestId,
    ) -> ServerResult<()> {
        let path =
            paths::join_request_path(&self.root, workspace_id.as_str(), join_request_id.as_str());

        match fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(ServerError::Storage(err)),
        }
    }
}

impl Storage for FsStorage {
    fn put_blob<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        blob_id: &'a BlobId,
        bytes: &'a [u8],
    ) -> BoxStorageFuture<'a, PutResult> {
        Box::pin(FsStorage::put_blob(self, workspace_id, blob_id, bytes))
    }

    fn get_blob<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        blob_id: &'a BlobId,
    ) -> BoxStorageFuture<'a, Vec<u8>> {
        Box::pin(FsStorage::get_blob(self, workspace_id, blob_id))
    }

    fn blob_exists<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        blob_id: &'a BlobId,
    ) -> BoxStorageFuture<'a, bool> {
        Box::pin(FsStorage::blob_exists(self, workspace_id, blob_id))
    }

    fn put_manifest<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        manifest_id: &'a ManifestId,
        bytes: &'a [u8],
    ) -> BoxStorageFuture<'a, PutResult> {
        Box::pin(FsStorage::put_manifest(
            self,
            workspace_id,
            manifest_id,
            bytes,
        ))
    }

    fn get_manifest<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        manifest_id: &'a ManifestId,
    ) -> BoxStorageFuture<'a, Vec<u8>> {
        Box::pin(FsStorage::get_manifest(self, workspace_id, manifest_id))
    }

    fn manifest_exists<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        manifest_id: &'a ManifestId,
    ) -> BoxStorageFuture<'a, bool> {
        Box::pin(FsStorage::manifest_exists(self, workspace_id, manifest_id))
    }

    fn get_head<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
    ) -> BoxStorageFuture<'a, WorkspaceHead> {
        Box::pin(FsStorage::get_head(self, workspace_id))
    }

    fn update_head<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        expected_revision: u64,
        manifest_id: ManifestId,
        updated_by: Option<DeviceId>,
    ) -> BoxStorageFuture<'a, (HeadUpdateResult, WorkspaceHead)> {
        Box::pin(FsStorage::update_head(
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
        Box::pin(FsStorage::get_access_state(self, workspace_id))
    }

    fn create_access_state<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        state: &'a AccessState,
    ) -> BoxStorageFuture<'a, ()> {
        Box::pin(FsStorage::create_access_state(self, workspace_id, state))
    }

    fn save_access_state<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        state: &'a AccessState,
    ) -> BoxStorageFuture<'a, ()> {
        Box::pin(FsStorage::save_access_state(self, workspace_id, state))
    }

    fn submit_join_request<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        request: &'a DeviceJoinRequest,
    ) -> BoxStorageFuture<'a, JoinRequestPutResult> {
        Box::pin(FsStorage::submit_join_request(self, workspace_id, request))
    }

    fn list_join_requests<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
    ) -> BoxStorageFuture<'a, Vec<DeviceJoinRequest>> {
        Box::pin(FsStorage::list_join_requests(self, workspace_id))
    }

    fn get_join_request<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        join_request_id: &'a JoinRequestId,
    ) -> BoxStorageFuture<'a, Option<DeviceJoinRequest>> {
        Box::pin(FsStorage::get_join_request(
            self,
            workspace_id,
            join_request_id,
        ))
    }

    fn remove_join_request<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        join_request_id: &'a JoinRequestId,
    ) -> BoxStorageFuture<'a, ()> {
        Box::pin(FsStorage::remove_join_request(
            self,
            workspace_id,
            join_request_id,
        ))
    }
}

async fn put_immutable_object(path: &Path, bytes: &[u8]) -> ServerResult<PutResult> {
    if object_exists(path).await? {
        return Ok(PutResult::AlreadyExists);
    }

    let created = atomic::write_new(path, bytes).await?;

    if created {
        Ok(PutResult::Created)
    } else {
        Ok(PutResult::AlreadyExists)
    }
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

async fn read_object(path: &Path, missing_error: ServerError) -> ServerResult<Vec<u8>> {
    match fs::read(path).await {
        Ok(bytes) => Ok(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(missing_error),
        Err(err) => Err(ServerError::Storage(err)),
    }
}

async fn object_exists(path: &Path) -> ServerResult<bool> {
    fs::try_exists(path).await.map_err(ServerError::Storage)
}
