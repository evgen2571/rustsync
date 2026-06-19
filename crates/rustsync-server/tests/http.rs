use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use rustsync_protocol::{BlobId, DeviceId, ManifestId};
use rustsync_server::{AppState, FsStorage, create_app};
use serde_json::json;
use tempfile::TempDir;
use tower::ServiceExt;

const OBJECT_BODY_LIMIT_BYTES: usize = 1024 * 1024;

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
                .expect("build request"),
        )
        .await
        .expect("send request");

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("health body is json");
    assert_eq!(body, json!({"status": "ok"}));
}

#[tokio::test]
async fn blob_endpoint_stores_and_server_workspace_scoped_bytes() {
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
async fn blob_endpoint_rejects_request_body_over_explicit_limit() {
    let (app, _temp) = app_with_temp_storage();
    let workspace_id = "workspace_test";
    let bytes = vec![b'x'; OBJECT_BODY_LIMIT_BYTES + 1];
    let blob_id = BlobId::from_content(&bytes);
    let uri = format!("/workspaces/{workspace_id}/blobs/{blob_id}");

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&uri)
                .body(Body::from(bytes))
                .expect("build oversized put request"),
        )
        .await
        .expect("send oversized put request");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
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

#[tokio::test]
async fn head_endpoint_starts_empty_and_updates_to_existing_manifest() {
    let (app, _temp) = app_with_temp_storage();
    let workspace_id = "workspace_test";
    let device_id = DeviceId::parse("device_test").expect("valid device id");
    let manifest_bytes = b"test manifest bytes";
    let manifest_id = ManifestId::from_content(manifest_bytes);
    let manifest_uri = format!("/workspaces/{workspace_id}/manifests/{manifest_id}");
    let head_uri = format!("/workspaces/{workspace_id}/head");

    let empty_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&head_uri)
                .body(Body::empty())
                .expect("build empty head request"),
        )
        .await
        .expect("send empty head request");

    assert_eq!(empty_response.status(), StatusCode::OK);
    let empty_body = to_bytes(empty_response.into_body(), usize::MAX)
        .await
        .expect("read empty head body");
    let empty: serde_json::Value = serde_json::from_slice(&empty_body).expect("head body is json");
    assert_eq!(
        empty,
        json!({
            "workspace_id": workspace_id,
            "manifest_id": null,
            "revision": 0,
            "updated_by": null,
            "updated_at": null,
        })
    );

    let put_manifest = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&manifest_uri)
                .body(Body::from(manifest_bytes.as_slice()))
                .expect("build put manifest request"),
        )
        .await
        .expect("send put manifest request");
    assert_eq!(put_manifest.status(), StatusCode::CREATED);

    let update_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&head_uri)
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-rustsync-device-id", device_id.as_str())
                .body(Body::from(
                    json!({
                        "expected_revision": 0,
                        "manifest_id": manifest_id,
                    })
                    .to_string(),
                ))
                .expect("build update head request"),
        )
        .await
        .expect("send update head request");

    assert_eq!(update_response.status(), StatusCode::OK);
    let update_body = to_bytes(update_response.into_body(), usize::MAX)
        .await
        .expect("read updated head body");
    let updated: serde_json::Value =
        serde_json::from_slice(&update_body).expect("head body is json");
    assert_eq!(updated["workspace_id"], workspace_id);
    assert_eq!(updated["manifest_id"], manifest_id.to_string());
    assert_eq!(updated["revision"], 1);
    assert_eq!(updated["updated_by"], device_id.to_string());
    assert!(updated["updated_at"].as_u64().is_some());
}

#[tokio::test]
async fn head_endpoint_rejects_stale_revision_and_missing_manifest() {
    let (app, _temp) = app_with_temp_storage();
    let workspace_id = "workspace_test";
    let manifest_bytes = b"test manifest bytes";
    let manifest_id = ManifestId::from_content(manifest_bytes);
    let missing_manifest_id = ManifestId::from_content(b"missing manifest bytes");
    let manifest_uri = format!("/workspaces/{workspace_id}/manifests/{manifest_id}");
    let head_uri = format!("/workspaces/{workspace_id}/head");

    let missing_manifest_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&head_uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "expected_revision": 0,
                        "manifest_id": missing_manifest_id,
                    })
                    .to_string(),
                ))
                .expect("build missing manifest head request"),
        )
        .await
        .expect("send missing manifest head request");
    assert_eq!(missing_manifest_response.status(), StatusCode::NOT_FOUND);

    let put_manifest = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&manifest_uri)
                .body(Body::from(manifest_bytes.as_slice()))
                .expect("build put manifest request"),
        )
        .await
        .expect("send put manifest request");
    assert_eq!(put_manifest.status(), StatusCode::CREATED);

    let first_update = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&head_uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "expected_revision": 0,
                        "manifest_id": manifest_id,
                    })
                    .to_string(),
                ))
                .expect("build first head update request"),
        )
        .await
        .expect("send first head update request");
    assert_eq!(first_update.status(), StatusCode::OK);

    let stale_update = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&head_uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "expected_revision": 0,
                        "manifest_id": manifest_id,
                    })
                    .to_string(),
                ))
                .expect("build stale head update request"),
        )
        .await
        .expect("send stale head update request");
    assert_eq!(stale_update.status(), StatusCode::CONFLICT);
    let stale_body = to_bytes(stale_update.into_body(), usize::MAX)
        .await
        .expect("read stale body");
    assert!(
        std::str::from_utf8(stale_body.as_ref())
            .expect("json body is utf8")
            .contains("head_revision_conflict")
    );
}
