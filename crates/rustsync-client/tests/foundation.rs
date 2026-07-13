use std::{collections::HashMap, time::Duration};

use rustsync_client::{ClientConfig, ClientError, RequestSigner, RustSyncClient};
use rustsync_protocol::{
    AccessEvent, AccessState, ApiErrorCode, ApiErrorResponse, BlobId, CreateWorkspaceRequest,
    DeviceId, DeviceJoinRequest, DeviceRecord, DeviceStatus, JoinRequestId,
    JoinRequestSubmissionStatus, MAX_ENCRYPTED_OBJECT_BYTES, ManifestId, ObjectUploadStatus,
    SignedAccessEvent, UnixTimestamp, UpdateHeadRequest, WorkspaceHead, WorkspaceId, WorkspaceRole,
    auth::{
        AuthHeaders, DEVICE_ID_HEADER, NONCE_HEADER, SIGNATURE_HEADER, SignedHttpRequestParts,
        TIMESTAMP_HEADER,
    },
    fingerprint_from_public_keys,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::sleep,
};
use url::Url;

#[derive(Debug)]
struct TestSigner {
    device_id: DeviceId,
}

impl RequestSigner for TestSigner {
    fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    fn sign(&self, canonical_request: &[u8]) -> rustsync_client::ClientResult<Vec<u8>> {
        Ok(canonical_request.to_vec())
    }
}

#[test]
fn client_can_be_constructed_with_config_and_signer() {
    let config = ClientConfig::new(Url::parse("https://sync.example.test").unwrap());
    let signer = TestSigner {
        device_id: DeviceId::parse("device_test").unwrap(),
    };

    let client = RustSyncClient::new(config.clone(), signer);

    assert_eq!(client.base_url(), config.base_url());
    assert_eq!(client.signer().device_id().as_str(), "device_test");
}

#[test]
fn request_signer_uses_protocol_canonical_payload_bytes() {
    let signer = TestSigner {
        device_id: DeviceId::parse("device_test").unwrap(),
    };
    let nonce = "nonce.test".parse().unwrap();
    let timestamp = UnixTimestamp::from_secs(123);
    let input = SignedHttpRequestParts::new("GET", "/health", b"").signature_input(
        signer.device_id().clone(),
        timestamp,
        nonce,
    );
    let payload = input.canonical_payload();

    let signature = signer.sign(&payload).unwrap();

    assert_eq!(signature, payload);
}

#[test]
fn client_error_can_wrap_protocol_error_response() {
    let response = ApiErrorResponse::new(ApiErrorCode::BlobNotFound, "blob was not found");
    let error = ClientError::Server(response.clone());

    match error {
        ClientError::Server(actual) => assert_eq!(actual, response),
        other => panic!("expected server error, got {other:?}"),
    }
}

#[tokio::test]
async fn upload_blob_sends_signed_put_request_and_returns_upload_status() {
    let bytes = b"encrypted blob bytes";
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let blob_id = BlobId::from_content(bytes);
    let expected_path = format!("/workspaces/{workspace_id}/blobs/{blob_id}");
    let (base_url, server) = spawn_signed_blob_server_once(
        expected_path.clone(),
        bytes.to_vec(),
        "201 Created",
        r#"{"status":"created"}"#,
        None,
    )
    .await;
    let client = test_client(&base_url);

    let response = client
        .upload_blob(&workspace_id, &blob_id, bytes)
        .await
        .expect("upload blob");

    assert_eq!(response.status, ObjectUploadStatus::Created);
    server.await.expect("server task");
}

