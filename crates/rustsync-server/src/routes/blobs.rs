use axum::{Router, routing::get};

use crate::{
    handlers::blobs::{get_blob, put_blob},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/blobs/{blob_id}", get(get_blob).put(put_blob))
}
