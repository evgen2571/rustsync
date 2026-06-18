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

    #[error("blob id is invalid")]
    InvalidBlobId,

    #[error("manifest id is invalid")]
    InvalidManifestId,

    #[error("manifest was not found")]
    ManifestNotFound,

    #[error("blob was not found")]
    BlobNotFound,

    #[error("object already exists with different bytes")]
    ObjectHashMismatch,

    #[error("storage error: {0}")]
    Storage(#[from] std::io::Error),
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: &'static str,
    message: String,
}

impl ServerError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidWorkspaceId | Self::InvalidBlobId | Self::InvalidManifestId => {
                StatusCode::BAD_REQUEST
            }
            Self::ManifestNotFound | Self::BlobNotFound => StatusCode::NOT_FOUND,
            Self::ObjectHashMismatch => StatusCode::CONFLICT,
            Self::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_code(&self) -> &'static str {
        match self {
            Self::InvalidWorkspaceId => "invalid_workspace_id",
            Self::InvalidBlobId => "invalid_blob_id",
            Self::InvalidManifestId => "invalid_manifest_id",
            Self::ManifestNotFound => "manifest_not_found",
            Self::BlobNotFound => "blob_not_found",
            Self::ObjectHashMismatch => "object_hash_mismatch",
            Self::Storage(_) => "storage_error",
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
