mod app;
mod error;
mod handlers;
mod routes;
mod state;
mod storage;

use crate::{app::create_app, state::AppState, storage::Storage};

#[tokio::main]
async fn main() {
    let storage = Storage::new("./server-storage".into());
    let state = AppState::new(storage);

    let app = create_app(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    println!("Server running on http://127.0.0.1:3000");

    axum::serve(listener, app).await.unwrap();
}
