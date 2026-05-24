use axum::{
    body::Bytes,
    extract::{Path, State},
    response::IntoResponse,
};

use crate::{error::ServerError, state::AppState};

pub async fn get_manifest(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
) -> Result<impl IntoResponse, ServerError> {
    let bytes = state.storage.load_manifest(&workspace_id).await?;

    Ok(bytes)
}

pub async fn put_manifest(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
    body: Bytes,
) -> Result<impl IntoResponse, ServerError> {
    state.storage.save_manifest(&workspace_id, &body).await?;

    Ok("manifest uploaded")
}
