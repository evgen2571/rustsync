use axum::{
    body::Bytes,
    extract::Path,
    response::{IntoResponse, Json},
};

use serde_json::json;

pub async fn get_manifest(Path(workspace_id): Path<String>) -> impl IntoResponse {
    let _workspace_id = workspace_id;

    "encrypted manifest"
}

pub async fn put_manifest(Path(workspace_id): Path<String>, body: Bytes) -> impl IntoResponse {
    let _workspace_id = workspace_id;
    let _encrypted_manifest = body;

    Json(json!({ "status": "ok", }))
}