#[tokio::test]
async fn create_workspace_sends_signed_post_request_and_returns_head() {
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let request = CreateWorkspaceRequest {
        workspace_id: workspace_id.clone(),
        access_state: AccessState::empty(workspace_id.clone()),
    };
    let expected_body = serde_json::to_vec(&request).expect("serialize request");
    let head = WorkspaceHead::empty(workspace_id.clone());
    let body = serde_json::json!({
        "workspace_id": workspace_id,
        "head": head,
    })
    .to_string();
    let (base_url, server) = spawn_signed_json_server_once(
        "POST",
        "/workspaces".to_string(),
        expected_body,
        "201 Created",
        body,
        None,
    )
    .await;
    let client = test_client(&base_url);

    let response = client
        .create_workspace(&request)
        .await
        .expect("create workspace");

    assert_eq!(response.workspace_id, request.workspace_id);
    assert_eq!(response.head, head);
    server.await.expect("server task");
}

#[tokio::test]
async fn submit_join_request_sends_signed_post_and_returns_request_status() {
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let join_request = test_join_request(workspace_id.clone());
    let expected_body = serde_json::to_vec(&join_request).expect("serialize join request");
    let body = serde_json::json!({
        "request_id": join_request.request_id,
        "status": "submitted",
    })
    .to_string();
    let expected_path = format!("/workspaces/{workspace_id}/devices/join-requests");
    let (base_url, server) = spawn_signed_json_server_once(
        "POST",
        expected_path,
        expected_body,
        "202 Accepted",
        body,
        None,
    )
    .await;
    let client = test_client(&base_url);

    let response = client
        .submit_join_request(&join_request)
        .await
        .expect("submit join request");

    assert_eq!(response.request_id, join_request.request_id);
    assert_eq!(response.status, JoinRequestSubmissionStatus::Submitted);
    server.await.expect("server task");
}

#[tokio::test]
async fn join_request_and_access_methods_use_signed_json_routes() {
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let join_request = test_join_request(workspace_id.clone());
    let event = test_access_event(&workspace_id, &join_request);
    let state_body = serde_json::json!({
        "access_state": AccessState::empty(workspace_id.clone()),
    })
    .to_string();

    let list_body = serde_json::json!({"requests": [join_request]}).to_string();
    let (base_url, server) = spawn_signed_json_server_once(
        "GET",
        format!("/workspaces/{workspace_id}/devices/join-requests"),
        Vec::new(),
        "200 OK",
        list_body,
        None,
    )
    .await;
    let client = test_client(&base_url);
    let listed = client
        .list_join_requests(&workspace_id)
        .await
        .expect("list join requests");
    assert_eq!(listed.requests.len(), 1);
    server.await.expect("list server task");

    let request = rustsync_protocol::ApproveJoinRequestRequest {
        join_request_id: listed.requests[0].request_id.clone(),
        event: event.clone(),
    };
    let expected_body = serde_json::to_vec(&request).expect("serialize approval request");
    let (base_url, server) = spawn_signed_json_server_once(
        "POST",
        format!(
            "/workspaces/{workspace_id}/devices/join-requests/{}/approval",
            request.join_request_id
        ),
        expected_body,
        "200 OK",
        state_body.clone(),
        None,
    )
    .await;
    let client = test_client(&base_url);
    let approved = client
        .approve_join_request(&workspace_id, &request.join_request_id, &event)
        .await
        .expect("approve join request");
    assert_eq!(
        approved.access_state,
        AccessState::empty(workspace_id.clone())
    );
    server.await.expect("approval server task");

    let request = rustsync_protocol::ApplyAccessEventRequest {
        event: event.clone(),
    };
    let expected_body = serde_json::to_vec(&request).expect("serialize access event request");
    let (base_url, server) = spawn_signed_json_server_once(
        "POST",
        format!("/workspaces/{workspace_id}/access/events"),
        expected_body,
        "200 OK",
        state_body.clone(),
        None,
    )
    .await;
    let client = test_client(&base_url);
    let applied = client
        .apply_access_event(&workspace_id, &event)
        .await
        .expect("apply access event");
    assert_eq!(
        applied.access_state,
        AccessState::empty(workspace_id.clone())
    );
    server.await.expect("apply server task");

    let (base_url, server) = spawn_signed_json_server_once(
        "GET",
        format!("/workspaces/{workspace_id}/access/state"),
        Vec::new(),
        "200 OK",
        state_body,
        None,
    )
    .await;
    let client = test_client(&base_url);
    let fetched = client
        .fetch_access_state(&workspace_id)
        .await
        .expect("fetch access state");
    assert_eq!(fetched.access_state, AccessState::empty(workspace_id));
    server.await.expect("state server task");
}

