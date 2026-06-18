use axum::Router;

use crate::{api, state::AppState};

pub fn create_app(state: AppState) -> Router {
    api::routes().with_state(state)
}
