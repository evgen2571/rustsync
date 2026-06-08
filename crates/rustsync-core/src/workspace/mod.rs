mod layout;

pub use layout::WorkspaceLayout;

pub use crate::encryption::{self, EncryptedFile};
pub use crate::keyring::{KeyVisibility, WorkspaceKey, WorkspaceKeyring, validate_key_id};

pub(crate) use crate::error::{WorkspaceError, WorkspaceResult};

use rustsync_protocol::DeviceId;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

pub const WORKSPACE_DIR: &str = ".rustsync";
pub const ACTIVE_KEY_ID: &str = "main";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub workspace_id: String,
    pub active_key_id: String,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub layout: WorkspaceLayout,
    pub config: WorkspaceConfig,
}

impl Workspace {
    pub fn init(root: impl AsRef<Path>, owner_device_id: &DeviceId) -> WorkspaceResult<Self> {
        let layout = WorkspaceLayout::new(root);

        if layout.rustsync_dir.exists() {
            return Err(WorkspaceError::AlreadyInitialized {
                path: layout.rustsync_dir,
            })?;
        }

        fs::create_dir_all(&layout.keys_dir)?;

        let config = WorkspaceConfig {
            workspace_id: Uuid::new_v4().to_string(),
            active_key_id: ACTIVE_KEY_ID.to_string(),
        };

        let config_text = toml::to_string_pretty(&config)?;
        fs::write(&layout.config_path, config_text)?;

        WorkspaceKeyring::init(
            &layout.keys_dir,
            &layout.keyring_path,
            owner_device_id,
            ACTIVE_KEY_ID,
        )?;

        Ok(Self { layout, config })
    }

    pub fn open(root: impl AsRef<Path>) -> WorkspaceResult<Self> {
        let layout = WorkspaceLayout::new(root);

        if !layout.rustsync_dir.exists() {
            return Err(WorkspaceError::NotInitialized {
                path: layout.rustsync_dir,
            });
        }

        let config_text = fs::read_to_string(&layout.config_path)?;
        let config: WorkspaceConfig = toml::from_str(&config_text)?;

        Ok(Self { layout, config })
    }

    pub fn workspace_id(&self) -> &str {
        &self.config.workspace_id
    }

    pub fn active_key_id(&self) -> &str {
        &self.config.active_key_id
    }

    pub fn keyring(&self) -> WorkspaceResult<WorkspaceKeyring> {
        Ok(WorkspaceKeyring::open(
            &self.layout.keys_dir,
            &self.layout.keyring_path,
        )?)
    }

    pub fn create_key(
        &self,
        key_id: &str,
        visibility: KeyVisibility,
        create_by_device_id: &DeviceId,
    ) -> WorkspaceResult<()> {
        let mut keyring = self.keyring()?;

        keyring.create_key(key_id, visibility, create_by_device_id)?;

        Ok(())
    }

    pub fn load_key(&self, key_id: &str) -> WorkspaceResult<WorkspaceKey> {
        let keyring = self.keyring()?;
        Ok(keyring.load_key(key_id)?)
    }

    pub fn set_active_key(&mut self, key_id: &str) -> WorkspaceResult<()> {
        validate_key_id(key_id)?;

        let keyring = self.keyring()?;

        keyring.get(key_id)?;

        self.config.active_key_id = key_id.to_string();
        self.save_config()?;

        Ok(())
    }

    pub fn save_config(&self) -> WorkspaceResult<()> {
        let config_text = toml::to_string_pretty(&self.config)?;
        fs::write(&self.layout.config_path, config_text)?;
        Ok(())
    }

    pub fn crypto(&self) -> WorkspaceCrypto<'_> {
        WorkspaceCrypto { workspace: self }
    }
}

pub struct WorkspaceCrypto<'a> {
    workspace: &'a Workspace,
}

impl<'a> WorkspaceCrypto<'a> {
    pub fn encrypt_bytes(&self, plaintext: &[u8]) -> WorkspaceResult<EncryptedFile> {
        let key_id = self.workspace.active_key_id();
        let key = self.workspace.load_key(key_id)?;

        Ok(encryption::encrypt(plaintext, key_id, &key)?)
    }

    pub fn decrypt_file(&self, encrypted_file: &EncryptedFile) -> WorkspaceResult<Vec<u8>> {
        let key = self.workspace.load_key(&encrypted_file.key_id)?;

        Ok(encryption::decrypt(encrypted_file, &key)?)
    }
}

pub fn init_workspace(
    root: impl AsRef<Path>,
    owner_device_id: &DeviceId,
) -> WorkspaceResult<Workspace> {
    Workspace::init(root, owner_device_id)
}

pub fn open_workspace(root: impl AsRef<Path>) -> WorkspaceResult<Workspace> {
    Workspace::open(root)
}
