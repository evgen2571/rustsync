mod common;

use common::sample_device_record;
use ed25519_dalek::Signer;
use rustsync_protocol::{
    DeviceJoinRequest, DeviceStatus, JoinRequestId, ProtocolError, WorkspaceId,
    device_signature_payload,
};

fn sign_payload(signing_key: &ed25519_dalek::SigningKey, payload: &[u8]) -> Vec<u8> {
    signing_key
        .sign(&device_signature_payload(payload))
        .to_bytes()
        .to_vec()
}

#[test]
fn device_records_and_join_requests_verify_signatures() {
    let (signing_key, record) = sample_device_record(DeviceStatus::Active);
    let message = b"hello";
    let signature = sign_payload(&signing_key, message);

    assert!(record.verify_signature(message, &signature).is_ok());

    let workspace_id = WorkspaceId::parse("workspace_test123").expect("valid workspace id");
    let join_request_id = JoinRequestId::parse("join_request123").expect("valid join request id");
    let request =
        DeviceJoinRequest::new_unsigned(join_request_id, workspace_id.clone(), record, 42);
    let request_signature = sign_payload(&signing_key, &request.signing_payload());
    let request = request.with_signature(request_signature);

    assert!(request.verify().is_ok());
    assert!(request.verify_for_workspace(&workspace_id).is_ok());

    let wrong_workspace = WorkspaceId::parse("workspace_other123").expect("valid workspace id");
    assert!(matches!(
        request.verify_for_workspace(&wrong_workspace),
        Err(ProtocolError::WorkspaceMismatch { .. })
    ))
}
