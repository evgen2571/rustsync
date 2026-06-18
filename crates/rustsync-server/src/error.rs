use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

pub type ServerResult<T> = Result<T, ServerError>;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("workspace id is invalid")]
    InvalidWorkspaceId,

    #[error("manifest was not found")]
    ManifestNotFound,

    #[error("blob was not found")]
    BlobNotFound,

    #[error("storage error: {0}")]
    Storage(#[from] std::io::Error),

    #[error("server I/O error")]
    Io(std::io::Error),
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: &'static str,
    message: String,
}

impl ServerError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidWorkspaceId => StatusCode::BAD_REQUEST,
            Self::ManifestNotFound | Self::BlobNotFound => StatusCode::NOT_FOUND,
            Self::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_code(&self) -> &'static str {
        match self {
            Self::InvalidWorkspaceId => "invalid_workspace_id",
            Self::ManifestNotFound => "manifest_not_found",
            Self::BlobNotFound => "blob_not_found",
            Self::Storage(_) => "storage_error",
            Self::Io(_) => "io_error",
        }
    }
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let status = self.status_code();

        let body = Json(ErrorResponse {
            error: self.error_code(),
            message: self.to_string(),
        });

        (status, body).into_response()
    }
}
