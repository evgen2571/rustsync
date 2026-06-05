use rand::RngCore;
use rand_core::OsRng;
use std::{fs, path::Path};

use super::{KeyringError, KeyringResult};

pub const WORKSPACE_KEY_SIZE: usize = 32;

pub type WorkspaceKey = [u8; WORKSPACE_KEY_SIZE];

pub fn generate_workspace_key() -> WorkspaceKey {
    let mut key = [0u8; WORKSPACE_KEY_SIZE];
    OsRng.fill_bytes(&mut key);
    key
}

pub fn save_workspace_key(path: impl AsRef<Path>, key: &WorkspaceKey) -> KeyringResult<()> {
    let path = path.as_ref();

    #[cfg(unix)]
    {
        use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt};

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;

        file.write_all(key)?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;

        file.write_all(key)?;
        Ok(())
    }
}

pub fn load_workspace_key(path: impl AsRef<Path>) -> KeyringResult<WorkspaceKey> {
    let path = path.as_ref();

    let bytes = fs::read(path)?;

    if bytes.len() != WORKSPACE_KEY_SIZE {
        return Err(KeyringError::InvalidKeySize {
            path: path.to_path_buf(),
            expected: WORKSPACE_KEY_SIZE,
            actual: bytes.len(),
        });
    }

    let mut key = [0u8; WORKSPACE_KEY_SIZE];
    key.copy_from_slice(&bytes);

    Ok(key)
}
