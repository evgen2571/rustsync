use rustsync_core::device::DeviceIdentity;
use rustsync_protocol::{
    AccessEvent, AccessState, DeviceStatus, JoinRequestId, SignedAccessEvent, UnixTimestamp,
    WorkspaceId, WorkspaceRole,
    id::{ACCESS_EVENT_ID_PREFIX, AccessEventId},
};

fn workspace_id() -> WorkspaceId {
    WorkspaceId::parse("workspace_access_test").expect("valid workspace id")
}

fn event_id(suffix: &str) -> AccessEventId {
    AccessEventId::parse(format!("{ACCESS_EVENT_ID_PREFIX}{suffix}")).expect("valid event id")
}

fn signed_event(
    signer: &DeviceIdentity,
    event_id: AccessEventId,
    workspace_id: WorkspaceId,
    expected_revision: u64,
    event: AccessEvent,
) -> SignedAccessEvent {
    let unsigned = SignedAccessEvent::new_unsigned(
        event_id,
        workspace_id,
        expected_revision,
        signer.device_id().clone(),
        UnixTimestamp::from_secs(123),
        event,
    );
    let signature = signer
        .sign(&unsigned.signing_payload())
        .expect("sign access event");

    unsigned.with_signature(signature)
}

fn state_with_owner(owner: &DeviceIdentity) -> AccessState {
    let workspace_id = workspace_id();
    let created = signed_event(
        owner,
        event_id("workspace_created"),
        workspace_id.clone(),
        0,
        AccessEvent::WorkspaceCreated {
            owner: owner.public_record(DeviceStatus::Active),
        },
    );
    let mut state = AccessState::empty(workspace_id);
    state
        .apply_verified_event(&created)
        .expect("apply workspace-created event");

    state
}

#[test]
fn workspace_created_event_stores_owner_device_record() {
    let owner = DeviceIdentity::generate("owner device").expect("generate owner");

    let state = state_with_owner(&owner);

    let stored = state
        .active_device_record(owner.device_id())
        .expect("owner should have active device record");
    assert_eq!(stored, &owner.public_record(DeviceStatus::Active));
    assert_eq!(state.device_record(owner.device_id()), Some(stored));
}

#[test]
fn device_join_event_stores_joined_device_record_as_active() {
    let owner = DeviceIdentity::generate("owner device").expect("generate owner");
    let member = DeviceIdentity::generate("member device").expect("generate member");
    let mut state = state_with_owner(&owner);
    let workspace_id = state.workspace_id().clone();

    let joined = signed_event(
        &owner,
        event_id("device_joined"),
        workspace_id,
        state.revision(),
        AccessEvent::DeviceJoined {
            join_request_id: JoinRequestId::parse("join_request_access_test")
                .expect("valid join request id"),
            device: member.public_record(DeviceStatus::Pending),
            role: WorkspaceRole::Member,
        },
    );

    state
        .apply_verified_event(&joined)
        .expect("apply device-joined event");

    let stored = state
        .active_device_record(member.device_id())
        .expect("member should have active device record");

    assert_eq!(stored.device_id, *member.device_id());
    assert_eq!(stored.status, DeviceStatus::Active);
    assert_eq!(stored.signing_public_key, *member.signing_public_key());
    assert_eq!(stored.exchange_public_key, *member.exchange_public_key());
}

#[test]
fn device_removed_event_revokes_stored_device_record() {
    let owner = DeviceIdentity::generate("owner device").expect("generate owner");
    let member = DeviceIdentity::generate("member device").expect("generate member");
    let mut state = state_with_owner(&owner);
    let workspace_id = state.workspace_id().clone();

    let joined = signed_event(
        &owner,
        event_id("device_joined"),
        workspace_id.clone(),
        state.revision(),
        AccessEvent::DeviceJoined {
            join_request_id: JoinRequestId::parse("join_request_access_test")
                .expect("valid join request id"),
            device: member.public_record(DeviceStatus::Pending),
            role: WorkspaceRole::Member,
        },
    );

    state
        .apply_verified_event(&joined)
        .expect("apply device-joined event");

    let removed = signed_event(
        &owner,
        event_id("device_removed"),
        workspace_id,
        state.revision(),
        AccessEvent::DeviceRemoved {
            device_id: member.device_id().clone(),
        },
    );

    state
        .apply_verified_event(&removed)
        .expect("apply device-removed event");

    assert!(state.active_device_record(member.device_id()).is_err());
    assert_eq!(
        state
            .device_record(member.device_id())
            .expect("removed device record remains for auditing")
            .status,
        DeviceStatus::Revoked
    );
}
