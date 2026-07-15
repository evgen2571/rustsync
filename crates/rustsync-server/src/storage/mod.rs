mod atomic;
mod fs;
mod indexed_fs;
mod paths;
mod sqlite;

use async_trait::async_trait;
use rustsync_protocol::{
    AccessState, BlobId, DeviceId, DeviceJoinRequest, JoinRequestId, ManifestId, WorkspaceHead,
    WorkspaceId,
};

use crate::error::ServerResult;

pub use fs::FsStorage;
pub use indexed_fs::IndexedFsStorage;

#[async_trait]
pub trait Storage: Send + Sync {
    async fn put_blob(
        &self,
        workspace_id: &WorkspaceId,
        blob_id: &BlobId,
        bytes: &[u8],
    ) -> ServerResult<PutResult>;
    async fn get_blob(&self, workspace_id: &WorkspaceId, blob_id: &BlobId)
    -> ServerResult<Vec<u8>>;
    async fn blob_exists(&self, workspace_id: &WorkspaceId, blob_id: &BlobId)
    -> ServerResult<bool>;
    async fn put_manifest(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
        bytes: &[u8],
    ) -> ServerResult<PutResult>;
    async fn get_manifest(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
    ) -> ServerResult<Vec<u8>>;
    async fn manifest_exists(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
    ) -> ServerResult<bool>;
    async fn get_head(&self, workspace_id: &WorkspaceId) -> ServerResult<WorkspaceHead>;
    async fn update_head(
        &self,
        workspace_id: &WorkspaceId,
        expected_revision: u64,
        manifest_id: ManifestId,
        updated_by: Option<DeviceId>,
    ) -> ServerResult<(HeadUpdateResult, WorkspaceHead)>;
    async fn get_access_state(&self, workspace_id: &WorkspaceId) -> ServerResult<AccessState>;
    async fn create_access_state(
        &self,
        workspace_id: &WorkspaceId,
        state: &AccessState,
    ) -> ServerResult<()>;
    async fn save_access_state(
        &self,
        workspace_id: &WorkspaceId,
        state: &AccessState,
    ) -> ServerResult<()>;
    async fn submit_join_request(
        &self,
        workspace_id: &WorkspaceId,
        request: &DeviceJoinRequest,
    ) -> ServerResult<JoinRequestPutResult>;
    async fn list_join_requests(
        &self,
        workspace_id: &WorkspaceId,
    ) -> ServerResult<Vec<DeviceJoinRequest>>;
    async fn get_join_request(
        &self,
        workspace_id: &WorkspaceId,
        join_request_id: &JoinRequestId,
    ) -> ServerResult<Option<DeviceJoinRequest>>;
    async fn remove_join_request(
        &self,
        workspace_id: &WorkspaceId,
        join_request_id: &JoinRequestId,
    ) -> ServerResult<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutResult {
    Created,
    AlreadyExists,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadUpdateResult {
    Updated,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinRequestPutResult {
    Submitted,
    AlreadyPending,
}
