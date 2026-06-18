use std::path::{Path, PathBuf};

use rustsync_protocol::{BlobId, ManifestId, WorkspaceId};
use tokio::fs;

use crate::{
    error::{ServerError, ServerResult},
    storage::{PutResult, atomic, paths},
};

#[derive(Debug, Clone)]
pub struct FsStorage {
    root: PathBuf,
}

impl FsStorage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
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
