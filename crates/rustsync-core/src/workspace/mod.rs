mod key;
mod layout;

pub use key::{
    WORKSPACE_KEY_SIZE, WorkspaceKey, generate_workspace_key, load_workspace_key,
    save_workspace_key,
};

pub use layout::WorkspaceLayout;

use crate::encryption::{self, EncryptedFile};
use crate::error::{Result as CoreResult, WorkspaceError};

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

pub const WORKSPACE_DIR: &str = ".rustsync";
pub const ACTIVE_KEY_ID: &str = "main-key";

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
    pub fn init(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let layout = WorkspaceLayout::new(root);

        if layout.rustsync_dir.exists() {
            return Err(WorkspaceError::AlreadyInitialized {
                path: layout.rustsync_dir,
            });
        }

        fs::create_dir_all(&layout.keys_dir)?;

        let config = WorkspaceConfig {
            workspace_id: Uuid::new_v4().to_string(),
            active_key_id: ACTIVE_KEY_ID.to_string(),
        };

        let config_text = toml::to_string_pretty(&config)?;
        fs::write(&layout.config_path, config_text)?;

        let key = generate_workspace_key();
        save_workspace_key(&layout.main_key_path, &key)?;

        Ok(Self { layout, config })
    }

    pub fn open(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
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

    pub fn load_key(&self, key_id: &str) -> Result<WorkspaceKey, WorkspaceError> {
        validate_key_id(key_id)?;

        let path = self.layout.key_path(key_id);

        if !path.exists() {
            return Err(WorkspaceError::KeyNotFound {
                key_id: key_id.to_string(),
            });
        }

        Ok(load_workspace_key(path)?)
    }

    pub fn crypto(&self) -> WorkspaceCrypto<'_> {
        WorkspaceCrypto { workspace: self }
    }
}

pub struct WorkspaceCrypto<'a> {
    workspace: &'a Workspace,
}

impl<'a> WorkspaceCrypto<'a> {
    pub fn encrypt_bytes(&self, plaintext: &[u8]) -> CoreResult<EncryptedFile> {
        let key_id = self.workspace.active_key_id();
        let key = self.workspace.load_key(key_id)?;

        Ok(encryption::encrypt(plaintext, key_id, &key)?)
    }

    pub fn decrypt_file(&self, encrypted_file: &EncryptedFile) -> CoreResult<Vec<u8>> {
        let key = self.workspace.load_key(&encrypted_file.key_id)?;

        Ok(encryption::decrypt(encrypted_file, &key)?)
    }
}

pub fn init_workspace(root: impl AsRef<Path>) -> Result<Workspace, WorkspaceError> {
    Workspace::init(root)
}

pub fn open_workspace(root: impl AsRef<Path>) -> Result<Workspace, WorkspaceError> {
    Workspace::open(root)
}

fn validate_key_id(key_id: &str) -> Result<(), WorkspaceError> {
    let is_valid = !key_id.is_empty()
        && key_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    if !is_valid {
        return Err(WorkspaceError::InvalidKeyId {
            key_id: key_id.to_string(),
        });
    }

    Ok(())
}
