use axum::{Router, routing::get};

use crate::{
    handlers::manifest::{get_manifest, put_manifest},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/workspaces/{workspace_id}/manifest",
        get(get_manifest).put(put_manifest),
    )
}
