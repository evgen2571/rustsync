use tempfile::tempdir;

use rustsync_core::{
    access::load_access_state,
    device::{DeviceIdentity, load_device_registry, load_local_device_identity},
    workspace::{Workspace, WorkspaceLayout},
};
use rustsync_protocol::{KeyId, WorkspaceRole};

#[test]
fn workspace_layout_derives_expected_paths() {
    let temp = tempdir().expect("temp dir");
    let layout = WorkspaceLayout::new(temp.path());
    let key_id = KeyId::parse("shared_key").expect("valid key id");

    assert_eq!(layout.root, temp.path());
    assert_eq!(layout.rustsync_dir, temp.path().join(".rustsync"));
    assert_eq!(
        layout.config_path,
        temp.path().join(".rustsync/workspace.toml")
    );
    assert_eq!(
        layout.key_path(&key_id),
        temp.path().join(".rustsync/keys/shared_key.key")
    );
}

#[test]
fn workspace_init_open_and_crypto_round_trip() {
    let temp = tempdir().expect("temp dir");
    let owner = DeviceIdentity::generate("test laptop").expect("generate device identity");

    let workspace =
        Workspace::init_with_device_identity(temp.path(), &owner).expect("initialize workspace");
    assert!(workspace.layout.rustsync_dir.exists());
    assert!(workspace.layout.config_path.exists());
    assert!(workspace.layout.keyring_path.exists());
    assert_eq!(workspace.workspace_id(), &workspace.config.workspace_id);
    assert_eq!(workspace.local_device_id(), owner.device_id());

    let reopened = Workspace::open(temp.path()).expect("open workspace");
    assert_eq!(reopened.workspace_id(), workspace.workspace_id());
    assert_eq!(reopened.default_key_id(), workspace.default_key_id());
    assert_eq!(reopened.local_device_id(), owner.device_id());

    let keyring = reopened.keyring().expect("open keyring");
    assert!(keyring.contains(reopened.default_key_id()));

    let plaintext = b"hello";
    let encrypted = reopened
        .crypto()
        .encrypt_bytes(plaintext)
        .expect("encrypt data");
    let decrypted = reopened
        .crypto()
        .decrypt_file(&encrypted)
        .expect("decrypt data");

    assert_eq!(decrypted, plaintext);
}

#[test]
fn workspace_init_with_device_identity_persists_local_device_state() {
    let temp = tempdir().expect("temp dir");
    let owner = DeviceIdentity::generate("test laptop").expect("generate device identity");

    let workspace =
        Workspace::init_with_device_identity(temp.path(), &owner).expect("initialize workspace");

    let loaded_identity = load_local_device_identity(&workspace.layout.device_identity_path)
        .expect("load device identity")
        .expect("identity should exist");
    assert_eq!(loaded_identity.device_id(), owner.device_id());
    assert_eq!(loaded_identity.device_name(), "test laptop");
    assert_eq!(loaded_identity.fingerprint(), owner.fingerprint());

    let registry = load_device_registry(&workspace.layout.device_registry_path)
        .expect("load device registry")
        .expect("device registry should exist");
    let owner_record = registry
        .require_active(owner.device_id())
        .expect("owner should be active device");
    assert_eq!(owner_record.device_id, *owner.device_id());
    assert_eq!(owner_record.fingerprint, owner.fingerprint());

    let access_state = load_access_state(&workspace.layout.access_control_path)
        .expect("load access state")
        .expect("access state should exist");
    let membership = access_state
        .active_membership(owner.device_id())
        .expect("owner should be active member");
    assert_eq!(membership.role, WorkspaceRole::Owner);
}
