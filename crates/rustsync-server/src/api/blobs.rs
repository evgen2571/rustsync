use axum::{
    Router,
    body::Bytes,
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
};

use crate::{AppState, error::ServerResult};

pub fn routes() -> Router<AppState> {
    Router::new().route("/blobs/{blob_id}", get(get_blob).put(put_blob))
}

pub async fn get_blob(
    State(state): State<AppState>,
    Path(blob_id): Path<String>,
) -> ServerResult<impl IntoResponse> {
    let bytes = state.storage.load_blob(&blob_id).await?;

    Ok(bytes)
}

pub async fn put_blob(
    State(state): State<AppState>,
    Path(blob_id): Path<String>,
    body: Bytes,
) -> ServerResult<impl IntoResponse> {
    state.storage.save_blob(&blob_id, &body).await?;

    Ok("blob uploaded")
}
