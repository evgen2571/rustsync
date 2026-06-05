mod key;

pub use key::{
    WORKSPACE_KEY_SIZE, WorkspaceKey, generate_workspace_key, load_workspace_key,
    save_workspace_key,
};

pub(crate) use crate::error::{KeyringError, KeyringResult};
