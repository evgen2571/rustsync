use axum::Router;

use crate::state::AppState;

mod blobs;
mod heads;
mod health;
mod manifests;
mod workspaces;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(health::routes())
        .merge(workspaces::routes())
        .merge(heads::routes())
        .merge(manifests::routes())
        .merge(blobs::routes())
}
