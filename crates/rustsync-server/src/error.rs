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

    #[error("device id is invalid")]
    InvalidDeviceId,

    #[error("manifest was not found")]
    ManifestNotFound,

    #[error("blob was not found")]
    BlobNotFound,

    #[error("workspace head revision conflict")]
    HeadRevisionConflict,

    #[error("workspace head revision overflow")]
    HeadRevisionOverflow,

    #[error("object already exists with different bytes")]
    ObjectHashMismatch,

    #[error("invalid stored workspace head: {0}")]
    InvalidStoredHead(#[from] serde_json::Error),

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
            Self::InvalidWorkspaceId
            | Self::InvalidBlobId
            | Self::InvalidManifestId
            | Self::InvalidDeviceId => StatusCode::BAD_REQUEST,
            Self::ManifestNotFound | Self::BlobNotFound => StatusCode::NOT_FOUND,
            Self::ObjectHashMismatch | Self::HeadRevisionConflict => StatusCode::CONFLICT,
            Self::HeadRevisionOverflow | Self::InvalidStoredHead(_) | Self::Storage(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    fn error_code(&self) -> &'static str {
        match self {
            Self::InvalidWorkspaceId => "invalid_workspace_id",
            Self::InvalidBlobId => "invalid_blob_id",
            Self::InvalidManifestId => "invalid_manifest_id",
            Self::InvalidDeviceId => "invalid_device_id",
            Self::ManifestNotFound => "manifest_not_found",
            Self::BlobNotFound => "blob_not_found",
            Self::HeadRevisionConflict => "head_revision_conflict",
            Self::HeadRevisionOverflow => "head_revision_overflow",
            Self::ObjectHashMismatch => "object_hash_mismatch",
            Self::InvalidStoredHead(_) => "invalid_stored_head",
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