#[tokio::test]
async fn download_blob_sends_signed_get_request_and_returns_opaque_bytes() {
    let bytes = b"encrypted blob bytes";
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let blob_id = BlobId::from_content(bytes);
    let expected_path = format!("/workspaces/{workspace_id}/blobs/{blob_id}");
    let (base_url, server) =
        spawn_signed_binary_server_once("GET", expected_path, Vec::new(), "200 OK", bytes, None)
            .await;
    let client = test_client(&base_url);

    let downloaded = client
        .download_blob(&workspace_id, &blob_id)
        .await
        .expect("download blob");

    assert_eq!(downloaded, bytes);
    server.await.expect("server task");
}

#[tokio::test]
async fn download_blob_rejects_an_oversized_response_before_buffering_it() {
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let blob_id = BlobId::from_content(b"expected blob bytes");
    let expected_path = format!("/workspaces/{workspace_id}/blobs/{blob_id}");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let base_url = format!("http://{}", listener.local_addr().expect("local addr"));
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept connection");
        let request = read_http_request(&mut stream).await;
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, expected_path);
        assert_signed_request(&request, "GET", &expected_path, &[]);

        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            MAX_ENCRYPTED_OBJECT_BYTES + 1,
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write response headers");
    });
    let client = test_client(&base_url);

    let error = client
        .download_blob(&workspace_id, &blob_id)
        .await
        .expect_err("oversized response must be rejected");

    assert!(
        matches!(error, ClientError::InvalidResponse(message) if message.contains("byte limit"))
    );
    server.await.expect("server task");
}

#[tokio::test]
async fn upload_manifest_sends_signed_put_request_and_returns_upload_status() {
    let bytes = b"encrypted manifest bytes";
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let manifest_id = ManifestId::from_content(bytes);
    let expected_path = format!("/workspaces/{workspace_id}/manifests/{manifest_id}");
    let (base_url, server) = spawn_signed_blob_server_once(
        expected_path,
        bytes.to_vec(),
        "200 OK",
        r#"{"status":"already_exists"}"#,
        None,
    )
    .await;
    let client = test_client(&base_url);

    let response = client
        .upload_manifest(&workspace_id, &manifest_id, bytes)
        .await
        .expect("upload manifest");

    assert_eq!(response.status, ObjectUploadStatus::AlreadyExists);
    server.await.expect("server task");
}

#[tokio::test]
async fn download_manifest_sends_signed_get_request_and_returns_opaque_bytes() {
    let bytes = b"encrypted manifest bytes";
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let manifest_id = ManifestId::from_content(bytes);
    let expected_path = format!("/workspaces/{workspace_id}/manifests/{manifest_id}");
    let (base_url, server) =
        spawn_signed_binary_server_once("GET", expected_path, Vec::new(), "200 OK", bytes, None)
            .await;
    let client = test_client(&base_url);

    let downloaded = client
        .download_manifest(&workspace_id, &manifest_id)
        .await
        .expect("download manifest");

    assert_eq!(downloaded, bytes);
    server.await.expect("server task");
}

#[tokio::test]
async fn fetch_workspace_head_sends_signed_get_request_and_returns_protocol_head() {
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let manifest_id = ManifestId::from_content(b"encrypted manifest bytes");
    let expected_path = format!("/workspaces/{workspace_id}/head");
    let head = WorkspaceHead {
        workspace_id: workspace_id.clone(),
        manifest_id: Some(manifest_id.clone()),
        revision: 7,
        updated_by: Some(DeviceId::parse("device_other").unwrap()),
        updated_at: Some(UnixTimestamp::from_secs(123_456)),
    };
    let body = serde_json::to_string(&head).expect("serialize head");
    let (base_url, server) =
        spawn_signed_json_server_once("GET", expected_path, Vec::new(), "200 OK", body, None).await;
    let client = test_client(&base_url);

    let fetched = client
        .fetch_workspace_head(&workspace_id)
        .await
        .expect("fetch workspace head");

    assert_eq!(fetched, head);
    server.await.expect("server task");
}

