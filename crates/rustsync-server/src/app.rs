use axum::{Router, extract::DefaultBodyLimit};

use crate::{api, state::AppState};

const MAX_OBJECT_BODY_BYTES: usize = 1024 * 1024;

pub fn create_app(state: AppState) -> Router {
    api::routes()
        .layer(DefaultBodyLimit::max(MAX_OBJECT_BODY_BYTES))
        .with_state(state)
}
