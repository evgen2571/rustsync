use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, Uri, header},
};
use rustsync_client::{ClientConfig, ClientError, RequestSigner, RustSyncClient};
use rustsync_core::device::DeviceIdentity;
use rustsync_protocol::{
    AccessEvent, AccessState, ApiErrorCode, BlobId, CreateWorkspaceRequest, DeviceStatus,
    ManifestId, ObjectUploadResponse, ObjectUploadStatus, RequestNonce, SignedAccessEvent,
    UnixTimestamp, WorkspaceHead, WorkspaceId, WorkspaceRole,
    auth::{
        DEVICE_ID_HEADER, NONCE_HEADER, SIGNATURE_HEADER, SignedHttpRequestParts, TIMESTAMP_HEADER,
    },
    id::AccessEventId,
};
use rustsync_server::{AppState, IndexedFsStorage, create_app};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;
use tower::ServiceExt;

const OBJECT_BODY_LIMIT_BYTES: usize = 1024 * 1024;
static NEXT_TEST_NONCE: AtomicU64 = AtomicU64::new(1);

async fn app_with_temp_storage() -> (axum::Router, TempDir) {
    let temp = tempfile::tempdir().expect("create temp dir");
    let storage = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let app = create_app(AppState::new(storage));

    (app, temp)
}

async fn app_with_initialized_workspace() -> (axum::Router, TempDir, DeviceIdentity) {
    let temp = tempfile::tempdir().expect("create temp dir");
    let storage = IndexedFsStorage::open(temp.path().to_path_buf())
        .await
        .expect("open indexed storage");
    let (workspace_id, access_state, identity) = initial_owner_access_state("workspace_test");

    storage
        .save_access_state(&workspace_id, &access_state)
        .await
        .expect("persist access state");

    let app = create_app(AppState::new(storage));
    (app, temp, identity)
}

fn initial_owner_access_state(workspace_id: &str) -> (WorkspaceId, AccessState, DeviceIdentity) {
    let workspace_id = WorkspaceId::parse(workspace_id).expect("valid workspace id");
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

    (workspace_id, access_state, identity)
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
    let input = SignedHttpRequestParts::new(method.as_str(), path_and_query, &body)
        .signature_input(identity.device_id().clone(), timestamp, nonce.clone());
    let signature = identity
        .sign(&input.canonical_payload())
        .expect("sign request");
    let headers = input
        .with_signature(signature)
        .auth_headers()
        .to_header_values();

    let mut header_map = HeaderMap::new();
    header_map.insert(
        DEVICE_ID_HEADER,
        HeaderValue::from_str(&headers.device_id).expect("valid device header"),
    );
    header_map.insert(
        TIMESTAMP_HEADER,
        HeaderValue::from_str(&headers.timestamp).expect("valid timestamp header"),
    );
    header_map.insert(
        NONCE_HEADER,
        HeaderValue::from_str(&headers.nonce).expect("valid nonce header"),
    );
    header_map.insert(
        SIGNATURE_HEADER,
        HeaderValue::from_str(&headers.signature).expect("valid signature header"),
    );

    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::from(body))
        .expect("build signed request");
    request.headers_mut().extend(header_map);
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
    let (app, _temp) = app_with_temp_storage().await;

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
async fn create_workspace_accepts_initial_owner_signed_access_state() {
    let (app, _temp) = app_with_temp_storage().await;
    let (workspace_id, access_state, identity) = initial_owner_access_state("workspace_test");
    let request_body = serde_json::to_vec(&CreateWorkspaceRequest {
        workspace_id: workspace_id.clone(),
        access_state,
    })
    .expect("serialize create workspace request");

    let response = app
        .clone()
        .oneshot(signed_request(
            Method::POST,
            "/workspaces",
            request_body,
            &identity,
        ))
        .await
        .expect("send create workspace request");

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read create workspace body");
    let body: serde_json::Value =
        serde_json::from_slice(&body).expect("create workspace body is json");
    assert_eq!(body["workspace_id"], workspace_id.as_str());
    assert_eq!(
        serde_json::from_value::<WorkspaceHead>(body["head"].clone()).expect("decode head"),
        WorkspaceHead::empty(workspace_id.clone())
    );

    let response = app
        .oneshot(signed_request(
            Method::GET,
            "/workspaces/workspace_test/head",
            Vec::new(),
            &identity,
        ))
        .await
        .expect("send signed head request after create");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read head body");
    let head: WorkspaceHead = serde_json::from_slice(&body).expect("head body is json");
    assert_eq!(head, WorkspaceHead::empty(workspace_id));
}

