use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rustsync_protocol::{ApiErrorCode, ApiErrorResponse, ProtocolError, WorkspaceId};
use std::path::PathBuf;

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

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("manifest was not found")]
    ManifestNotFound,

    #[error("blob was not found")]
    BlobNotFound,

    #[error("key envelope was not found")]
    KeyEnvelopeNotFound,

    #[error("key envelope already exists with different bytes")]
    KeyEnvelopeConflict,

    #[error("authenticated device is not the key envelope recipient")]
    KeyEnvelopeRecipientMismatch,

    #[error("workspace already exists")]
    WorkspaceAlreadyExists,

    #[error("workspace head revision conflict")]
    HeadRevisionConflict,

    #[error("join request conflicts with an existing request")]
    JoinRequestConflict,

    #[error("workspace head revision overflow")]
    HeadRevisionOverflow,

    #[error("object already exists with different bytes")]
    ObjectHashMismatch,

    #[error("object already exists with different metadata")]
    ObjectMetadataMismatch,

    #[error("database schema version {found} is newer than supported version {supported}")]
    UnsupportedSchemaVersion { found: i64, supported: i64 },

    #[error("database value is out of range for {0}")]
    IntegerOutOfRange(&'static str),

    #[error("database contains corrupt data: {0}")]
    CorruptDatabase(String),

    #[error("object storage is corrupt: {0}")]
    StorageCorruption(String),

    #[error("storage root `{root}` is not a directory")]
    StorageRootNotDirectory { root: PathBuf },

    #[error("storage root already in use: `{root}`")]
    StorageRootAlreadyInUse { root: PathBuf },

    #[error(
        "could not {operation} workspace database `{path}` for workspace `{workspace}`: {source}"
    )]
    WorkspaceDatabase {
        workspace: WorkspaceId,
        path: PathBuf,
        operation: &'static str,
        #[source]
        source: Box<ServerError>,
    },

    #[error("could not {operation} storage root `{root}`: {source}")]
    StorageRoot {
        root: PathBuf,
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },

    #[error("could not bind server listener at `{address}`: {source}")]
    Bind {
        address: String,
        #[source]
        source: std::io::Error,
    },

    #[error("server listener at `{address}` failed: {source}")]
    Serve {
        address: String,
        #[source]
        source: std::io::Error,
    },

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

    #[error("authentication required")]
    AuthenticationRequired,

    #[error("invalid authentication header")]
    InvalidAuthHeader,

    #[error("invalid authentication timestamp")]
    InvalidAuthTimestamp,

    #[error("authentication timestamp is outside the accepted window")]
    AuthTimestampOutsideWindow,

    #[error("request replay detected")]
    ReplayDetected,

    #[error("request body is too large")]
    RequestBodyTooLarge,

    #[error("authorization failed: {0}")]
    AuthProtocol(ProtocolError),

    #[error("storage error: {0}")]
    Storage(#[from] std::io::Error),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl ServerError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidWorkspaceId
            | Self::InvalidBlobId
            | Self::InvalidManifestId
            | Self::InvalidDeviceId
            | Self::InvalidRequest(_)
            | Self::InvalidAccessState(_)
            | Self::AccessStateWorkspaceMismatch { .. } => StatusCode::BAD_REQUEST,
            Self::ManifestNotFound | Self::BlobNotFound | Self::KeyEnvelopeNotFound => {
                StatusCode::NOT_FOUND
            }
            Self::WorkspaceAlreadyExists | Self::KeyEnvelopeConflict => StatusCode::CONFLICT,
            Self::AuthenticationRequired
            | Self::InvalidAuthHeader
            | Self::ReplayDetected
            | Self::InvalidAuthTimestamp
            | Self::AuthTimestampOutsideWindow => StatusCode::UNAUTHORIZED,
            Self::AuthProtocol(ProtocolError::PermissionDenied { .. }) => StatusCode::FORBIDDEN,
            Self::KeyEnvelopeRecipientMismatch => StatusCode::FORBIDDEN,
            Self::AuthProtocol(_) => StatusCode::UNAUTHORIZED,
            Self::ObjectHashMismatch
            | Self::ObjectMetadataMismatch
            | Self::HeadRevisionConflict
            | Self::JoinRequestConflict => StatusCode::CONFLICT,
            Self::RequestBodyTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::HeadRevisionOverflow
            | Self::UnsupportedSchemaVersion { .. }
            | Self::IntegerOutOfRange(_)
            | Self::CorruptDatabase(_)
            | Self::StorageCorruption(_)
            | Self::StorageRootNotDirectory { .. }
            | Self::StorageRootAlreadyInUse { .. }
            | Self::WorkspaceDatabase { .. }
            | Self::StorageRoot { .. }
            | Self::Bind { .. }
            | Self::Serve { .. }
            | Self::InvalidStoredHead(_)
            | Self::InvalidStoredAccessStateJson(_)
            | Self::Storage(_)
            | Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_code(&self) -> ApiErrorCode {
        match self {
            Self::InvalidWorkspaceId => ApiErrorCode::InvalidWorkspaceId,
            Self::InvalidBlobId => ApiErrorCode::InvalidBlobId,
            Self::InvalidManifestId => ApiErrorCode::InvalidManifestId,
            Self::InvalidDeviceId => ApiErrorCode::InvalidDeviceId,
            Self::InvalidRequest(_) => ApiErrorCode::InvalidRequest,
            Self::JoinRequestConflict => ApiErrorCode::InvalidRequest,
            Self::ManifestNotFound => ApiErrorCode::ManifestNotFound,
            Self::BlobNotFound => ApiErrorCode::BlobNotFound,
            Self::KeyEnvelopeNotFound => ApiErrorCode::KeyEnvelopeNotFound,
            Self::KeyEnvelopeConflict => ApiErrorCode::KeyEnvelopeConflict,
            Self::WorkspaceAlreadyExists => ApiErrorCode::WorkspaceAlreadyExists,
            Self::HeadRevisionConflict => ApiErrorCode::HeadRevisionConflict,
            Self::HeadRevisionOverflow => ApiErrorCode::HeadRevisionOverflow,
            Self::InvalidStoredAccessStateJson(_) => ApiErrorCode::InvalidStoredAccessStateJson,
            Self::InvalidAccessState(_) => ApiErrorCode::InvalidAccessState,
            Self::AccessStateWorkspaceMismatch { .. } => ApiErrorCode::AccessStateWorkspaceMismatch,
            Self::ObjectHashMismatch | Self::ObjectMetadataMismatch => {
                ApiErrorCode::ObjectHashMismatch
            }
            Self::InvalidStoredHead(_) => ApiErrorCode::InvalidStoredHead,
            Self::AuthenticationRequired => ApiErrorCode::AuthenticationRequired,
            Self::InvalidAuthHeader => ApiErrorCode::InvalidAuthHeader,
            Self::InvalidAuthTimestamp => ApiErrorCode::InvalidAuthTimestamp,
            Self::AuthTimestampOutsideWindow => ApiErrorCode::AuthTimestampOutsideWindow,
            Self::RequestBodyTooLarge => ApiErrorCode::RequestBodyTooLarge,
            Self::ReplayDetected => ApiErrorCode::ReplayDetected,
            Self::AuthProtocol(ProtocolError::PermissionDenied { .. }) => {
                ApiErrorCode::PermissionDenied
            }
            Self::KeyEnvelopeRecipientMismatch => ApiErrorCode::UnauthorizedDevice,
            Self::AuthProtocol(_) => ApiErrorCode::AuthenticationFailed,
            Self::UnsupportedSchemaVersion { .. }
            | Self::IntegerOutOfRange(_)
            | Self::CorruptDatabase(_)
            | Self::StorageCorruption(_)
            | Self::StorageRootNotDirectory { .. }
            | Self::StorageRootAlreadyInUse { .. }
            | Self::WorkspaceDatabase { .. }
            | Self::StorageRoot { .. }
            | Self::Bind { .. }
            | Self::Serve { .. }
            | Self::Storage(_)
            | Self::Database(_) => ApiErrorCode::StorageError,
        }
    }
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let status = self.status_code();

        let message = if status.is_server_error() {
            "internal server error".to_owned()
        } else {
            self.to_string()
        };
        let body = Json(ApiErrorResponse::new(self.error_code(), message));

        (status, body).into_response()
    }
}
