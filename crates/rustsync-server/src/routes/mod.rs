use axum::Router;

use crate::state::AppState;

pub mod health;

pub fn routes() -> Router<AppState> {
    Router::new().merge(health::routes())
}