#[tokio::test]
async fn create_workspace_requires_initial_owner_signature() {
    let (app, _temp) = app_with_temp_storage().await;
    let (workspace_id, access_state, _identity) = initial_owner_access_state("workspace_test");
    let request_body = serde_json::to_vec(&CreateWorkspaceRequest {
        workspace_id,
        access_state,
    })
    .expect("serialize create workspace request");

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/workspaces")
                .body(Body::from(request_body))
                .expect("build unsigned create workspace request"),
        )
        .await
        .expect("send unsigned create workspace request");

    assert_error_response(
        response,
        StatusCode::UNAUTHORIZED,
        "authentication_required",
    )
    .await;
}

#[tokio::test]
async fn create_workspace_rejects_duplicate_workspace() {
    let (app, _temp) = app_with_temp_storage().await;
    let (workspace_id, access_state, identity) = initial_owner_access_state("workspace_test");
    let request_body = serde_json::to_vec(&CreateWorkspaceRequest {
        workspace_id,
        access_state,
    })
    .expect("serialize create workspace request");

    for expected_status in [StatusCode::CREATED, StatusCode::CONFLICT] {
        let response = app
            .clone()
            .oneshot(signed_request(
                Method::POST,
                "/workspaces",
                request_body.clone(),
                &identity,
            ))
            .await
            .expect("send create workspace request");

        if expected_status == StatusCode::CREATED {
            assert_eq!(response.status(), expected_status);
        } else {
            assert_error_response(response, expected_status, "workspace_already_exists").await;
        }
    }
}

#[tokio::test]
async fn create_workspace_rejects_mismatched_access_state_workspace() {
    let (app, _temp) = app_with_temp_storage().await;
    let (_state_workspace_id, access_state, identity) =
        initial_owner_access_state("workspace_test");
    let request_body = serde_json::to_vec(&CreateWorkspaceRequest {
        workspace_id: WorkspaceId::parse("workspace_other").expect("valid workspace id"),
        access_state,
    })
    .expect("serialize create workspace request");

    let response = app
        .oneshot(signed_request(
            Method::POST,
            "/workspaces",
            request_body,
            &identity,
        ))
        .await
        .expect("send mismatched create workspace request");

    assert_error_response(
        response,
        StatusCode::BAD_REQUEST,
        "access_state_workspace_mismatch",
    )
    .await;
}

#[tokio::test]
async fn workspace_sync_endpoint_requires_authentication() {
    let (app, _temp) = app_with_temp_storage().await;
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
async fn authenticated_workspace_route_with_invalid_blob_id_returns_bad_request() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let request = signed_request(
        Method::GET,
        "/workspaces/workspace_test/blobs/not-a-blob-id",
        Vec::new(),
        &identity,
    );

    let response = app
        .oneshot(request)
        .await
        .expect("send signed request with invalid blob id");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read invalid blob response body");
    assert!(
        !body.is_empty(),
        "invalid blob id response should explain the bad request"
    );
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
async fn workspace_sync_endpoint_rejects_tampered_signed_body() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let workspace_id = "workspace_test";
    let signed_bytes = b"original signed blob bytes";
    let blob_id = BlobId::from_content(signed_bytes);
    let uri = format!("/workspaces/{workspace_id}/blobs/{blob_id}");
    let request = signed_request(Method::PUT, &uri, signed_bytes.as_slice(), &identity);
    let (parts, _body) = request.into_parts();
    let tampered_request = Request::from_parts(parts, Body::from("tampered blob bytes"));

    let response = app
        .oneshot(tampered_request)
        .await
        .expect("send signed request with tampered body");

    assert_error_response(response, StatusCode::UNAUTHORIZED, "authentication_failed").await;
}

