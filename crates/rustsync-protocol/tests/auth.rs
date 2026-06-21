use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rustsync_protocol::{
    DeviceId, RequestNonce, UnixTimestamp,
    auth::{AuthHeaders, canonical_request_payload},
};

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
fn request_nonce_rejects_empty_or_unsafe_values() {
    assert!(RequestNonce::parse("").is_err());
    assert!(RequestNonce::parse("nonce with spaces").is_err());
    assert!(RequestNonce::parse("nonce/slash").is_err());
}
