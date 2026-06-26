use rustsync_protocol::{
    BlobId, CreateWorkspaceRequest, CreateWorkspaceResponse, ManifestId, ObjectUploadResponse,
    UpdateHeadRequest, WORKSPACES_ROUTE, WorkspaceHead, WorkspaceId, WorkspaceSyncEndpoint,
};
use url::Url;

use crate::{
    ClientConfig, ClientResult, RequestSigner,
    transport::{self, Method},
};

#[derive(Debug)]
pub struct RustSyncClient<S> {
    config: ClientConfig,
    signer: S,
    http: reqwest::Client,
}

impl<S> RustSyncClient<S>
where
    S: RequestSigner,
{
    #[must_use]
    pub fn new(config: ClientConfig, signer: S) -> Self {
        let http = reqwest::Client::builder()
            .timeout(config.request_timeout())
            .build()
            .expect("reqwest client configuration should be valid");

        Self {
            config,
            signer,
            http,
        }
    }

    pub async fn upload_blob(
        &self,
        workspace_id: &WorkspaceId,
        blob_id: &BlobId,
        bytes: impl AsRef<[u8]>,
    ) -> ClientResult<ObjectUploadResponse> {
        let path =
            WorkspaceSyncEndpoint::blob(workspace_id.clone(), blob_id.clone()).relative_path();
        self.upload_object(&path, bytes).await
    }

    pub async fn create_workspace(
        &self,
        request: &CreateWorkspaceRequest,
    ) -> ClientResult<CreateWorkspaceResponse> {
        transport::request_json_body_signed(
            &self.http,
            self.config.base_url(),
            Method::Post,
            WORKSPACES_ROUTE.trim_start_matches('/'),
            request,
            &self.signer,
        )
        .await
    }

    pub async fn download_blob(
        &self,
        workspace_id: &WorkspaceId,
        blob_id: &BlobId,
    ) -> ClientResult<Vec<u8>> {
        let path =
            WorkspaceSyncEndpoint::blob(workspace_id.clone(), blob_id.clone()).relative_path();
        self.download_object(&path).await
    }

    pub async fn upload_manifest(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
        bytes: impl AsRef<[u8]>,
    ) -> ClientResult<ObjectUploadResponse> {
        let path = WorkspaceSyncEndpoint::manifest(workspace_id.clone(), manifest_id.clone())
            .relative_path();
        self.upload_object(&path, bytes).await
    }

    pub async fn download_manifest(
        &self,
        workspace_id: &WorkspaceId,
        manifest_id: &ManifestId,
    ) -> ClientResult<Vec<u8>> {
        let path = WorkspaceSyncEndpoint::manifest(workspace_id.clone(), manifest_id.clone())
            .relative_path();
        self.download_object(&path).await
    }

    pub async fn fetch_workspace_head(
        &self,
        workspace_id: &WorkspaceId,
    ) -> ClientResult<WorkspaceHead> {
        let path = WorkspaceSyncEndpoint::head(workspace_id.clone()).relative_path();
        transport::request_json_signed(
            &self.http,
            self.config.base_url(),
            Method::Get,
            &path,
            Vec::new(),
            &self.signer,
        )
        .await
    }

    pub async fn update_workspace_head(
        &self,
        workspace_id: &WorkspaceId,
        expected_revision: u64,
        manifest_id: &ManifestId,
    ) -> ClientResult<WorkspaceHead> {
        let path = WorkspaceSyncEndpoint::head(workspace_id.clone()).relative_path();
        let request = UpdateHeadRequest {
            expected_revision,
            manifest_id: manifest_id.clone(),
        };
        transport::request_json_body_signed(
            &self.http,
            self.config.base_url(),
            Method::Put,
            &path,
            &request,
            &self.signer,
        )
        .await
    }

    async fn upload_object(
        &self,
        path: &str,
        bytes: impl AsRef<[u8]>,
    ) -> ClientResult<ObjectUploadResponse> {
        transport::request_json_signed(
            &self.http,
            self.config.base_url(),
            Method::Put,
            path,
            bytes.as_ref().to_vec(),
            &self.signer,
        )
        .await
    }

    async fn download_object(&self, path: &str) -> ClientResult<Vec<u8>> {
        transport::request_bytes_signed(
            &self.http,
            self.config.base_url(),
            Method::Get,
            path,
            Vec::new(),
            &self.signer,
        )
        .await
    }

    #[must_use]
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    #[must_use]
    pub fn base_url(&self) -> &Url {
        self.config.base_url()
    }

    #[must_use]
    pub fn signer(&self) -> &S {
        &self.signer
    }
}
