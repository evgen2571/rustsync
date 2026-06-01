mod key;
mod layout;

pub use key::{
    WORKSPACE_KEY_SIZE, WorkspaceKey, generate_workspace_key, load_workspace_key,
    save_workspace_key,
};

pub use layout::WorkspaceLayout;

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
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
    pub fn init(root: impl AsRef<Path>) -> io::Result<Self> {
        let layout = WorkspaceLayout::new(root);

        if layout.rustsync_dir.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "workspace is already initialized",
            ));
        }

        fs::create_dir_all(&layout.keys_dir)?;

        let config = WorkspaceConfig {
            workspace_id: Uuid::new_v4().to_string(),
            active_key_id: ACTIVE_KEY_ID.to_string(),
        };

        let config_text = toml::to_string_pretty(&config)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;

        fs::write(&layout.config_path, config_text)?;

        let key = generate_workspace_key();
        save_workspace_key(&layout.main_key_path, &key)?;

        Ok(Self { layout, config })
    }

    pub fn open(root: impl AsRef<Path>) -> io::Result<Self> {
        let layout = WorkspaceLayout::new(root);

        if !layout.rustsync_dir.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "workspace is not initialized",
            ));
        }

        let config_text = fs::read_to_string(&layout.config_path)?;

        let config: WorkspaceConfig = toml::from_str(&config_text)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;

        Ok(Self { layout, config })
    }

    pub fn active_key_id(&self) -> &str {
        &self.config.active_key_id
    }

    pub fn active_key_path(&self) -> std::path::PathBuf {
        self.layout.key_path(&self.config.active_key_path)
    }

    pub fn load_active_key(&self) -> io::Result<WorkspaceKey> {
        load_workspace_key(self.active_key_path())
    }

    pub fn crypto(&self) -> WorkspaceCrypto<'_> {
        WorkspaceCrypto { workspace: self }
    }
}

pub struct WorkspaceCrypto<'a> {
    workspace: &'a Workspace,
}

impl<'a> WorkspaceCrypto<'a> {
    pub fn encrypt_bytes(&self, plaintext: Vec<u8>) -> Result<EncryptedFile, EncryptionError> {
        let key_id = self.workspace.active_key_id();
        let key = self.workspace.load_active_key()?;

        encryption::encrypt(plaintext, key_id, &key)
    }

    pub fn decrypt_file(&self, encrypted_file: &EncryptedFile) -> Result<Vec<u8>, EncryptionError> {
        let key = self.workspace.load_key(&encrypted_file.key_id)?;

        encryption::decrypt(encrypted_file, &key)
    }
}

pub fn init_workspace(root: impl AsRef<Path>) -> io::Result<Workspace> {
    Workspace::init(root)
}

pub fn open_workspace(root: impl AsRef<Path>) -> io::Result<Workspace> {
    Workspace::open(root)
}
