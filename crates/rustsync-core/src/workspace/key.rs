use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use rand_core::{OsRng, RngCore};

pub const WORKSPACE_KEY_SIZE: usize = 32;

pub fn generate_workspace_key() -> [u8; WORKSPACE_KEY_SIZE] {
    let mut key = [0u8; WORKSPACE_KEY_SIZE];
    OsRng.fill_bytes(&mut key);
    key
}

pub fn save_workspace_key(
    path: impl AsRef<Path>,
    key: &[u8; WORKSPACE_KEY_SIZE],
) -> io::Result<()> {
    let path = path.as_ref();

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

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

pub fn load_workspace_key(path: impl AsRef<Path>) -> io::Result<[u8; WORKSPACE_KEY_SIZE]> {
    let bytes = fs::read(path)?;

    if bytes.len() != WORKSPACE_KEY_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "workspace key must be exactly 32 bytes",
        ));
    }

    let mut key = [0u8; WORKSPACE_KEY_SIZE];
    key.copy_from_slice(&bytes);

    Ok(key)
}