#[tokio::test]
async fn update_workspace_head_sends_signed_put_json_and_returns_updated_head() {
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let manifest_id = ManifestId::from_content(b"encrypted manifest bytes");
    let expected_path = format!("/workspaces/{workspace_id}/head");
    let request = UpdateHeadRequest {
        expected_revision: 3,
        manifest_id: manifest_id.clone(),
    };
    let expected_body = serde_json::to_vec(&request).expect("serialize update request");
    let head = WorkspaceHead {
        workspace_id: workspace_id.clone(),
        manifest_id: Some(manifest_id),
        revision: 4,
        updated_by: Some(DeviceId::parse("device_test").unwrap()),
        updated_at: Some(UnixTimestamp::from_secs(123_456)),
    };
    let body = serde_json::to_string(&head).expect("serialize head");
    let (base_url, server) =
        spawn_signed_json_server_once("PUT", expected_path, expected_body, "200 OK", body, None)
            .await;
    let client = test_client(&base_url);

    let updated = client
        .update_workspace_head(&workspace_id, 3, &request.manifest_id)
        .await
        .expect("update workspace head");

    assert_eq!(updated, head);
    server.await.expect("server task");
}

#[tokio::test]
async fn upload_blob_maps_protocol_error_response() {
    let bytes = b"encrypted blob bytes";
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let blob_id = BlobId::from_content(bytes);
    let expected_path = format!("/workspaces/{workspace_id}/blobs/{blob_id}");
    let body = r#"{"error":"unauthorized_device","message":"device is not authorized"}"#;
    let (base_url, server) = spawn_signed_blob_server_once(
        expected_path,
        bytes.to_vec(),
        "401 Unauthorized",
        body,
        None,
    )
    .await;
    let client = test_client(&base_url);

    let error = client
        .upload_blob(&workspace_id, &blob_id, bytes)
        .await
        .expect_err("upload should fail with protocol error");

    match error {
        ClientError::Server(response) => {
            assert_eq!(response.error, ApiErrorCode::UnauthorizedDevice);
            assert_eq!(response.message, "device is not authorized");
        }
        other => panic!("expected server protocol error, got {other:?}"),
    }

    server.await.expect("server task");
}

#[tokio::test]
async fn upload_blob_reports_invalid_json_response() {
    let bytes = b"encrypted blob bytes";
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let blob_id = BlobId::from_content(bytes);
    let expected_path = format!("/workspaces/{workspace_id}/blobs/{blob_id}");
    let (base_url, server) =
        spawn_signed_blob_server_once(expected_path, bytes.to_vec(), "200 OK", "not-json", None)
            .await;
    let client = test_client(&base_url);

    let error = client
        .upload_blob(&workspace_id, &blob_id, bytes)
        .await
        .expect_err("invalid JSON should fail cleanly");

    match error {
        ClientError::InvalidResponse(message) => {
            assert!(message.contains("decode JSON response"));
        }
        other => panic!("expected invalid response error, got {other:?}"),
    }

    server.await.expect("server task");
}

#[tokio::test]
async fn upload_blob_reports_timeout_separately_from_network_errors() {
    let bytes = b"encrypted blob bytes";
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let blob_id = BlobId::from_content(bytes);
    let expected_path = format!("/workspaces/{workspace_id}/blobs/{blob_id}");
    let (base_url, server) = spawn_signed_blob_server_once(
        expected_path,
        bytes.to_vec(),
        "200 OK",
        r#"{"status":"already_exists"}"#,
        Some(Duration::from_millis(100)),
    )
    .await;
    let config = ClientConfig::new(Url::parse(&base_url).expect("base url"))
        .with_request_timeout(Duration::from_millis(20));
    let signer = TestSigner {
        device_id: DeviceId::parse("device_test").unwrap(),
    };
    let client = RustSyncClient::new(config, signer);

    let error = client
        .upload_blob(&workspace_id, &blob_id, bytes)
        .await
        .expect_err("slow response should time out");

    assert!(matches!(error, ClientError::Timeout));

    server.await.expect("server task");
}

