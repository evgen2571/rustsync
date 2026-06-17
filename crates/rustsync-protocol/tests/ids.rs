use rustsync_protocol::id::AccessEventId;
use rustsync_protocol::{DeviceId, JoinRequestId, KeyId, WorkspaceId};

#[test]
fn ids_round_trip_and_serialize() {
    let device_id = DeviceId::parse("device_test123").expect("valid device id");
    let join_request_id = JoinRequestId::parse("join_request123").expect("valid join request id");
    let workspace_id = WorkspaceId::parse("workspace_test123").expect("valid workspace id");
    let key_id = KeyId::parse("shared_key").expect("valid key id");
    let event_id = AccessEventId::parse("event_test123").expect("valid access event id");

    assert_eq!(device_id.as_str(), "device_test123");
    assert_eq!(join_request_id.as_str(), "join_request123");
    assert_eq!(workspace_id.as_str(), "workspace_test123");
    assert_eq!(key_id.as_str(), "shared_key");
    assert_eq!(event_id.as_str(), "event_test123");

    let json = serde_json::to_string(&device_id).expect("serialize device id");
    assert_eq!(json, "\"device_test123\"");
    let decoded: DeviceId = serde_json::from_str(&json).expect("deserialize device id");
    assert_eq!(decoded, device_id);
}
