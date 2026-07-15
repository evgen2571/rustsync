mod layout;
mod local_engine;

pub use layout::WorkspaceLayout;
pub use local_engine::{
    ApplyReport, LocalWorkspaceEngine, LocalWorkspaceError, LocalWorkspaceResult, StageReport,
    StageSummary, StagedBlob, WorkingTreeStatus, staged_blob_path,
};
use rustsync_protocol::{
    AccessState,
    id::{ACCESS_EVENT_ID_PREFIX, AccessEventId},
};

use crate::access::save_access_state;
use crate::device::{
    DeviceIdentity, DeviceRegistry, save_device_registry, save_local_device_identity,
};
use crate::encryption;
use crate::keyring::WorkspaceKey;
pub use crate::keyring::{KeyVisibility, WorkspaceKeyring, validate_key_id};

pub(crate) use crate::error::{WorkspaceError, WorkspaceResult};

use rustsync_protocol::{
    AccessEvent, DeviceId, DeviceStatus, EncryptedObject, KeyId, SYSTEM_KEY_ID, SignedAccessEvent,
    UnixTimestamp, WorkspaceId,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

pub const WORKSPACE_DIR: &str = ".rustsync";
pub const ACTIVE_KEY_ID: &str = "main";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub workspace_id: WorkspaceId,
    pub local_device_id: DeviceId,
    pub default_key_id: KeyId,
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
            });
        }

        fs::create_dir_all(&layout.keys_dir)?;

        let workspace_id = WorkspaceId::parse(format!("workspace_{}", Uuid::new_v4().simple(),))?;

        let default_key_id = KeyId::parse(SYSTEM_KEY_ID)?;

        let config = WorkspaceConfig {
            workspace_id: workspace_id.clone(),
            local_device_id: owner_device_id.clone(),
            default_key_id: default_key_id.clone(),
        };

        let config_text = toml::to_string_pretty(&config)?;
        fs::write(&layout.config_path, config_text)?;

        WorkspaceKeyring::init(
            &layout.keys_dir,
            &layout.keyring_path,
            owner_device_id,
            &default_key_id,
        )?;

        Ok(Self { layout, config })
    }

    pub fn init_with_device_identity(
        root: impl AsRef<Path>,
        owner: &DeviceIdentity,
    ) -> WorkspaceResult<Self> {
        owner.validate()?;

        let workspace = Self::init(root, owner.device_id())?;

        save_local_device_identity(&workspace.layout.device_identity_path, owner)?;

        let registry = DeviceRegistry::with_owner(workspace.workspace_id().clone(), owner)?;
        save_device_registry(&workspace.layout.device_registry_path, &registry)?;

        let access_state = initial_owner_access_state(workspace.workspace_id().clone(), owner)?;
        save_access_state(&workspace.layout.access_control_path, &access_state)?;

        Ok(workspace)
    }

    pub fn bootstrap_with_key(
        root: impl AsRef<Path>,
        identity: &DeviceIdentity,
        access_state: &AccessState,
        key_id: KeyId,
        key_generation: u64,
        workspace_key: &WorkspaceKey,
    ) -> WorkspaceResult<Self> {
        identity.validate()?;
        access_state.validate()?;

        let layout = WorkspaceLayout::new(root);
        if layout.rustsync_dir.exists() {
            return Err(WorkspaceError::AlreadyInitialized {
                path: layout.rustsync_dir,
            });
        }

        let cleanup_path = layout.rustsync_dir.clone();
        let result = (|| -> WorkspaceResult<Self> {
            let local_device = access_state.active_device_record(identity.device_id())?;
            if local_device.signing_public_key != *identity.signing_public_key()
                || local_device.exchange_public_key != *identity.exchange_public_key()
            {
                return Err(WorkspaceError::Device(
                    crate::error::DeviceError::SigningKeyMismatch,
                ));
            }

            fs::create_dir_all(&layout.keys_dir)?;
            let config = WorkspaceConfig {
                workspace_id: access_state.workspace_id().clone(),
                local_device_id: identity.device_id().clone(),
                default_key_id: key_id.clone(),
            };
            fs::write(&layout.config_path, toml::to_string_pretty(&config)?)?;

            let mut keyring = WorkspaceKeyring::open(&layout.keys_dir, &layout.keyring_path)?;
            keyring.import_key(
                crate::keyring::WorkspaceKeyRecord {
                    key_id,
                    generation: key_generation,
                    visibility: KeyVisibility::Shared,
                    algorithm: crate::keyring::KeyAlgorithm::XChaCha20Poly1305,
                    create_by_device_id: identity.device_id().clone(),
                    created_at: UnixTimestamp::now().as_secs(),
                },
                workspace_key,
            )?;
            save_local_device_identity(&layout.device_identity_path, identity)?;

            let mut registry = DeviceRegistry::new(access_state.workspace_id().clone());
            for membership in access_state.all_memberships() {
                if let Some(record) = access_state.device_record(&membership.device_id) {
                    registry.insert(record.clone())?;
                }
            }
            save_device_registry(&layout.device_registry_path, &registry)?;
            save_access_state(&layout.access_control_path, access_state)?;

            Ok(Self { layout, config })
        })();

        if result.is_err() {
            let _ = fs::remove_dir_all(cleanup_path);
        }
        result
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

    pub fn workspace_id(&self) -> &WorkspaceId {
        &self.config.workspace_id
    }

    pub fn default_key_id(&self) -> &KeyId {
        &self.config.default_key_id
    }

    pub fn local_device_id(&self) -> &DeviceId {
        &self.config.local_device_id
    }

    pub fn keyring(&self) -> WorkspaceResult<WorkspaceKeyring> {
        Ok(WorkspaceKeyring::open(
            &self.layout.keys_dir,
            &self.layout.keyring_path,
        )?)
    }

    pub fn create_key(
        &self,
        key_id: &KeyId,
        visibility: KeyVisibility,
        create_by_device_id: &DeviceId,
    ) -> WorkspaceResult<()> {
        let mut keyring = self.keyring()?;

        keyring.create_key(key_id, visibility, create_by_device_id)?;

        Ok(())
    }

    pub fn load_key(&self, key_id: &KeyId) -> WorkspaceResult<WorkspaceKey> {
        let keyring = self.keyring()?;
        Ok(keyring.load_key(key_id)?)
    }

    pub fn set_active_key(&mut self, key_id: &KeyId) -> WorkspaceResult<()> {
        validate_key_id(key_id)?;

        let keyring = self.keyring()?;

        keyring.get(key_id)?;

        self.config.default_key_id = key_id.clone();
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
    pub fn encrypt_bytes(&self, plaintext: &[u8]) -> WorkspaceResult<EncryptedObject> {
        let key_id = self.workspace.default_key_id();
        let key = self.workspace.load_key(key_id)?;

        Ok(encryption::encrypt(plaintext, key_id, &key)?)
    }

    pub fn decrypt_file(&self, encrypted_file: &EncryptedObject) -> WorkspaceResult<Vec<u8>> {
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

fn initial_owner_access_state(
    workspace_id: WorkspaceId,
    owner: &DeviceIdentity,
) -> WorkspaceResult<AccessState> {
    let event_id = AccessEventId::parse(format!(
        "{ACCESS_EVENT_ID_PREFIX}{}",
        Uuid::new_v4().simple()
    ))?;
    let created_at = UnixTimestamp::now();
    let event = AccessEvent::WorkspaceCreated {
        owner: owner.public_record(DeviceStatus::Active),
    };
    let unsigned = SignedAccessEvent::new_unsigned(
        event_id,
        workspace_id.clone(),
        0,
        owner.device_id().clone(),
        created_at,
        event,
    );
    let signature = owner.sign(&unsigned.signing_payload())?;
    let signed = unsigned.with_signature(signature);

    let mut state = AccessState::empty(workspace_id);
    state.apply_verified_event(&signed)?;

    Ok(state)
}
