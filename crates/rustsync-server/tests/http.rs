use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, Uri, header},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rustsync_client::{ClientConfig, ClientError, RequestSigner, RustSyncClient};
use rustsync_core::device::DeviceIdentity;
use rustsync_protocol::{
    AccessEvent, AccessState, BlobId, DeviceStatus, ManifestId, ObjectUploadResponse,
    ObjectUploadStatus, RequestNonce, SignedAccessEvent, UnixTimestamp, WorkspaceId,
    auth::{
        DEVICE_ID_HEADER, NONCE_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER,
        canonical_request_payload, sha256_hex,
    },
    id::AccessEventId,
};
use rustsync_server::{AppState, FsStorage, create_app};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;
use tower::ServiceExt;

const OBJECT_BODY_LIMIT_BYTES: usize = 1024 * 1024;
static NEXT_TEST_NONCE: AtomicU64 = AtomicU64::new(1);

fn app_with_temp_storage() -> (axum::Router, TempDir) {
    let temp = tempfile::tempdir().expect("create temp dir");
    let storage = FsStorage::new(temp.path().to_path_buf());
    let app = create_app(AppState::new(storage));

    (app, temp)
}

async fn app_with_initialized_workspace() -> (axum::Router, TempDir, DeviceIdentity) {
    let temp = tempfile::tempdir().expect("create temp dir");
    let storage = FsStorage::new(temp.path().to_path_buf());
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let identity = DeviceIdentity::generate("server auth test device").expect("generate identity");

    let mut access_state = AccessState::empty(workspace_id.clone());
    let event = SignedAccessEvent::new_unsigned(
        AccessEventId::parse("event_workspace_created").expect("valid event id"),
        workspace_id.clone(),
        0,
        identity.device_id().clone(),
        UnixTimestamp::now(),
        AccessEvent::WorkspaceCreated {
            owner: identity.public_record(DeviceStatus::Active),
        },
    );
    let signature = identity
        .sign(&event.signing_payload())
        .expect("sign workspace-created event");
    access_state
        .apply_verified_event(&event.with_signature(signature))
        .expect("apply workspace-created event");
    storage
        .save_access_state(&workspace_id, &access_state)
        .await
        .expect("persist access state");

    let app = create_app(AppState::new(storage));
    (app, temp, identity)
}

fn signed_request(
    method: Method,
    uri: &str,
    body: impl Into<Vec<u8>>,
    identity: &DeviceIdentity,
) -> Request<Body> {
    let now = UnixTimestamp::now();
    let nonce_number = NEXT_TEST_NONCE.fetch_add(1, Ordering::Relaxed);
    let nonce = RequestNonce::parse(format!("nonce_{}_{}", now.as_secs(), nonce_number))
        .expect("generated nonce is valid");
    signed_request_with_auth(method, uri, body, identity, now, &nonce)
}

fn signed_request_with_auth(
    method: Method,
    uri: &str,
    body: impl Into<Vec<u8>>,
    identity: &DeviceIdentity,
    timestamp: UnixTimestamp,
    nonce: &RequestNonce,
) -> Request<Body> {
    let body = body.into();
    let uri: Uri = uri.parse().expect("valid request uri");
    let path_and_query = uri
        .path_and_query()
        .map_or_else(|| uri.path(), |path_and_query| path_and_query.as_str());
    let payload = canonical_request_payload(
        method.as_str(),
        path_and_query,
        &sha256_hex(&body),
        timestamp,
        identity.device_id(),
        nonce,
        body.len() as u64,
    );
    let signature = identity.sign(&payload).expect("sign request");

    let mut headers = HeaderMap::new();
    headers.insert(
        DEVICE_ID_HEADER,
        HeaderValue::from_str(identity.device_id().as_str()).expect("valid device header"),
    );
    headers.insert(
        TIMESTAMP_HEADER,
        HeaderValue::from_str(&timestamp.as_secs().to_string()).expect("valid timestamp header"),
    );
    headers.insert(
        NONCE_HEADER,
        HeaderValue::from_str(nonce.as_str()).expect("valid nonce header"),
    );
    headers.insert(
        SIGNATURE_HEADER,
        HeaderValue::from_str(&URL_SAFE_NO_PAD.encode(signature)).expect("valid signature header"),
    );

    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::from(body))
        .expect("build signed request");
    request.headers_mut().extend(headers);
    request
}

