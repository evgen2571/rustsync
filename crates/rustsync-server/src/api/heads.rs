use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use rustsync_protocol::{UpdateHeadRequest, WorkspaceId};

use crate::{
    AppState,
    auth::AuthenticatedDevice,
    error::{ServerError, ServerResult},
    storage::HeadUpdateResult,
};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/workspaces/{workspace_id}/head",
        get(get_head).put(update_head),
    )
}

pub async fn get_head(
    State(state): State<AppState>,
    Path(workspace_id): Path<WorkspaceId>,
) -> ServerResult<impl IntoResponse> {
    let head = state.storage.get_head(&workspace_id).await?;

    Ok(Json(head))
}

pub async fn update_head(
    State(state): State<AppState>,
    Path(workspace_id): Path<WorkspaceId>,
    Extension(auth): Extension<AuthenticatedDevice>,
    Json(request): Json<UpdateHeadRequest>,
) -> ServerResult<impl IntoResponse> {
    let (result, head) = state
        .storage
        .update_head(
            &workspace_id,
            request.expected_revision,
            request.manifest_id,
            Some(auth.device_id),
        )
        .await?;

    match result {
        HeadUpdateResult::Updated => Ok((StatusCode::OK, Json(head))),
        HeadUpdateResult::Conflict => Err(ServerError::HeadRevisionConflict),
    }
}
