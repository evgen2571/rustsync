use axum::{Router, routing::get};

use crate::{handlers::health::health_check, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/health", get(health_check))
}