struct DeviceIdentitySigner<'a>(&'a DeviceIdentity);

impl RequestSigner for DeviceIdentitySigner<'_> {
    fn device_id(&self) -> &rustsync_protocol::DeviceId {
        self.0.device_id()
    }

    fn sign(&self, canonical_request: &[u8]) -> rustsync_client::ClientResult<Vec<u8>> {
        self.0
            .sign(canonical_request)
            .map_err(|error| ClientError::Signing(error.to_string()))
    }
}

async fn assert_error_response(
    response: axum::response::Response,
    expected_status: StatusCode,
    expected_code: &str,
) {
    assert_eq!(response.status(), expected_status);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read error body");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("error body is json");
    assert_eq!(body["error"], expected_code);
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
async fn workspace_sync_endpoint_requires_authentication() {
    let (app, _temp) = app_with_temp_storage();
    let workspace_id = "workspace_test";
    let bytes = b"test blob bytes";
    let blob_id = BlobId::from_content(bytes);
    let uri = format!("/workspaces/{workspace_id}/blobs/{blob_id}");

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&uri)
                .body(Body::from(bytes.as_slice()))
                .expect("build unauthenticated put request"),
        )
        .await
        .expect("send unauthenticated put request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn workspace_sync_endpoint_rejects_invalid_signature() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let workspace_id = "workspace_test";
    let bytes = b"test blob bytes";
    let blob_id = BlobId::from_content(bytes);
    let uri = format!("/workspaces/{workspace_id}/blobs/{blob_id}");
    let mut request = signed_request(Method::PUT, &uri, bytes.as_slice(), &identity);
    request
        .headers_mut()
        .insert(SIGNATURE_HEADER, HeaderValue::from_static("AAAA"));

    let response = app
        .oneshot(request)
        .await
        .expect("send request with invalid signature");

    assert_error_response(response, StatusCode::UNAUTHORIZED, "authentication_failed").await;
}

#[tokio::test]
async fn workspace_sync_endpoint_rejects_replayed_nonce() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let workspace_id = "workspace_test";
    let head_uri = format!("/workspaces/{workspace_id}/head");
    let timestamp = UnixTimestamp::now();
    let nonce = RequestNonce::parse("nonce_replay_test").expect("valid nonce");

    let first_response = app
        .clone()
        .oneshot(signed_request_with_auth(
            Method::GET,
            &head_uri,
            Vec::new(),
            &identity,
            timestamp,
            &nonce,
        ))
        .await
        .expect("send first signed request");
    assert_eq!(first_response.status(), StatusCode::OK);

    let replay_response = app
        .oneshot(signed_request_with_auth(
            Method::GET,
            &head_uri,
            Vec::new(),
            &identity,
            timestamp,
            &nonce,
        ))
        .await
        .expect("send replayed signed request");

    assert_error_response(replay_response, StatusCode::UNAUTHORIZED, "replay_detected").await;
}

#[tokio::test]
async fn workspace_sync_endpoint_rejects_expired_timestamp() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let workspace_id = "workspace_test";
    let head_uri = format!("/workspaces/{workspace_id}/head");
    let timestamp =
        UnixTimestamp::from_secs(UnixTimestamp::now().as_secs().saturating_sub(10 * 60));

    let response = app
        .oneshot(signed_request_with_auth(
            Method::GET,
            &head_uri,
            Vec::new(),
            &identity,
            timestamp,
            &RequestNonce::parse("nonce_expired_timestamp_test").expect("valid nonce"),
        ))
        .await
        .expect("send expired signed request");

    assert_error_response(
        response,
        StatusCode::UNAUTHORIZED,
        "auth_timestamp_outside_window",
    )
    .await;
}

