use std::fs;

use rustsync_core::{
    access::{open_key_envelope, seal_key_envelope},
    device::{DeviceIdentity, load_device_registry, load_local_device_identity},
    workspace::{KeyVisibility, Workspace, WorkspaceLayout},
};
use rustsync_protocol::{
    AccessEvent, AccessState, DeviceStatus, JoinRequestId, KeyId, SignedAccessEvent, UnixTimestamp,
    WorkspaceId, WorkspaceRole,
    id::{ACCESS_EVENT_ID_PREFIX, AccessEventId},
};
use tempfile::tempdir;

fn workspace_id() -> WorkspaceId {
    WorkspaceId::parse("workspace_envelope_test").expect("valid workspace id")
}

fn event_id(suffix: &str) -> AccessEventId {
    AccessEventId::parse(format!("{ACCESS_EVENT_ID_PREFIX}envelope_{suffix}"))
        .expect("valid access event id")
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

fn state_with_owner_and_recipient(
    owner: &DeviceIdentity,
    recipient: &DeviceIdentity,
) -> AccessState {
    let mut state = AccessState::empty(workspace_id());
    let created = signed_event(
        owner,
        event_id("workspace_created"),
        state.workspace_id().clone(),
        state.revision(),
        AccessEvent::WorkspaceCreated {
            owner: owner.public_record(DeviceStatus::Active),
        },
    );
    state
        .apply_verified_event(&created)
        .expect("apply workspace-created event");

    let joined = signed_event(
        owner,
        event_id("recipient_joined"),
        state.workspace_id().clone(),
        state.revision(),
        AccessEvent::DeviceJoined {
            join_request_id: JoinRequestId::parse("join_request_envelope_recipient")
                .expect("valid join request id"),
            device: recipient.public_record(DeviceStatus::Pending),
            role: WorkspaceRole::Member,
        },
    );
    state
        .apply_verified_event(&joined)
        .expect("apply recipient join event");

    state
}

fn state_with_owner(owner: &DeviceIdentity) -> AccessState {
    let mut state = AccessState::empty(workspace_id());
    let created = signed_event(
        owner,
        event_id("workspace_created"),
        state.workspace_id().clone(),
        state.revision(),
        AccessEvent::WorkspaceCreated {
            owner: owner.public_record(DeviceStatus::Active),
        },
    );
    state
        .apply_verified_event(&created)
        .expect("apply workspace-created event");
    state
}

fn workspace_key_fixture(owner: &DeviceIdentity) -> (tempfile::TempDir, Workspace, KeyId, String) {
    let temp = tempdir().expect("temporary source workspace");
    let workspace =
        Workspace::init(temp.path(), owner.device_id()).expect("initialize source workspace");
    let key_id = workspace.default_key_id().clone();
    let fixture_hex = hex::encode(
        fs::read(workspace.layout.key_path(&key_id)).expect("read workspace-key fixture bytes"),
    );

    (temp, workspace, key_id, fixture_hex)
}

fn assert_secret_absent(text: &str, workspace_key_fixture_hex: &str) {
    assert!(
        !text.contains(workspace_key_fixture_hex),
        "workspace-key fixture appeared in diagnostic output"
    );
}

#[test]
fn key_envelope_opens_only_for_its_intended_recipient() {
    let owner = DeviceIdentity::generate("owner").expect("generate owner");
    let recipient = DeviceIdentity::generate("recipient").expect("generate recipient");
    let state = state_with_owner_and_recipient(&owner, &recipient);
    let (_key_temp, key_workspace, key_id, fixture_hex) = workspace_key_fixture(&owner);
    let workspace_key = key_workspace.load_key(&key_id).expect("load workspace key");
    let owner_record = state
        .active_device_record(owner.device_id())
        .expect("owner record");
    let recipient_record = state
        .active_device_record(recipient.device_id())
        .expect("recipient record");

    let envelope = seal_key_envelope(
        &state,
        KeyVisibility::Shared,
        key_id,
        1,
        &workspace_key,
        &owner,
        owner_record,
        recipient_record,
        UnixTimestamp::from_secs(123),
    )
    .expect("seal key envelope");

    assert_secret_absent(&format!("{envelope:?}"), &fixture_hex);
    open_key_envelope(
        &state,
        KeyVisibility::Shared,
        &envelope,
        &recipient,
        recipient_record,
        owner_record,
    )
    .expect("intended recipient opens envelope");
}

#[test]
fn key_envelope_rejects_tampering_without_leaking_workspace_key_material() {
    let owner = DeviceIdentity::generate("owner").expect("generate owner");
    let recipient = DeviceIdentity::generate("recipient").expect("generate recipient");
    let state = state_with_owner_and_recipient(&owner, &recipient);
    let (_key_temp, key_workspace, key_id, fixture_hex) = workspace_key_fixture(&owner);
    let workspace_key = key_workspace.load_key(&key_id).expect("load workspace key");
    let owner_record = state
        .active_device_record(owner.device_id())
        .expect("owner record");
    let recipient_record = state
        .active_device_record(recipient.device_id())
        .expect("recipient record");
    let envelope = seal_key_envelope(
        &state,
        KeyVisibility::Shared,
        key_id,
        1,
        &workspace_key,
        &owner,
        owner_record,
        recipient_record,
        UnixTimestamp::from_secs(123),
    )
    .expect("seal key envelope");

    let mut changed_workspace = envelope.clone();
    changed_workspace.workspace_id =
        WorkspaceId::parse("workspace_other").expect("valid workspace id");
    let mut changed_recipient = envelope.clone();
    changed_recipient.recipient_device_id = owner.device_id().clone();
    let mut changed_sender = envelope.clone();
    changed_sender.sender_device_id = recipient.device_id().clone();
    let mut changed_signature = envelope.clone();
    changed_signature.signature[0] ^= 1;
    let mut changed_ciphertext = envelope.clone();
    changed_ciphertext.encrypted_workspace_key[0] ^= 1;
    let mut future_revision = envelope;
    future_revision.access_revision += 1;

    for tampered in [
        changed_workspace,
        changed_recipient,
        changed_sender,
        changed_signature,
        changed_ciphertext,
        future_revision,
    ] {
        let error = match open_key_envelope(
            &state,
            KeyVisibility::Shared,
            &tampered,
            &recipient,
            recipient_record,
            owner_record,
        ) {
            Ok(_) => panic!("tampered envelope must be rejected"),
            Err(error) => error,
        };
        assert_secret_absent(&error.to_string(), &fixture_hex);
        assert_secret_absent(&format!("{error:?}"), &fixture_hex);
    }
}

#[test]
fn bootstrap_with_key_writes_usable_workspace_metadata() {
    let owner = DeviceIdentity::generate("bootstrap device").expect("generate owner");
    let state = state_with_owner(&owner);
    let (_key_temp, key_workspace, key_id, _fixture_hex) = workspace_key_fixture(&owner);
    let workspace_key = key_workspace.load_key(&key_id).expect("load workspace key");
    let target = tempdir().expect("temporary bootstrap workspace");

    let workspace = Workspace::bootstrap_with_key(
        target.path(),
        &owner,
        &state,
        key_id.clone(),
        1,
        &workspace_key,
    )
    .expect("bootstrap workspace");

    assert_eq!(workspace.workspace_id(), state.workspace_id());
    assert_eq!(workspace.local_device_id(), owner.device_id());
    assert_eq!(workspace.default_key_id(), &key_id);
    assert!(workspace.keyring().expect("open keyring").contains(&key_id));
    assert_eq!(
        load_local_device_identity(&workspace.layout.device_identity_path)
            .expect("load local device identity")
            .expect("identity metadata exists")
            .device_id(),
        owner.device_id()
    );
    assert!(
        load_device_registry(&workspace.layout.device_registry_path)
            .expect("load device registry")
            .expect("device registry metadata exists")
            .require_active(owner.device_id())
            .is_ok()
    );
    let encrypted = workspace
        .crypto()
        .encrypt_bytes(b"bootstrap metadata is usable")
        .expect("encrypt with bootstrapped key");
    assert_eq!(
        workspace
            .crypto()
            .decrypt_file(&encrypted)
            .expect("decrypt with bootstrapped key"),
        b"bootstrap metadata is usable"
    );
}

#[test]
fn bootstrap_with_key_preserves_existing_workspace_key_on_rejection() {
    let owner = DeviceIdentity::generate("owner").expect("generate owner");
    let state = state_with_owner(&owner);
    let target = tempdir().expect("temporary target workspace");
    let existing =
        Workspace::init(target.path(), owner.device_id()).expect("initialize target workspace");
    let existing_key_id = existing.default_key_id().clone();
    let existing_key_path = existing.layout.key_path(&existing_key_id);
    let existing_key_bytes = fs::read(&existing_key_path).expect("read existing key");
    let (_key_temp, key_workspace, key_id, _fixture_hex) = workspace_key_fixture(&owner);
    let workspace_key = key_workspace.load_key(&key_id).expect("load workspace key");

    Workspace::bootstrap_with_key(target.path(), &owner, &state, key_id, 1, &workspace_key)
        .expect_err("existing workspace must not be replaced");

    assert_eq!(
        fs::read(existing_key_path).expect("read preserved key"),
        existing_key_bytes
    );
}

#[test]
fn bootstrap_with_key_validation_failure_leaves_no_workspace_metadata() {
    let owner = DeviceIdentity::generate("owner").expect("generate owner");
    let outsider = DeviceIdentity::generate("outsider").expect("generate outsider");
    let state = state_with_owner(&owner);
    let (_key_temp, key_workspace, key_id, _fixture_hex) = workspace_key_fixture(&owner);
    let workspace_key = key_workspace.load_key(&key_id).expect("load workspace key");
    let target = tempdir().expect("temporary bootstrap workspace");
    let layout = WorkspaceLayout::new(target.path());

    Workspace::bootstrap_with_key(target.path(), &outsider, &state, key_id, 1, &workspace_key)
        .expect_err("non-member identity must fail bootstrap validation");

    assert!(!layout.rustsync_dir.exists());
}
