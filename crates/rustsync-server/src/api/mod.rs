use axum::Router;

use crate::state::AppState;

mod blobs;
mod health;
mod manifests;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(health::routes())
        .merge(manifests::routes())
        .merge(blobs::routes())
}
