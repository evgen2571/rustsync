use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rustsync_protocol::{ProtocolError, WorkspaceId};
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

    #[error("invalid workspace access state: {0}")]
    InvalidStoredAccessStateJson(serde_json::Error),

    #[error("invalid workspace access state: {0}")]
    InvalidAccessState(ProtocolError),

    #[error("workspace access state belongs to `{actual}`, expected `{expected}`")]
    AccessStateWorkspaceMismatch {
        expected: WorkspaceId,
        actual: WorkspaceId,
    },

    #[error("authentication requried")]
    AuthenticationRequired,

    #[error("invalid authentication header")]
    InvalidAuthHeader,

    #[error("invalid request signature")]
    InvalidRequestSignature,

    #[error("invalid authentication timestamp")]
    InvalidAuthTimestamp,

    #[error("authentication timestamp is outside the accepted window")]
    AuthTimestampOutsideWindow,

    #[error("request body is too large")]
    RequestBodyTooLarge,

    #[error("authorization failed: {0}")]
    AuthProtocol(ProtocolError),

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
            Self::AuthenticationRequired
            | Self::InvalidAuthHeader
            | Self::InvalidRequestSignature
            | Self::InvalidAuthTimestamp
            | Self::AuthTimestampOutsideWindow => StatusCode::UNAUTHORIZED,
            Self::AuthProtocol(ProtocolError::PermissionDenied { .. }) => StatusCode::FORBIDDEN,
            Self::AuthProtocol(_) => StatusCode::UNAUTHORIZED,
            Self::ObjectHashMismatch | Self::HeadRevisionConflict => StatusCode::CONFLICT,
            Self::RequestBodyTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::HeadRevisionOverflow
            | Self::InvalidStoredHead(_)
            | Self::InvalidStoredAccessStateJson(_)
            | Self::InvalidAccessState(_)
            | Self::AccessStateWorkspaceMismatch { .. }
            | Self::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
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
            Self::InvalidStoredAccessStateJson(_) => "invalid_stored_access_state_json",
            Self::InvalidAccessState(_) => "invalid_access_state",
            Self::AccessStateWorkspaceMismatch { .. } => "access_state_workspace_mismatch",
            Self::ObjectHashMismatch => "object_hash_mismatch",
            Self::InvalidStoredHead(_) => "invalid_stored_head",
            Self::AuthenticationRequired => "authentication_required",
            Self::InvalidAuthHeader => "invalid_auth_header",
            Self::InvalidRequestSignature => "invalid_request_signature",
            Self::InvalidAuthTimestamp => "invalid_auth_timestamp",
            Self::AuthTimestampOutsideWindow => "auth_timestamp_outside_window",
            Self::RequestBodyTooLarge => "request_body_too_large",
            Self::AuthProtocol(ProtocolError::PermissionDenied { .. }) => "permission_denied",
            Self::AuthProtocol(_) => "authentication_failed",
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
