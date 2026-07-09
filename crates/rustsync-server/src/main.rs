use rustsync_server::{AppState, IndexedFsStorage, ServerConfig, create_app};

#[tokio::main]
async fn main() {
    let config = ServerConfig::default();

    let storage = IndexedFsStorage::open(config.storage_dir.clone())
        .await
        .unwrap();
    let state = AppState::new(storage);

    let app = create_app(state);

    let listener = tokio::net::TcpListener::bind(config.bind_addr())
        .await
        .unwrap();

    println!("Server running on http://{}", config.bind_addr());

    axum::serve(listener, app).await.unwrap();
}