#[tokio::test]
async fn workspace_sync_endpoint_rejects_future_timestamp() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let workspace_id = "workspace_test";
    let head_uri = format!("/workspaces/{workspace_id}/head");
    let timestamp = UnixTimestamp::from_secs(UnixTimestamp::now().as_secs() + 10 * 60);

    let response = app
        .oneshot(signed_request_with_auth(
            Method::GET,
            &head_uri,
            Vec::new(),
            &identity,
            timestamp,
            &RequestNonce::parse("nonce_future_timestamp_test").expect("valid nonce"),
        ))
        .await
        .expect("send future signed request");

    assert_error_response(
        response,
        StatusCode::UNAUTHORIZED,
        "auth_timestamp_outside_window",
    )
    .await;
}

#[tokio::test]
async fn rustsync_client_signed_blob_upload_is_accepted_by_server() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test http listener");
    let base_url = format!("http://{}", listener.local_addr().expect("local addr"));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test app");
    });

    let client = RustSyncClient::new(
        ClientConfig::new(url::Url::parse(&base_url).expect("base url")),
        DeviceIdentitySigner(&identity),
    );
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let bytes = b"client signed blob bytes";
    let blob_id = BlobId::from_content(bytes);

    let response = client
        .upload_blob(&workspace_id, &blob_id, bytes)
        .await
        .expect("client signed upload is accepted by server");

    assert_eq!(response.status, ObjectUploadStatus::Created);
    server.abort();
}

#[tokio::test]
async fn blob_endpoint_stores_and_server_workspace_scoped_bytes() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let workspace_id = "workspace_test";
    let bytes = b"test blob bytes";
    let blob_id = BlobId::from_content(bytes);
    let uri = format!("/workspaces/{workspace_id}/blobs/{blob_id}");

    let put_response = app
        .clone()
        .oneshot(signed_request(
            Method::PUT,
            &uri,
            bytes.as_slice(),
            &identity,
        ))
        .await
        .expect("send put request");

    assert_eq!(put_response.status(), StatusCode::CREATED);
    let put_body = to_bytes(put_response.into_body(), usize::MAX)
        .await
        .expect("read put response body");
    let put_body: ObjectUploadResponse =
        serde_json::from_slice(&put_body).expect("put response body is upload json");
    assert_eq!(put_body.status, ObjectUploadStatus::Created);

    let get_response = app
        .oneshot(signed_request(Method::GET, &uri, Vec::new(), &identity))
        .await
        .expect("send get request");

    assert_eq!(get_response.status(), StatusCode::OK);
    assert_eq!(
        get_response.headers().get(header::CONTENT_TYPE),
        Some(&HeaderValue::from_static("application/octet-stream"))
    );

    let body = to_bytes(get_response.into_body(), usize::MAX)
        .await
        .expect("read blob body");
    assert_eq!(body.as_ref(), bytes);
}

#[tokio::test]
async fn blob_endpoint_rejects_request_body_over_explicit_limit() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let workspace_id = "workspace_test";
    let bytes = vec![b'x'; OBJECT_BODY_LIMIT_BYTES + 1];
    let blob_id = BlobId::from_content(&bytes);
    let uri = format!("/workspaces/{workspace_id}/blobs/{blob_id}");

    let response = app
        .oneshot(signed_request(Method::PUT, &uri, bytes, &identity))
        .await
        .expect("send oversized put request");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn manifest_endpoint_reports_conflict_for_mismatched_existing_bytes() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let workspace_id = "workspace_test";
    let bytes = b"test manifest bytes";
    let manifest_id = ManifestId::from_content(bytes);
    let uri = format!("/workspaces/{workspace_id}/manifests/{manifest_id}");

    let first_put = app
        .clone()
        .oneshot(signed_request(
            Method::PUT,
            &uri,
            bytes.as_slice(),
            &identity,
        ))
        .await
        .expect("send first put request");

    assert_eq!(first_put.status(), StatusCode::CREATED);

    let conflict = app
        .clone()
        .oneshot(signed_request(
            Method::PUT,
            &uri,
            "different manifest bytes",
            &identity,
        ))
        .await
        .expect("send conflict request");

    assert_error_response(conflict, StatusCode::CONFLICT, "object_hash_mismatch").await;
}

