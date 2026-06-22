use rustsync_protocol::ApiErrorResponse;
use thiserror::Error;

pub type ClientResult<T> = Result<T, ClientError>;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("server returned an error: {0:?}")]
    Server(ApiErrorResponse),

    #[error("request signing failed: {0}")]
    Signing(String),

    #[error("network request failed: {0}")]
    Network(#[from] reqwest::Error),

    #[error("invalid client configuration: {0}")]
    InvalidConfig(String),
}
