use axum::{Router, extract::DefaultBodyLimit, middleware::from_fn_with_state};

use crate::{api, auth::workspace_auth_middleware, state::AppState};

const MAX_OBJECT_BODY_BYTES: usize = 1024 * 1024;

pub fn create_app(state: AppState) -> Router {
    api::routes()
        .layer(DefaultBodyLimit::max(MAX_OBJECT_BODY_BYTES))
        .layer(from_fn_with_state(state.clone(), workspace_auth_middleware))
        .with_state(state)
}
