use axum::Router;

use crate::{routes, state::AppState};

pub fn create_app(state: AppState) -> Router {
    routes::routes().with_state(state)
}
