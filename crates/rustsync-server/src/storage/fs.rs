use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use rustsync_protocol::{
    AccessState, BlobId, DeviceId, DeviceJoinRequest, JoinRequestId, ManifestId, UnixTimestamp,
    WorkspaceHead, WorkspaceId,
};
use tokio::{fs, sync::Mutex};

use crate::{
    error::{ServerError, ServerResult},
    storage::{HeadUpdateResult, JoinRequestPutResult, PutResult, Storage, atomic, paths},
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

#[async_trait]
impl Storage for FsStorage {
    async fn put_blob(&self, w: &WorkspaceId, id: &BlobId, b: &[u8]) -> ServerResult<PutResult> {
        Self::put_blob(self, w, id, b).await
    }
    async fn get_blob(&self, w: &WorkspaceId, id: &BlobId) -> ServerResult<Vec<u8>> {
        Self::get_blob(self, w, id).await
    }
    async fn blob_exists(&self, w: &WorkspaceId, id: &BlobId) -> ServerResult<bool> {
        Self::blob_exists(self, w, id).await
    }
    async fn put_manifest(
        &self,
        w: &WorkspaceId,
        id: &ManifestId,
        b: &[u8],
    ) -> ServerResult<PutResult> {
        Self::put_manifest(self, w, id, b).await
    }
    async fn get_manifest(&self, w: &WorkspaceId, id: &ManifestId) -> ServerResult<Vec<u8>> {
        Self::get_manifest(self, w, id).await
    }
    async fn manifest_exists(&self, w: &WorkspaceId, id: &ManifestId) -> ServerResult<bool> {
        Self::manifest_exists(self, w, id).await
    }
    async fn get_head(&self, w: &WorkspaceId) -> ServerResult<WorkspaceHead> {
        Self::get_head(self, w).await
    }
    async fn update_head(
        &self,
        w: &WorkspaceId,
        e: u64,
        m: ManifestId,
        d: Option<DeviceId>,
    ) -> ServerResult<(HeadUpdateResult, WorkspaceHead)> {
        Self::update_head(self, w, e, m, d).await
    }
    async fn get_access_state(&self, w: &WorkspaceId) -> ServerResult<AccessState> {
        Self::get_access_state(self, w).await
    }
    async fn create_access_state(&self, w: &WorkspaceId, s: &AccessState) -> ServerResult<()> {
        Self::create_access_state(self, w, s).await
    }
    async fn save_access_state(&self, w: &WorkspaceId, s: &AccessState) -> ServerResult<()> {
        Self::save_access_state(self, w, s).await
    }
    async fn submit_join_request(
        &self,
        w: &WorkspaceId,
        r: &DeviceJoinRequest,
    ) -> ServerResult<JoinRequestPutResult> {
        Self::submit_join_request(self, w, r).await
    }
    async fn list_join_requests(&self, w: &WorkspaceId) -> ServerResult<Vec<DeviceJoinRequest>> {
        Self::list_join_requests(self, w).await
    }
    async fn get_join_request(
        &self,
        w: &WorkspaceId,
        id: &JoinRequestId,
    ) -> ServerResult<Option<DeviceJoinRequest>> {
        Self::get_join_request(self, w, id).await
    }
    async fn remove_join_request(&self, w: &WorkspaceId, id: &JoinRequestId) -> ServerResult<()> {
        Self::remove_join_request(self, w, id).await
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