#[tokio::test]
async fn workspace_sync_endpoint_rejects_tampered_signed_path_or_header() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let head_uri = "/workspaces/workspace_test/head";
    let mut path_tampered = signed_request(Method::GET, head_uri, Vec::new(), &identity);
    *path_tampered.uri_mut() = "/workspaces/workspace_test/head?tampered=1"
        .parse()
        .expect("valid tampered uri");

    let path_response = app
        .clone()
        .oneshot(path_tampered)
        .await
        .expect("send signed request with tampered path");
    assert_error_response(
        path_response,
        StatusCode::UNAUTHORIZED,
        "authentication_failed",
    )
    .await;

    let mut header_tampered = signed_request(Method::GET, head_uri, Vec::new(), &identity);
    header_tampered
        .headers_mut()
        .insert(NONCE_HEADER, HeaderValue::from_static("nonce_tampered"));

    let header_response = app
        .oneshot(header_tampered)
        .await
        .expect("send signed request with tampered nonce header");
    assert_error_response(
        header_response,
        StatusCode::UNAUTHORIZED,
        "authentication_failed",
    )
    .await;
}

#[tokio::test]
async fn workspace_sync_endpoint_rejects_malformed_auth_headers() {
    let (app, _temp, identity) = app_with_initialized_workspace().await;
    let mut request = signed_request(
        Method::GET,
        "/workspaces/workspace_test/head",
        Vec::new(),
        &identity,
    );
    request.headers_mut().insert(
        TIMESTAMP_HEADER,
        HeaderValue::from_static("not-a-timestamp"),
    );

    let response = app
        .oneshot(request)
        .await
        .expect("send request with malformed timestamp header");

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
async fn rustsync_client_signed_object_operations_are_accepted_by_server() {
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
    let blob_bytes = b"client signed blob bytes";
    let blob_id = BlobId::from_content(blob_bytes);

    let blob_upload = client
        .upload_blob(&workspace_id, &blob_id, blob_bytes)
        .await
        .expect("client signed blob upload is accepted by server");
    assert_eq!(blob_upload.status, ObjectUploadStatus::Created);

    let downloaded_blob = client
        .download_blob(&workspace_id, &blob_id)
        .await
        .expect("client signed blob download is accepted by server");
    assert_eq!(downloaded_blob, blob_bytes);

    let manifest_bytes = b"client signed manifest bytes";
    let manifest_id = ManifestId::from_content(manifest_bytes);

    let manifest_upload = client
        .upload_manifest(&workspace_id, &manifest_id, manifest_bytes)
        .await
        .expect("client signed manifest upload is accepted by server");
    assert_eq!(manifest_upload.status, ObjectUploadStatus::Created);

    let downloaded_manifest = client
        .download_manifest(&workspace_id, &manifest_id)
        .await
        .expect("client signed manifest download is accepted by server");
    assert_eq!(downloaded_manifest, manifest_bytes);

    let empty_head = client
        .fetch_workspace_head(&workspace_id)
        .await
        .expect("client signed head fetch is accepted by server");
    assert_eq!(empty_head.workspace_id, workspace_id);
    assert_eq!(empty_head.manifest_id, None);
    assert_eq!(empty_head.revision, 0);

    let updated_head = client
        .update_workspace_head(&workspace_id, 0, &manifest_id)
        .await
        .expect("client signed head update is accepted by server");
    assert_eq!(updated_head.manifest_id, Some(manifest_id.clone()));
    assert_eq!(updated_head.revision, 1);
    assert_eq!(updated_head.updated_by, Some(identity.device_id().clone()));
    assert!(updated_head.updated_at.is_some());

    let fetched_head = client
        .fetch_workspace_head(&workspace_id)
        .await
        .expect("client signed updated head fetch is accepted by server");
    assert_eq!(fetched_head, updated_head);

    let stale_error = client
        .update_workspace_head(&workspace_id, 0, &manifest_id)
        .await
        .expect_err("stale client head update should fail cleanly");
    match stale_error {
        ClientError::Server(response) => {
            assert_eq!(response.error, ApiErrorCode::HeadRevisionConflict);
        }
        other => panic!("expected head revision conflict, got {other:?}"),
    }

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

fn signed_device_join_event(
    workspace_id: &WorkspaceId,
    expected_revision: u64,
    owner: &DeviceIdentity,
    join_request: &rustsync_protocol::DeviceJoinRequest,
    role: WorkspaceRole,
    event_suffix: &str,
) -> SignedAccessEvent {
    let event = SignedAccessEvent::new_unsigned(
        AccessEventId::parse(format!("event_join_{event_suffix}")).expect("valid event id"),
        workspace_id.clone(),
        expected_revision,
        owner.device_id().clone(),
        UnixTimestamp::now(),
        AccessEvent::DeviceJoined {
            join_request_id: join_request.request_id.clone(),
            device: join_request.device.clone(),
            role,
        },
    );
    let signature = owner
        .sign(&event.signing_payload())
        .expect("sign device-joined event");
    event.with_signature(signature)
}

#[tokio::test]
async fn join_request_can_be_submitted_listed_approved_and_then_syncs() {
    let (app, _temp, owner) = app_with_initialized_workspace().await;
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let joining = DeviceIdentity::generate("new device").expect("generate joining device");
    let join_request = joining
        .create_join_request(workspace_id.clone())
        .expect("create join request");
    let submit_uri = format!("/workspaces/{workspace_id}/devices/join-requests");
    let submit_body = serde_json::to_vec(&join_request).expect("serialize join request");

    let submit_response = app
        .clone()
        .oneshot(signed_request(
            Method::POST,
            &submit_uri,
            submit_body.clone(),
            &joining,
        ))
        .await
        .expect("submit join request");
    assert_eq!(submit_response.status(), StatusCode::CREATED);

    let duplicate_response = app
        .clone()
        .oneshot(signed_request(
            Method::POST,
            &submit_uri,
            submit_body,
            &joining,
        ))
        .await
        .expect("submit duplicate join request");
    assert_eq!(duplicate_response.status(), StatusCode::OK);

    let second_join_request = joining
        .create_join_request(workspace_id.clone())
        .expect("create second join request for same device");
    assert_ne!(second_join_request.request_id, join_request.request_id);
    let duplicate_device_response = app
        .clone()
        .oneshot(signed_request(
            Method::POST,
            &submit_uri,
            serde_json::to_vec(&second_join_request).expect("serialize second join request"),
            &joining,
        ))
        .await
        .expect("submit duplicate device join request");
    assert_eq!(duplicate_device_response.status(), StatusCode::OK);
    let duplicate_device_body = to_bytes(duplicate_device_response.into_body(), usize::MAX)
        .await
        .expect("read duplicate device response body");
    let duplicate_device: rustsync_protocol::JoinRequestSubmissionResponse =
        serde_json::from_slice(&duplicate_device_body).expect("duplicate device response is json");
    assert_eq!(duplicate_device.request_id, join_request.request_id);

    let list_response = app
        .clone()
        .oneshot(signed_request(Method::GET, &submit_uri, Vec::new(), &owner))
        .await
        .expect("list join requests");
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body = to_bytes(list_response.into_body(), usize::MAX)
        .await
        .expect("read list body");
    let listed: rustsync_protocol::ListJoinRequestsResponse =
        serde_json::from_slice(&list_body).expect("list response is json");
    assert_eq!(listed.requests, vec![join_request.clone()]);

    let event = signed_device_join_event(
        &workspace_id,
        1,
        &owner,
        &join_request,
        WorkspaceRole::Member,
        "approve_member",
    );
    let approve_uri = format!(
        "/workspaces/{workspace_id}/devices/join-requests/{}/approval",
        join_request.request_id
    );
    let approve_body = serde_json::to_vec(&rustsync_protocol::ApproveJoinRequestRequest {
        join_request_id: join_request.request_id.clone(),
        event,
    })
    .expect("serialize approve request");
    let approve_response = app
        .clone()
        .oneshot(signed_request(
            Method::POST,
            &approve_uri,
            approve_body,
            &owner,
        ))
        .await
        .expect("approve join request");
    assert_eq!(approve_response.status(), StatusCode::OK);

    let head_response = app
        .oneshot(signed_request(
            Method::GET,
            "/workspaces/workspace_test/head",
            Vec::new(),
            &joining,
        ))
        .await
        .expect("joined device fetches head");
    assert_eq!(head_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn join_request_rejects_uninitialized_workspace() {
    let (app, _temp) = app_with_temp_storage().await;
    let workspace_id = WorkspaceId::parse("workspace_missing").expect("valid workspace id");
    let joining = DeviceIdentity::generate("new device").expect("generate joining device");
    let join_request = joining
        .create_join_request(workspace_id.clone())
        .expect("create join request");
    let submit_uri = format!("/workspaces/{workspace_id}/devices/join-requests");
    let body = serde_json::to_vec(&join_request).expect("serialize join request");

    let response = app
        .oneshot(signed_request(Method::POST, &submit_uri, body, &joining))
        .await
        .expect("submit join request");
    assert_error_response(response, StatusCode::BAD_REQUEST, "invalid_request").await;
}

#[tokio::test]
async fn join_request_rejects_tampered_signature_and_wrong_workspace() {
    let (app, _temp, _owner) = app_with_initialized_workspace().await;
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let joining = DeviceIdentity::generate("new device").expect("generate joining device");
    let mut join_request = joining
        .create_join_request(workspace_id.clone())
        .expect("create join request");
    join_request.signature[0] ^= 0xff;
    let submit_uri = format!("/workspaces/{workspace_id}/devices/join-requests");
    let body = serde_json::to_vec(&join_request).expect("serialize join request");
    let response = app
        .clone()
        .oneshot(signed_request(Method::POST, &submit_uri, body, &joining))
        .await
        .expect("submit tampered join request");
    assert_error_response(response, StatusCode::BAD_REQUEST, "invalid_access_state").await;

    let wrong_workspace_uri = "/workspaces/workspace_other/devices/join-requests";
    let valid_request = joining
        .create_join_request(workspace_id)
        .expect("create valid join request");
    let body = serde_json::to_vec(&valid_request).expect("serialize join request");
    let response = app
        .oneshot(signed_request(
            Method::POST,
            wrong_workspace_uri,
            body,
            &joining,
        ))
        .await
        .expect("submit wrong workspace join request");
    assert_error_response(response, StatusCode::BAD_REQUEST, "invalid_access_state").await;
}

#[tokio::test]
async fn stale_access_event_revision_is_rejected_after_prior_approval() {
    let (app, _temp, owner) = app_with_initialized_workspace().await;
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let submit_uri = format!("/workspaces/{workspace_id}/devices/join-requests");

    let first = DeviceIdentity::generate("first joining").expect("generate first joining");
    let first_join = first
        .create_join_request(workspace_id.clone())
        .expect("create first join request");
    app.clone()
        .oneshot(signed_request(
            Method::POST,
            &submit_uri,
            serde_json::to_vec(&first_join).expect("serialize first join"),
            &first,
        ))
        .await
        .expect("submit first join request");

    let second = DeviceIdentity::generate("second joining").expect("generate second joining");
    let second_join = second
        .create_join_request(workspace_id.clone())
        .expect("create second join request");
    app.clone()
        .oneshot(signed_request(
            Method::POST,
            &submit_uri,
            serde_json::to_vec(&second_join).expect("serialize second join"),
            &second,
        ))
        .await
        .expect("submit second join request");

    let first_event = signed_device_join_event(
        &workspace_id,
        1,
        &owner,
        &first_join,
        WorkspaceRole::Member,
        "first_stale_revision_test",
    );
    let first_approve_uri = format!(
        "/workspaces/{workspace_id}/devices/join-requests/{}/approval",
        first_join.request_id
    );
    let first_approve = app
        .clone()
        .oneshot(signed_request(
            Method::POST,
            &first_approve_uri,
            serde_json::to_vec(&rustsync_protocol::ApproveJoinRequestRequest {
                join_request_id: first_join.request_id,
                event: first_event,
            })
            .expect("serialize first approval"),
            &owner,
        ))
        .await
        .expect("approve first join request");
    assert_eq!(first_approve.status(), StatusCode::OK);

    let stale_second_event = signed_device_join_event(
        &workspace_id,
        1,
        &owner,
        &second_join,
        WorkspaceRole::Member,
        "second_stale_revision_test",
    );
    let second_approve_uri = format!(
        "/workspaces/{workspace_id}/devices/join-requests/{}/approval",
        second_join.request_id
    );
    let stale_second_approve = app
        .oneshot(signed_request(
            Method::POST,
            &second_approve_uri,
            serde_json::to_vec(&rustsync_protocol::ApproveJoinRequestRequest {
                join_request_id: second_join.request_id,
                event: stale_second_event,
            })
            .expect("serialize stale second approval"),
            &owner,
        ))
        .await
        .expect("approve stale second join request");
    assert_error_response(
        stale_second_approve,
        StatusCode::BAD_REQUEST,
        "invalid_access_state",
    )
    .await;
}

#[tokio::test]
async fn member_without_manage_devices_cannot_list_or_approve_join_requests() {
    let (app, _temp, owner) = app_with_initialized_workspace().await;
    let workspace_id = WorkspaceId::parse("workspace_test").expect("valid workspace id");
    let member = DeviceIdentity::generate("member device").expect("generate member");
    let member_join = member
        .create_join_request(workspace_id.clone())
        .expect("create member join request");
    let submit_uri = format!("/workspaces/{workspace_id}/devices/join-requests");
    app.clone()
        .oneshot(signed_request(
            Method::POST,
            &submit_uri,
            serde_json::to_vec(&member_join).expect("serialize member join"),
            &member,
        ))
        .await
        .expect("submit member join request");
    let member_event = signed_device_join_event(
        &workspace_id,
        1,
        &owner,
        &member_join,
        WorkspaceRole::Member,
        "member",
    );
    let approve_uri = format!(
        "/workspaces/{workspace_id}/devices/join-requests/{}/approval",
        member_join.request_id
    );
    app.clone()
        .oneshot(signed_request(
            Method::POST,
            &approve_uri,
            serde_json::to_vec(&rustsync_protocol::ApproveJoinRequestRequest {
                join_request_id: member_join.request_id.clone(),
                event: member_event,
            })
            .expect("serialize approve member"),
            &owner,
        ))
        .await
        .expect("approve member");

    let other_joining = DeviceIdentity::generate("other joining").expect("generate joining");
    let other_join = other_joining
        .create_join_request(workspace_id.clone())
        .expect("create other join request");
    app.clone()
        .oneshot(signed_request(
            Method::POST,
            &submit_uri,
            serde_json::to_vec(&other_join).expect("serialize other join"),
            &other_joining,
        ))
        .await
        .expect("submit other join request");

    let list_response = app
        .clone()
        .oneshot(signed_request(
            Method::GET,
            &submit_uri,
            Vec::new(),
            &member,
        ))
        .await
        .expect("member lists join requests");
    assert_error_response(list_response, StatusCode::FORBIDDEN, "permission_denied").await;

    let other_event = signed_device_join_event(
        &workspace_id,
        2,
        &member,
        &other_join,
        WorkspaceRole::Member,
        "member_approve_denied",
    );
    let approve_uri = format!(
        "/workspaces/{workspace_id}/devices/join-requests/{}/approval",
        other_join.request_id
    );
    let approve_response = app
        .oneshot(signed_request(
            Method::POST,
            &approve_uri,
            serde_json::to_vec(&rustsync_protocol::ApproveJoinRequestRequest {
                join_request_id: other_join.request_id,
                event: other_event,
            })
            .expect("serialize denied approve"),
            &member,
        ))
        .await
        .expect("member approves join request");
    assert_error_response(approve_response, StatusCode::FORBIDDEN, "permission_denied").await;
}
