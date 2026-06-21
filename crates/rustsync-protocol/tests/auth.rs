use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rustsync_protocol::{
    DeviceId,
    auth::{AuthHeaders, canonical_request_payload},
};

#[test]
fn canonical_request_payload_is_deterministic() {
    let payload = canonical_request_payload(
        "PUT",
        "/workspaces/workspace_test/blobs/blob_test?part=1",
        "abc123",
        123456789,
        "device_test",
        "request_test",
        42,
    );

    assert_eq!(payload, b"rustsync-http-auth\nPUT\n/workspaces/workspace_test/blobs/blob_test?part=1\nabc123\n123456789\ndevice_test\nrequest_test\n42\n");
}

#[test]
fn auth_headers_parse_wire_values() {
    let signature = vec![1, 2, 3, 4];
    let encoded_signature = URL_SAFE_NO_PAD.encode(&signature);
    let headers = AuthHeaders::from_header_values(
        "device_test",
        "123456789",
        "request_test",
        &encoded_signature,
    )
    .expect("parse auth headers");

    assert_eq!(
        headers.device_id,
        DeviceId::parse("device_test").expect("valid device id")
    );
    assert_eq!(headers.timestamp_unix_seconds, 123456789);
    assert_eq!(headers.request_id, "request_test");
    assert_eq!(headers.signature, signature);
}
