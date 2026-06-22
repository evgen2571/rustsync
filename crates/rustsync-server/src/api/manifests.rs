use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use rustsync_protocol::{ManifestId, ObjectUploadResponse, WorkspaceId};

use crate::{AppState, error::ServerResult, storage::PutResult};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/workspaces/{workspace_id}/manifests/{manifest_id}",
        get(get_manifest).put(put_manifest),
    )
}

pub async fn get_manifest(
    State(state): State<AppState>,
    Path((workspace_id, manifest_id)): Path<(WorkspaceId, ManifestId)>,
) -> ServerResult<impl IntoResponse> {
    let bytes = state
        .storage
        .get_manifest(&workspace_id, &manifest_id)
        .await?;

    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes))
}

pub async fn put_manifest(
    State(state): State<AppState>,
    Path((workspace_id, manifest_id)): Path<(WorkspaceId, ManifestId)>,
    body: Bytes,
) -> ServerResult<impl IntoResponse> {
    let result = state
        .storage
        .put_manifest(&workspace_id, &manifest_id, &body)
        .await?;

    Ok(match result {
        PutResult::Created => (StatusCode::CREATED, Json(ObjectUploadResponse::created())),
        PutResult::AlreadyExists => (StatusCode::OK, Json(ObjectUploadResponse::already_exists())),
    })
}
