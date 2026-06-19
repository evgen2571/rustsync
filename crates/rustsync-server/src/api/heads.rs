use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use rustsync_protocol::{DeviceId, UpdateHeadRequest, WorkspaceId};

use crate::{
    AppState,
    error::{ServerError, ServerResult},
    storage::HeadUpdateResult,
};

// temp solution, not security/auth
const DEVICE_ID_HEADER: &str = "x-rustsync-device-id";

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
    headers: HeaderMap,
    Json(request): Json<UpdateHeadRequest>,
) -> ServerResult<impl IntoResponse> {
    let updated_by = parse_device_id_header(&headers)?;
    let (result, head) = state
        .storage
        .update_head(
            &workspace_id,
            request.expected_revision,
            request.manifest_id,
            updated_by,
        )
        .await?;

    match result {
        HeadUpdateResult::Updated => Ok((StatusCode::OK, Json(head))),
        HeadUpdateResult::Conflict => Err(ServerError::HeadRevisionConflict),
    }
}

fn parse_device_id_header(headers: &HeaderMap) -> ServerResult<Option<DeviceId>> {
    let Some(value) = headers.get(DEVICE_ID_HEADER) else {
        return Ok(None);
    };

    let value = value.to_str().map_err(|_| ServerError::InvalidDeviceId)?;
    DeviceId::parse(value)
        .map(Some)
        .map_err(|_| ServerError::InvalidDeviceId)
}
