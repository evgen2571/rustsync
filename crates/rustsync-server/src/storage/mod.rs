mod atomic;
mod fs;
mod paths;

use std::{future::Future, pin::Pin};

use rustsync_protocol::{
    AccessState, BlobId, DeviceId, DeviceJoinRequest, JoinRequestId, ManifestId, WorkspaceHead,
    WorkspaceId,
};

use crate::error::ServerResult;

pub use fs::FsStorage;

pub type BoxStorageFuture<'a, T> = Pin<Box<dyn Future<Output = ServerResult<T>> + Send + 'a>>;

pub trait Storage: Send + Sync {
    fn put_blob<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        blob_id: &'a BlobId,
        bytes: &'a [u8],
    ) -> BoxStorageFuture<'a, PutResult>;

    fn get_blob<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        blob_id: &'a BlobId,
    ) -> BoxStorageFuture<'a, Vec<u8>>;

    fn blob_exists<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        blob_id: &'a BlobId,
    ) -> BoxStorageFuture<'a, bool>;

    fn put_manifest<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        manifest_id: &'a ManifestId,
        bytes: &'a [u8],
    ) -> BoxStorageFuture<'a, PutResult>;

    fn get_manifest<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        manifest_id: &'a ManifestId,
    ) -> BoxStorageFuture<'a, Vec<u8>>;

    fn manifest_exists<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        manifest_id: &'a ManifestId,
    ) -> BoxStorageFuture<'a, bool>;

    fn get_head<'a>(&'a self, workspace_id: &'a WorkspaceId)
    -> BoxStorageFuture<'a, WorkspaceHead>;

    fn update_head<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        expected_revision: u64,
        manifest_id: ManifestId,
        updated_by: Option<DeviceId>,
    ) -> BoxStorageFuture<'a, (HeadUpdateResult, WorkspaceHead)>;

    fn get_access_state<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
    ) -> BoxStorageFuture<'a, AccessState>;

    fn create_access_state<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        state: &'a AccessState,
    ) -> BoxStorageFuture<'a, ()>;

    fn save_access_state<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        state: &'a AccessState,
    ) -> BoxStorageFuture<'a, ()>;

    fn submit_join_request<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        request: &'a DeviceJoinRequest,
    ) -> BoxStorageFuture<'a, JoinRequestPutResult>;

    fn list_join_requests<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
    ) -> BoxStorageFuture<'a, Vec<DeviceJoinRequest>>;

    fn get_join_request<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        join_request_id: &'a JoinRequestId,
    ) -> BoxStorageFuture<'a, Option<DeviceJoinRequest>>;

    fn remove_join_request<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        join_request_id: &'a JoinRequestId,
    ) -> BoxStorageFuture<'a, ()>;
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