#[tokio::test]
async fn upload_blob_reports_connection_failure_as_network_error() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind reset server");
    let base_url = format!("http://{}", listener.local_addr().expect("local addr"));
    let server = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.expect("accept connection");
    });
    let client = test_client(&base_url);
    let bytes = b"encrypted blob bytes";
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let blob_id = BlobId::from_content(bytes);

    let error = client
        .upload_blob(&workspace_id, &blob_id, bytes)
        .await
        .expect_err("closed connection should fail");

    match error {
        ClientError::Network(message) => assert!(!message.is_empty()),
        other => panic!("expected network error, got {other:?}"),
    }
    server.await.expect("server task");
}

fn test_client(base_url: &str) -> RustSyncClient<TestSigner> {
    let config = ClientConfig::new(Url::parse(base_url).expect("base url"));
    let signer = TestSigner {
        device_id: DeviceId::parse("device_test").unwrap(),
    };
    RustSyncClient::new(config, signer)
}

fn test_join_request(workspace_id: WorkspaceId) -> DeviceJoinRequest {
    DeviceJoinRequest::new_unsigned(
        JoinRequestId::parse("join_test").unwrap(),
        workspace_id,
        test_device_record("device_joining", DeviceStatus::Pending),
        UnixTimestamp::from_secs(123),
    )
    .with_signature(vec![1, 2, 3])
}

fn test_access_event(workspace_id: &WorkspaceId, request: &DeviceJoinRequest) -> SignedAccessEvent {
    SignedAccessEvent::new_unsigned(
        rustsync_protocol::id::AccessEventId::parse("event_test").unwrap(),
        workspace_id.clone(),
        1,
        DeviceId::parse("device_test").unwrap(),
        UnixTimestamp::from_secs(456),
        AccessEvent::DeviceJoined {
            join_request_id: request.request_id.clone(),
            device: request.device.clone(),
            role: WorkspaceRole::Member,
        },
    )
    .with_signature(vec![4, 5, 6])
}

fn test_device_record(device_id: &str, status: DeviceStatus) -> DeviceRecord {
    let signing_public_key = [1; 32];
    let exchange_public_key = [2; 32];
    DeviceRecord {
        device_id: DeviceId::parse(device_id).unwrap(),
        device_name: "test device".to_string(),
        signing_public_key,
        exchange_public_key,
        fingerprint: fingerprint_from_public_keys(&signing_public_key, &exchange_public_key),
        status,
    }
}

async fn spawn_signed_blob_server_once(
    expected_path: String,
    expected_body: Vec<u8>,
    status: &'static str,
    body: &'static str,
    response_delay: Option<Duration>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let base_url = format!("http://{}", listener.local_addr().expect("local addr"));

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept connection");
        let request = read_http_request(&mut stream).await;
        assert_eq!(request.method, "PUT");
        assert_eq!(request.path, expected_path);
        assert_eq!(request.body, expected_body);

        let headers = request.headers;
        let device_id = headers
            .get(DEVICE_ID_HEADER)
            .expect("device id header present");
        let timestamp = headers
            .get(TIMESTAMP_HEADER)
            .expect("timestamp header present");
        let nonce = headers.get(NONCE_HEADER).expect("nonce header present");
        let signature = headers
            .get(SIGNATURE_HEADER)
            .expect("signature header present");
        assert_eq!(device_id, "device_test");

        let headers = AuthHeaders::from_header_values(device_id, timestamp, nonce, signature)
            .expect("auth headers are valid");
        let signed = SignedHttpRequestParts::new("PUT", &expected_path, &expected_body)
            .from_auth_headers(headers);
        assert_eq!(signed.signature, signed.canonical_payload());

        if let Some(delay) = response_delay {
            sleep(delay).await;
        }

        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write response");
    });

    (base_url, server)
}

