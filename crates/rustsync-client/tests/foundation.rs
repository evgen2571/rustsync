use std::{collections::HashMap, time::Duration};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rustsync_client::{ClientConfig, ClientError, RequestSigner, RustSyncClient};
use rustsync_protocol::{
    ApiErrorCode, ApiErrorResponse, BlobId, DeviceId, ManifestId, ObjectUploadStatus, RequestNonce,
    UnixTimestamp, UpdateHeadRequest, WorkspaceHead, WorkspaceId,
    auth::{
        DEVICE_ID_HEADER, NONCE_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER,
        canonical_request_payload, sha256_hex,
    },
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
    let payload = canonical_request_payload(
        "GET",
        "/health",
        &sha256_hex(b""),
        timestamp,
        signer.device_id(),
        &nonce,
        0,
    );

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
        .expect("bind unused port");
    let base_url = format!("http://{}", listener.local_addr().expect("local addr"));
    drop(listener);
    let client = test_client(&base_url);
    let bytes = b"encrypted blob bytes";
    let workspace_id = WorkspaceId::parse("workspace_test").unwrap();
    let blob_id = BlobId::from_content(bytes);

    let error = client
        .upload_blob(&workspace_id, &blob_id, bytes)
        .await
        .expect_err("closed listener should fail");

    match error {
        ClientError::Network(message) => assert!(!message.is_empty()),
        other => panic!("expected network error, got {other:?}"),
    }
}

fn test_client(base_url: &str) -> RustSyncClient<TestSigner> {
    let config = ClientConfig::new(Url::parse(base_url).expect("base url"));
    let signer = TestSigner {
        device_id: DeviceId::parse("device_test").unwrap(),
    };
    RustSyncClient::new(config, signer)
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

        let timestamp = UnixTimestamp::from_secs(timestamp.parse().expect("timestamp is integer"));
        let nonce = RequestNonce::parse(nonce).expect("nonce is valid");
        let device_id = DeviceId::parse(device_id).expect("device id is valid");
        let canonical = canonical_request_payload(
            "PUT",
            &expected_path,
            &sha256_hex(&expected_body),
            timestamp,
            &device_id,
            &nonce,
            expected_body.len() as u64,
        );
        assert_eq!(signature, &URL_SAFE_NO_PAD.encode(canonical));

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

    let timestamp = UnixTimestamp::from_secs(timestamp.parse().expect("timestamp is integer"));
    let nonce = RequestNonce::parse(nonce).expect("nonce is valid");
    let device_id = DeviceId::parse(device_id).expect("device id is valid");
    let canonical = canonical_request_payload(
        expected_method,
        expected_path,
        &sha256_hex(expected_body),
        timestamp,
        &device_id,
        &nonce,
        expected_body.len() as u64,
    );
    assert_eq!(signature, &URL_SAFE_NO_PAD.encode(canonical));
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
