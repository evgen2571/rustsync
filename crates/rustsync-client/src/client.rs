use rustsync_protocol::{BlobId, ObjectUploadResponse, WorkspaceId};
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
        let path = format!("workspaces/{workspace_id}/blobs/{blob_id}");
        transport::request_json_signed(
            &self.http,
            self.config.base_url(),
            Method::Put,
            &path,
            bytes.as_ref().to_vec(),
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
