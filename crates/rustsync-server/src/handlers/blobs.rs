use axum::{
    body::Bytes,
    extract::{Path, State},
    response::IntoResponse,
};

use crate::{error::ServerError, state::AppState};

pub async fn get_blob(
    State(state): State<AppState>,
    Path(blob_id): Path<String>,
) -> Result<impl IntoResponse, ServerError> {
    let bytes = state.storage.load_blob(&blob_id).await?;

    Ok(bytes)
}

pub async fn put_blob(
    State(state): State<AppState>,
    Path(blob_id): Path<String>,
    body: Bytes,
) -> Result<impl IntoResponse, ServerError> {
    state.storage.save_blob(&blob_id, &body).await?;

    Ok("blob uploaded")
}
