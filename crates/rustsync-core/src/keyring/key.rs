use rand::RngCore;
use rand_core::OsRng;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use super::{KeyringError, KeyringResult};

pub const WORKSPACE_KEY_SIZE: usize = 32;

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct WorkspaceKey {
    bytes: [u8; WORKSPACE_KEY_SIZE],
}

impl WorkspaceKey {
    fn new() -> Self {
        Self {
            bytes: [0u8; WORKSPACE_KEY_SIZE],
        }
    }

    pub(crate) fn from_bytes(bytes: [u8; WORKSPACE_KEY_SIZE]) -> Self {
        Self { bytes }
    }

    pub(crate) fn try_from_vec(bytes: Vec<u8>) -> KeyringResult<Self> {
        let actual = bytes.len();
        let bytes = bytes.try_into().map_err(|_| KeyringError::InvalidKeySize {
            path: Path::new("<memory>").to_path_buf(),
            expected: WORKSPACE_KEY_SIZE,
            actual,
        })?;

        Ok(Self::from_bytes(bytes))
    }

    pub(crate) fn expose_secret(&self) -> &[u8; WORKSPACE_KEY_SIZE] {
        &self.bytes
    }

    fn expose_mut(&mut self) -> &mut [u8; WORKSPACE_KEY_SIZE] {
        &mut self.bytes
    }
}

pub(crate) fn generate_workspace_key() -> WorkspaceKey {
    let mut key = WorkspaceKey::new();
    OsRng.fill_bytes(key.expose_mut());
    key
}

pub(crate) fn save_workspace_key(path: impl AsRef<Path>, key: &WorkspaceKey) -> KeyringResult<()> {
    let path = path.as_ref();
    let mut file = open_new_key_file(path)?;

    let write_result = (|| -> io::Result<()> {
        file.write_all(key.expose_secret())?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(error) = write_result {
        drop(file);

        let _ = fs::remove_file(path);

        return Err(error.into());
    }

    Ok(())
}

pub(crate) fn load_workspace_key(path: impl AsRef<Path>) -> KeyringResult<WorkspaceKey> {
    let path = path.as_ref();
    let mut file = File::open(path)?;

    let actual_size = file.metadata()?.len();

    if actual_size != WORKSPACE_KEY_SIZE as u64 {
        return Err(KeyringError::InvalidKeySize {
            path: path.to_path_buf(),
            expected: WORKSPACE_KEY_SIZE,
            actual: usize::try_from(actual_size).unwrap_or(usize::MAX),
        });
    }

    let mut key = WorkspaceKey::new();
    file.read_exact(key.expose_mut())?;

    Ok(key)
}

fn open_new_key_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();

    options.write(true).create_new(true);

    #[cfg(unix)]
    options.mode(0o600);

    options.open(path)
}
