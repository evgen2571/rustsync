use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use rustsync_protocol::{BlobId, ObjectUploadResponse, WorkspaceId};

use crate::{AppState, error::ServerResult, storage::PutResult};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/workspaces/{workspace_id}/blobs/{blob_id}",
        get(get_blob).put(put_blob),
    )
}

pub async fn get_blob(
    State(state): State<AppState>,
    Path((workspace_id, blob_id)): Path<(WorkspaceId, BlobId)>,
) -> ServerResult<impl IntoResponse> {
    let bytes = state.storage.get_blob(&workspace_id, &blob_id).await?;

    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes))
}

pub async fn put_blob(
    State(state): State<AppState>,
    Path((workspace_id, blob_id)): Path<(WorkspaceId, BlobId)>,
    body: Bytes,
) -> ServerResult<impl IntoResponse> {
    let result = state
        .storage
        .put_blob(&workspace_id, &blob_id, &body)
        .await?;

    Ok(match result {
        PutResult::Created => (StatusCode::CREATED, Json(ObjectUploadResponse::created())),
        PutResult::AlreadyExists => (StatusCode::OK, Json(ObjectUploadResponse::already_exists())),
    })
}
