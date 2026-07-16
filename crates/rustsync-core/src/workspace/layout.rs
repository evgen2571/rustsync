use std::path::{Path, PathBuf};

use rustsync_protocol::KeyId;

use super::{ACTIVE_KEY_ID, WORKSPACE_DIR};

#[derive(Debug, Clone)]
pub struct WorkspaceLayout {
    pub root: PathBuf,
    pub rustsync_dir: PathBuf,
    pub config_path: PathBuf,

    pub keys_dir: PathBuf,
    pub keyring_path: PathBuf,
    pub main_key_path: PathBuf,

    pub manifest_path: PathBuf,
    pub sync_state_path: PathBuf,

    pub device_identity_path: PathBuf,
    pub device_registry_path: PathBuf,

    pub access_control_path: PathBuf,
}

impl WorkspaceLayout {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();

        let rustsync_dir = root.join(WORKSPACE_DIR);
        let config_path = rustsync_dir.join("workspace.toml");

        let keys_dir = rustsync_dir.join("keys");
        let keyring_path = keys_dir.join("keyring.toml");
        let main_key_path = keys_dir.join(format!("{ACTIVE_KEY_ID}.key"));

        let manifest_path = rustsync_dir.join("manifest.json");
        let sync_state_path = rustsync_dir.join("sync-state.json");

        let device_identity_path = rustsync_dir.join("device.identity.toml");
        let device_registry_path = rustsync_dir.join("devices.toml");

        let access_control_path = rustsync_dir.join("access.toml");

        Self {
            root,
            rustsync_dir,
            config_path,

            keys_dir,
            keyring_path,
            main_key_path,

            manifest_path,
            sync_state_path,

            device_identity_path,
            device_registry_path,

            access_control_path,
        }
    }

    pub fn key_path(&self, key_id: &KeyId) -> PathBuf {
        self.keys_dir.join(format!("{key_id}.key"))
    }
}
