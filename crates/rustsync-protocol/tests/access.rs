mod common;

use common::sample_device_record;
use rustsync_protocol::id::AccessEventId;
use rustsync_protocol::{
    AccessEvent, DeviceId, DeviceStatus, JoinRequestId, KeyId, ProtocolError, SignedAccessEvent,
    WorkspaceId, WorkspacePermission, WorkspaceRole,
};

#[test]
fn workspace_roles_and_access_events_validate() {
    assert!(WorkspaceRole::Owner.allows(WorkspacePermission::ManageKeys));
    assert!(WorkspaceRole::Owner.can_manage_access());
    assert!(WorkspaceRole::Member.can_sync());
    assert!(WorkspaceRole::Member.can_manage_access());
    assert!(WorkspaceRole::Member.allows(WorkspacePermission::ManageKeys));

    let (_, owner) = sample_device_record(DeviceStatus::Active);
    let workspace_created = AccessEvent::WorkspaceCreated { owner };
    assert_eq!(workspace_created.required_permission(), None);
    assert!(workspace_created.validate().is_ok());

    let (_, pending_device) = sample_device_record(DeviceStatus::Pending);
    let joined = AccessEvent::DeviceJoined {
        join_request_id: JoinRequestId::parse("join_request123").expect("valid join request id"),
        device: pending_device,
        role: WorkspaceRole::Member,
    };
    assert_eq!(
        joined.required_permission(),
        Some(WorkspacePermission::ManageDevices)
    );
    assert!(joined.validate().is_ok());

    let invalid_key_event = AccessEvent::RestrictedKeyGranted {
        key_id: KeyId::parse("shared_key").expect("valid key id"),
        key_generation: 0,
        device_id: DeviceId::parse("device_test123").expect("valid device id"),
    };
    assert!(matches!(
        invalid_key_event.validate(),
        Err(ProtocolError::InvalidKeyGeneration(0))
    ));
}

#[test]
fn signed_access_event_payload_includes_event_and_identity() {
    let (_, record) = sample_device_record(DeviceStatus::Active);
    let workspace_id = WorkspaceId::parse("workspace_test123").expect("valid workspace id");
    let event_id = AccessEventId::parse("event_test123").expect("valid access event id");
    let signed = SignedAccessEvent::new_unsigned(
        event_id,
        workspace_id,
        7,
        record.device_id.clone(),
        123,
        AccessEvent::DeviceRemoved {
            device_id: record.device_id.clone(),
        },
    )
    .with_signature(vec![1, 2, 3]);

    assert!(!signed.signing_payload().is_empty());
    assert!(signed.validate().is_ok());
}
