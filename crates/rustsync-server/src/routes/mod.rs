use axum::Router;

use crate::state::AppState;

pub mod blobs;
pub mod health;
pub mod manifest;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(health::routes())
        .merge(manifest::routes())
        .merge(blobs::routes())
}