async fn spawn_signed_binary_server_once(
    expected_method: &'static str,
    expected_path: String,
    expected_body: Vec<u8>,
    status: &'static str,
    body: &'static [u8],
    response_delay: Option<Duration>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let base_url = format!("http://{}", listener.local_addr().expect("local addr"));

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept connection");
        let request = read_http_request(&mut stream).await;
        assert_eq!(request.method, expected_method);
        assert_eq!(request.path, expected_path);
        assert_eq!(request.body, expected_body);
        assert_signed_request(&request, expected_method, &expected_path, &expected_body);

        if let Some(delay) = response_delay {
            sleep(delay).await;
        }

        let response_headers = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/octet-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(response_headers.as_bytes())
            .await
            .expect("write response headers");
        stream.write_all(body).await.expect("write response body");
    });

    (base_url, server)
}

async fn spawn_signed_json_server_once(
    expected_method: &'static str,
    expected_path: String,
    expected_body: Vec<u8>,
    status: &'static str,
    body: String,
    response_delay: Option<Duration>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let base_url = format!("http://{}", listener.local_addr().expect("local addr"));

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept connection");
        let request = read_http_request(&mut stream).await;
        assert_eq!(request.method, expected_method);
        assert_eq!(request.path, expected_path);
        assert_eq!(request.body, expected_body);
        assert_signed_request(&request, expected_method, &expected_path, &expected_body);

        if expected_method == "PUT" {
            let content_type = request
                .headers
                .get("content-type")
                .expect("content type header present");
            assert_eq!(content_type, "application/json");
        }

        if let Some(delay) = response_delay {
            sleep(delay).await;
        }

        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write response");
    });

    (base_url, server)
}

fn assert_signed_request(
    request: &RawRequest,
    expected_method: &str,
    expected_path: &str,
    expected_body: &[u8],
) {
    let headers = &request.headers;
    let device_id = headers
        .get(DEVICE_ID_HEADER)
        .expect("device id header present");
    let timestamp = headers
        .get(TIMESTAMP_HEADER)
        .expect("timestamp header present");
    let nonce = headers.get(NONCE_HEADER).expect("nonce header present");
    let signature = headers
        .get(SIGNATURE_HEADER)
        .expect("signature header present");
    assert_eq!(device_id, "device_test");

    let headers = AuthHeaders::from_header_values(device_id, timestamp, nonce, signature)
        .expect("auth headers are valid");
    let signed = SignedHttpRequestParts::new(expected_method, expected_path, expected_body)
        .from_auth_headers(headers);
    assert_eq!(signed.signature, signed.canonical_payload());
}

struct RawRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> RawRequest {
    let mut received = Vec::new();
    let mut buffer = [0_u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut buffer).await.expect("read request");
        assert_ne!(read, 0, "connection closed before headers");
        received.extend_from_slice(&buffer[..read]);
        if let Some(index) = received.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };

    let headers_text = String::from_utf8_lossy(&received[..header_end]);
    let mut lines = headers_text.split("\r\n");
    let request_line = lines.next().expect("request line present");
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().expect("method present").to_owned();
    let path = request_parts.next().expect("path present").to_owned();

    let mut headers = HashMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').expect("header contains colon");
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
    }

    let content_length = headers
        .get("content-length")
        .map_or(Ok(0), |value| value.parse::<usize>())
        .expect("content-length is usize");
    while received.len() < header_end + content_length {
        let read = stream.read(&mut buffer).await.expect("read request body");
        assert_ne!(read, 0, "connection closed before body");
        received.extend_from_slice(&buffer[..read]);
    }

    RawRequest {
        method,
        path,
        headers,
        body: received[header_end..header_end + content_length].to_vec(),
    }
}
