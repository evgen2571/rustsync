use axum::{
    Router,
    body::Bytes,
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
};
use rustsync_protocol::WorkspaceId;

use crate::{AppState, error::ServerResult};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/workspaces/{workspace_id}/manifest",
        get(get_manifest).put(put_manifest),
    )
}

async fn get_manifest(
    State(state): State<AppState>,
    Path(workspace_id): Path<WorkspaceId>,
) -> ServerResult<impl IntoResponse> {
    let bytes = state.storage.load_manifest(&workspace_id).await?;

    Ok(bytes)
}

async fn put_manifest(
    State(state): State<AppState>,
    Path(workspace_id): Path<WorkspaceId>,
    body: Bytes,
) -> ServerResult<impl IntoResponse> {
    state.storage.save_manifest(&workspace_id, &body).await?;

    Ok("manifest uploaded")
}
