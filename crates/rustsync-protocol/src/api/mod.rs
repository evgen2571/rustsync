pub mod routes;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    InvalidRequest,
    InvalidWorkspaceId,
    InvalidBlobId,
    InvalidManifestId,
    InvalidDeviceId,
    ManifestNotFound,
    BlobNotFound,
    ObjectHashMismatch,
    HeadRevisionConflict,
    HeadRevisionOverflow,
    AuthenticationRequired,
    InvalidAuthHeader,
    InvalidAuthTimestamp,
    AuthTimestampOutsideWindow,
    InvalidSignature,
    AuthenticationFailed,
    PermissionDenied,
    UnauthorizedDevice,
    RevokedDevice,
    ReplayDetected,
    RequestBodyTooLarge,
    InvalidStoredHead,
    InvalidStoredAccessStateJson,
    InvalidAccessState,
    AccessStateWorkspaceMismatch,
    StorageError,
    UnsupportedProtocolVersion,
    MissingRequiredFeature,
    InternalError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiErrorResponse {
    pub error: ApiErrorCode,
    pub message: String,
}

impl ApiErrorResponse {
    pub fn new(error: ApiErrorCode, message: impl Into<String>) -> Self {
        Self {
            error,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObjectUploadStatus {
    Created,
    AlreadyExists,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObjectUploadResponse {
    pub status: ObjectUploadStatus,
}

impl ObjectUploadResponse {
    pub const fn created() -> Self {
        Self {
            status: ObjectUploadStatus::Created,
        }
    }

    pub const fn already_exists() -> Self {
        Self {
            status: ObjectUploadStatus::AlreadyExists,
        }
    }
}
