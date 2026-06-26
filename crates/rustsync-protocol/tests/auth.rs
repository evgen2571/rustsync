mod common;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::Signer;
use rustsync_protocol::{
    DeviceId, DeviceStatus, RequestNonce, UnixTimestamp,
    auth::{
        AuthHeaders, HttpRequestSignatureInput, SignedHttpRequestParts, canonical_request_payload,
        sha256_hex,
    },
    device_signature_payload,
};

use common::sample_device_record;

#[test]
fn canonical_request_payload_is_deterministic() {
    let device_id = DeviceId::parse("device_test").expect("valid device id");
    let nonce = RequestNonce::parse("nonce_test").expect("valid nonce");
    let payload = canonical_request_payload(
        "PUT",
        "/workspaces/workspace_test/blobs/blob_test?part=1",
        "abc123",
        UnixTimestamp::from_secs(123456789),
        &device_id,
        &nonce,
        42,
    );

    assert_eq!(payload, b"rustsync-http-auth\nPUT\n/workspaces/workspace_test/blobs/blob_test?part=1\nabc123\n123456789\ndevice_test\nnonce_test\n42\n");
}

#[test]
fn auth_headers_parse_wire_values() {
    let signature = vec![1, 2, 3, 4];
    let encoded_signature = URL_SAFE_NO_PAD.encode(&signature);
    let headers = AuthHeaders::from_header_values(
        "device_test",
        "123456789",
        "nonce_test",
        &encoded_signature,
    )
    .expect("parse auth headers");

    assert_eq!(
        headers.device_id,
        DeviceId::parse("device_test").expect("valid device id")
    );
    assert_eq!(headers.timestamp, UnixTimestamp::from_secs(123456789));
    assert_eq!(
        headers.nonce,
        RequestNonce::parse("nonce_test").expect("valid nonce")
    );
    assert_eq!(headers.signature, signature);
}

#[test]
fn auth_headers_render_wire_values() {
    let signature = vec![1, 2, 3, 4];
    let headers = AuthHeaders {
        device_id: DeviceId::parse("device_test").expect("valid device id"),
        timestamp: UnixTimestamp::from_secs(123456789),
        nonce: RequestNonce::parse("nonce_test").expect("valid nonce"),
        signature: signature.clone(),
    };

    let values = headers.to_header_values();

    assert_eq!(values.device_id, "device_test");
    assert_eq!(values.timestamp, "123456789");
    assert_eq!(values.nonce, "nonce_test");
    assert_eq!(values.signature, URL_SAFE_NO_PAD.encode(&signature));
}

#[test]
fn signed_http_request_parts_builds_owned_signature_input_from_auth_headers() {
    let body = br#"{"name":"manifest"}"#;
    let signature = vec![9, 8, 7, 6];
    let headers = AuthHeaders {
        device_id: DeviceId::parse("device_test").expect("valid device id"),
        timestamp: UnixTimestamp::from_secs(42),
        nonce: RequestNonce::parse("nonce_test").expect("valid nonce"),
        signature: signature.clone(),
    };

    let signed = SignedHttpRequestParts::new("POST", "/workspaces/workspace_test/manifest", body)
        .from_auth_headers(headers);

    assert_eq!(signed.input.method, "POST");
    assert_eq!(
        signed.input.path_and_query,
        "/workspaces/workspace_test/manifest"
    );
    assert_eq!(signed.input.body_sha256_hex, sha256_hex(body));
    assert_eq!(signed.input.content_length, body.len() as u64);
    assert_eq!(signed.signature, signature);
    assert_eq!(
        signed.canonical_payload(),
        canonical_request_payload(
            "POST",
            "/workspaces/workspace_test/manifest",
            &sha256_hex(body),
            UnixTimestamp::from_secs(42),
            &DeviceId::parse("device_test").expect("valid device id"),
            &RequestNonce::parse("nonce_test").expect("valid nonce"),
            body.len() as u64,
        )
    );
}

#[test]
fn signed_http_request_parts_builds_signature_input_for_client_signing() {
    let body = b"hello";
    let device_id = DeviceId::parse("device_test").expect("valid device id");
    let timestamp = UnixTimestamp::from_secs(42);
    let nonce = RequestNonce::parse("nonce_test").expect("valid nonce");

    let input = SignedHttpRequestParts::new("PUT", "/workspaces/workspace_test/blob", body)
        .signature_input(device_id.clone(), timestamp, nonce.clone());

    assert_eq!(input.method, "PUT");
    assert_eq!(input.path_and_query, "/workspaces/workspace_test/blob");
    assert_eq!(input.body_sha256_hex, sha256_hex(body));
    assert_eq!(input.timestamp, timestamp);
    assert_eq!(input.device_id, device_id);
    assert_eq!(input.nonce, nonce);
    assert_eq!(input.content_length, body.len() as u64);
}

#[test]
fn signed_http_request_verifies_with_device_record() {
    let (signing_key, device) = sample_device_record(DeviceStatus::Active);
    let input = HttpRequestSignatureInput {
        method: "PUT".to_string(),
        path_and_query: "/workspaces/workspace_test/blobs/blob_test?part=1".to_string(),
        body_sha256_hex: sha256_hex(b"hello"),
        timestamp: UnixTimestamp::from_secs(123456789),
        device_id: device.device_id.clone(),
        nonce: RequestNonce::parse("nonce_test").expect("valid nonce"),
        content_length: 5,
    };
    let canonical_payload = input.canonical_payload();
    let signature = signing_key
        .sign(&device_signature_payload(&canonical_payload))
        .to_bytes()
        .to_vec();

    let signed = input.with_signature(signature);

    assert!(signed.verify_with_device(&device).is_ok());
}

#[test]
fn signed_http_request_rejects_tampered_payload_or_wrong_device() {
    let (signing_key, device) = sample_device_record(DeviceStatus::Active);
    let (_other_signing_key, mut other_device) = sample_device_record(DeviceStatus::Active);
    other_device.device_id = DeviceId::parse("device_other").expect("valid device id");
    let input = HttpRequestSignatureInput {
        method: "PUT".to_string(),
        path_and_query: "/workspaces/workspace_test/blobs/blob_test".to_string(),
        body_sha256_hex: sha256_hex(b"hello"),
        timestamp: UnixTimestamp::from_secs(123456789),
        device_id: device.device_id.clone(),
        nonce: RequestNonce::parse("nonce_test").expect("valid nonce"),
        content_length: 5,
    };
    let canonical_payload = input.canonical_payload();
    let signature = signing_key
        .sign(&device_signature_payload(&canonical_payload))
        .to_bytes()
        .to_vec();
    let mut tampered = input.with_signature(signature);

    tampered.input.path_and_query = "/workspaces/workspace_test/blobs/other_blob".to_string();
    assert!(tampered.verify_with_device(&device).is_err());
    assert!(tampered.verify_with_device(&other_device).is_err());
}

#[test]
fn request_nonce_rejects_empty_or_unsafe_values() {
    assert!(RequestNonce::parse("").is_err());
    assert!(RequestNonce::parse("nonce with spaces").is_err());
    assert!(RequestNonce::parse("nonce/slash").is_err());
}
