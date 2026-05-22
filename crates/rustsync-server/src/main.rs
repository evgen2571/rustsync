mod app;
mod handlers;
mod routes;
mod state;

use crate::{app::create_app, state::AppState};

#[tokio::main]
async fn main() {
    let state = AppState::new();
    let app = create_app(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    println!("Server running on http://127.0.0.1:3000");

    axum::serve(listener, app).await.unwrap();
}
