use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use rustsync_protocol::{BlobId, ManifestId};
use rustsync_server::{AppState, FsStorage, create_app};
use tempfile::TempDir;
use tower::ServiceExt;

fn app_with_temp_storage() -> (axum::Router, TempDir) {
    let temp = tempfile::tempdir().expect("create temp dir");
    let storage = FsStorage::new(temp.path().to_path_buf());
    let app = create_app(AppState::new(storage));

    (app, temp)
}

#[tokio::test]
async fn health_endpoint_reports_ok() {
    let (app, _temp) = app_with_temp_storage();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("bulid request"),
        )
        .await
        .expect("send request");

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    assert_eq!(body.as_ref(), br#"{"status": "ok"}"#);
}

#[tokio::test]
async fn blob_enpoint_stores_and_server_workspace_scoped_bytes() {
    let (app, _temp) = app_with_temp_storage();
    let workspace_id = "workspace_test";
    let bytes = b"test blob bytes";
    let blob_id = BlobId::from_content(bytes);
    let uri = format!("/workspaces/{workspace_id}/blobs/{blob_id}");

    let put_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&uri)
                .body(Body::from(bytes.as_slice()))
                .expect("build put request"),
        )
        .await
        .expect("send put request");

    assert_eq!(put_response.status(), StatusCode::CREATED);

    let get_response = app
        .oneshot(
            Request::builder()
                .uri(&uri)
                .body(Body::empty())
                .expect("build get request"),
        )
        .await
        .expect("send get request");

    assert_eq!(get_response.status(), StatusCode::OK);
    assert_eq!(
        get_response.headers().get(header::CONTENT_TYPE),
        Some(&header::HeaderValue::from_static(
            "application/octet-stream"
        ))
    );

    let body = to_bytes(get_response.into_body(), usize::MAX)
        .await
        .expect("read blob body");
    assert_eq!(body.as_ref(), bytes);
}

#[tokio::test]
async fn manifest_endpoint_reports_conflict_for_mismatched_existing_bytes() {
    let (app, _temp) = app_with_temp_storage();
    let workspace_id = "workspace_test";
    let bytes = b"test manifest bytes";
    let manifest_id = ManifestId::from_content(bytes);
    let uri = format!("/workspaces/{workspace_id}/manifests/{manifest_id}");

    let first_put = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&uri)
                .body(Body::from(bytes.as_slice()))
                .expect("build first put request"),
        )
        .await
        .expect("send fisrt put request");

    assert_eq!(first_put.status(), StatusCode::CREATED);

    let conflict = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&uri)
                .body(Body::from("different manifest bytes"))
                .expect("build conflict request"),
        )
        .await
        .expect("send conflict request");

    assert_eq!(conflict.status(), StatusCode::CONFLICT);

    let body = to_bytes(conflict.into_body(), usize::MAX)
        .await
        .expect("read conflict body");
    assert!(
        std::str::from_utf8(body.as_ref())
            .expect("json body is utf8")
            .contains("object_hash_mismatch")
    );
}