#[tokio::test]
async fn head_endpoint_starts_empty_and_updates_to_existing_manifest() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let workspace_id = "workspace_test";
    let manifest_bytes = b"test manifest bytes";
    let manifest_id = ManifestId::from_content(manifest_bytes);
    let manifest_uri = format!("/workspaces/{workspace_id}/manifests/{manifest_id}");
    let head_uri = format!("/workspaces/{workspace_id}/head");

    let empty_response = app
        .clone()
        .oneshot(signed_request(
            Method::GET,
            &head_uri,
            Vec::new(),
            &identity,
        ))
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
        .oneshot(signed_request(
            Method::PUT,
            &manifest_uri,
            manifest_bytes.as_slice(),
            &identity,
        ))
        .await
        .expect("send put manifest request");
    assert_eq!(put_manifest.status(), StatusCode::CREATED);

    let update_body = json!({
        "expected_revision": 0,
        "manifest_id": manifest_id,
    })
    .to_string();
    let mut update_request = signed_request(Method::PUT, &head_uri, update_body, &identity);
    update_request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );

    let update_response = app
        .clone()
        .oneshot(update_request)
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
    assert_eq!(updated["updated_by"], identity.device_id().to_string());
    assert!(updated["updated_at"].as_u64().is_some());
}

#[tokio::test]
async fn head_endpoint_rejects_stale_revision_and_missing_manifest() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let workspace_id = "workspace_test";
    let manifest_bytes = b"test manifest bytes";
    let manifest_id = ManifestId::from_content(manifest_bytes);
    let missing_manifest_id = ManifestId::from_content(b"missing manifest bytes");
    let manifest_uri = format!("/workspaces/{workspace_id}/manifests/{manifest_id}");
    let head_uri = format!("/workspaces/{workspace_id}/head");

    let missing_body = json!({
        "expected_revision": 0,
        "manifest_id": missing_manifest_id,
    })
    .to_string();
    let mut missing_request = signed_request(Method::PUT, &head_uri, missing_body, &identity);
    missing_request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let missing_manifest_response = app
        .clone()
        .oneshot(missing_request)
        .await
        .expect("send missing manifest head request");
    assert_eq!(missing_manifest_response.status(), StatusCode::NOT_FOUND);

    let put_manifest = app
        .clone()
        .oneshot(signed_request(
            Method::PUT,
            &manifest_uri,
            manifest_bytes.as_slice(),
            &identity,
        ))
        .await
        .expect("send put manifest request");
    assert_eq!(put_manifest.status(), StatusCode::CREATED);

    let first_body = json!({
        "expected_revision": 0,
        "manifest_id": manifest_id,
    })
    .to_string();
    let mut first_request = signed_request(Method::PUT, &head_uri, first_body, &identity);
    first_request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let first_update = app
        .clone()
        .oneshot(first_request)
        .await
        .expect("send first head update request");
    assert_eq!(first_update.status(), StatusCode::OK);

    let stale_body = json!({
        "expected_revision": 0,
        "manifest_id": manifest_id,
    })
    .to_string();
    let mut stale_request = signed_request(Method::PUT, &head_uri, stale_body, &identity);
    stale_request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let stale_update = app
        .oneshot(stale_request)
        .await
        .expect("send stale head update request");
    assert_error_response(stale_update, StatusCode::CONFLICT, "head_revision_conflict").await;
}
