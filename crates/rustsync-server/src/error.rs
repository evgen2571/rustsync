use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug)]
pub enum ServerError {
    InvalidId,
    ManifestNotFound,
    BlobNotFound,
    Io(std::io::Error),
}

#[derive(Serialize)]
struct ErrorResponse {
    error: &'static str,
    message: String,
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            ServerError::InvalidId => (
                StatusCode::BAD_REQUEST,
                "invalid_id",
                "workspace id is invalid".to_string(),
            ),

            ServerError::ManifestNotFound => (
                StatusCode::NOT_FOUND,
                "manifest_not_found",
                "manifest was not found".to_string(),
            ),

            ServerError::BlobNotFound => (
                StatusCode::NOT_FOUND,
                "blob_not_found",
                "blob was not found".to_string(),
            ),

            ServerError::Io(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_error",
                format!("storage error: {err}"),
            ),
        };

        let body = Json(ErrorResponse { error, message });

        (status, body).into_response()
    }
}

impl From<std::io::Error> for ServerError {
    fn from(err: std::io::Error) -> Self {
        ServerError::Io(err)
    }
}
