use rustsync_server::{AppState, IndexedFsStorage, ServerConfig, create_app, error::ServerError};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("server startup failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), ServerError> {
    let config = ServerConfig::default();

    let storage = IndexedFsStorage::open(config.storage_dir.clone()).await?;
    let state = AppState::new(storage);
    let app = create_app(state);

    let address = config.bind_addr();
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .map_err(|source| ServerError::Bind {
            address: address.clone(),
            source,
        })?;

    println!("Server running on http://{address}");

    axum::serve(listener, app)
        .await
        .map_err(|source| ServerError::Serve { address, source })
}
