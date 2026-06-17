use tempfile::tempdir;

use rustsync_core::{
    device::DeviceIdentity,
    workspace::{Workspace, WorkspaceLayout},
};
use rustsync_protocol::KeyId;

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

    let workspace = Workspace::init(temp.path(), owner.device_id()).expect("initialize workspace");
    assert!(workspace.layout.rustsync_dir.exists());
    assert!(workspace.layout.config_path.exists());
    assert!(workspace.layout.keyring_path.exists());
    assert_eq!(workspace.workspace_id(), &workspace.config.workspace_id);

    let reopened = Workspace::open(temp.path()).expect("open workspace");
    assert_eq!(reopened.workspace_id(), workspace.workspace_id());
    assert_eq!(reopened.default_key_id(), workspace.default_key_id());

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
