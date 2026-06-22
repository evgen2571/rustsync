use url::Url;

use crate::{ClientConfig, RequestSigner};

#[derive(Debug)]
pub struct RustSyncClient<S> {
    config: ClientConfig,
    signer: S,
    _http: reqwest::Client,
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
            _http: http,
        }
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
