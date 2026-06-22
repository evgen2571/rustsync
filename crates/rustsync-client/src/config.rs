use std::time::Duration;

use url::Url;

const DEFAULT_REQUEST_TIMEOUT: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    base_url: Url,
    request_timeout: Duration,
}

impl ClientConfig {
    #[must_use]
    pub fn new(base_url: Url) -> Self {
        Self {
            base_url,
            request_timeout: Duration::from_secs(DEFAULT_REQUEST_TIMEOUT),
        }
    }

    #[must_use]
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    #[must_use]
    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    #[must_use]
    pub const fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }
}
