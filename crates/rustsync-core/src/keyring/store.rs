use rustsync_protocol::{DeviceId, KeyId, UnixTimestamp};
use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{
    KeyAlgorithm, KeyVisibility, KeyringError, KeyringRegistry, KeyringResult, WorkspaceKey,
    WorkspaceKeyRecord, generate_workspace_key, load_workspace_key, save_workspace_key,
};

#[derive(Debug, Clone)]
pub struct WorkspaceKeyring {
    keys_dir: PathBuf,
    registry_path: PathBuf,
    registry: KeyringRegistry,
}

impl WorkspaceKeyring {
    pub fn init(
        keys_dir: impl AsRef<Path>,
        registry_path: impl AsRef<Path>,
        owner_device_id: &DeviceId,
        initial_key_id: &KeyId,
    ) -> KeyringResult<Self> {
        validate_key_id(initial_key_id)?;

        let keys_dir = keys_dir.as_ref().to_path_buf();
        let registry_path = registry_path.as_ref().to_path_buf();

        fs::create_dir_all(&keys_dir)?;

        let mut keyring = Self {
            keys_dir,
            registry_path,
            registry: KeyringRegistry::new(),
        };

        keyring.create_key(initial_key_id, KeyVisibility::Shared, owner_device_id)?;
        keyring.save()?;

        Ok(keyring)
    }

    pub fn open(
        keys_dir: impl AsRef<Path>,
        registry_path: impl AsRef<Path>,
    ) -> KeyringResult<Self> {
        let keys_dir = keys_dir.as_ref().to_path_buf();
        let registry_path = registry_path.as_ref().to_path_buf();

        let registry = if registry_path.exists() {
            let text = fs::read_to_string(&registry_path)?;
            toml::from_str(&text)?
        } else {
            KeyringRegistry::new()
        };

        Ok(Self {
            keys_dir,
            registry_path,
            registry,
        })
    }

    pub fn create_key(
        &mut self,
        key_id: &KeyId,
        visibility: KeyVisibility,
        create_by_device_id: &DeviceId,
    ) -> KeyringResult<WorkspaceKeyRecord> {
        validate_key_id(key_id)?;

        if self.registry.contains(key_id) {
            return Err(KeyringError::KeyAlreadyExists {
                key_id: key_id.clone(),
            });
        }

        let key_path = self.key_path(key_id);

        if key_path.exists() {
            return Err(KeyringError::KeyAlreadyExists {
                key_id: key_id.clone(),
            });
        }

        let key = generate_workspace_key();
        save_workspace_key(&key_path, &key)?;

        let record = WorkspaceKeyRecord {
            key_id: key_id.clone(),
            generation: 1,
            visibility,
            algorithm: KeyAlgorithm::Aes256Gcm,
            create_by_device_id: create_by_device_id.clone(),
            created_at: UnixTimestamp::now().as_secs(),
        };

        self.registry.insert(record.clone());
        self.save()?;

        Ok(record)
    }

    pub fn load_key(&self, key_id: &KeyId) -> KeyringResult<WorkspaceKey> {
        validate_key_id(key_id)?;

        let path = self.key_path(key_id);

        if !path.exists() {
            return Err(KeyringError::KeyNotFound {
                key_id: key_id.clone(),
            });
        }

        load_workspace_key(path)
    }

    pub fn get(&self, key_id: &KeyId) -> KeyringResult<&WorkspaceKeyRecord> {
        self.registry
            .get(key_id)
            .ok_or_else(|| KeyringError::KeyNotFound {
                key_id: key_id.clone(),
            })
    }

    pub fn contains(&self, key_id: &KeyId) -> bool {
        self.registry.contains(key_id)
    }

    pub fn list(&self) -> impl Iterator<Item = &WorkspaceKeyRecord> {
        self.registry.all()
    }

    pub fn save(&self) -> KeyringResult<()> {
        let text = toml::to_string_pretty(&self.registry)?;
        fs::write(&self.registry_path, text)?;
        Ok(())
    }

    pub fn key_path(&self, key_id: &KeyId) -> PathBuf {
        self.keys_dir.join(format!("{key_id}.key"))
    }
}

pub fn validate_key_id(key_id: &KeyId) -> KeyringResult<()> {
    let is_valid = !key_id.as_str().is_empty()
        && key_id
            .as_str()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    if !is_valid {
        return Err(KeyringError::InvalidKeyId {
            key_id: key_id.clone(),
        });
    }

    Ok(())
}
