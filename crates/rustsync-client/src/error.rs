use rustsync_protocol::ApiErrorResponse;
use thiserror::Error;

pub type ClientResult<T> = Result<T, ClientError>;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("server returned an error: {0:?}")]
    Server(ApiErrorResponse),

    #[error("request timed out")]
    Timeout,

    #[error("request signing failed: {0}")]
    Signing(String),

    #[error("network request failed: {0}")]
    Network(String),

    #[error("invalid serer response: {0}")]
    InvalidResponse(String),

    #[error("invalid client configuration: {0}")]
    InvalidConfig(String),
}

impl ClientError {
    pub(crate) fn from_reqwest(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else if error.is_decode() {
            Self::InvalidResponse(format!("failed to decode JSON response: {error}"))
        } else {
            Self::Network(error.to_string())
        }
    }
}
