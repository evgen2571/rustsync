use rustsync_protocol::AccessState;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use super::AccessResult;

pub fn save_access_state(path: impl AsRef<Path>, state: &AccessState) -> AccessResult<()> {
    state.validate()?;

    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let bytes = toml::to_string_pretty(&state)?.into_bytes();
    atomic_write(path, &bytes)
}

pub fn load_access_state(path: impl AsRef<Path>) -> AccessResult<Option<AccessState>> {
    let path = path.as_ref();

    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(path)?;
    let state: AccessState = toml::from_str(&text)?;

    state.validate()?;
    Ok(Some(state))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> AccessResult<()> {
    let temporary_path = temporary_path(path);

    let result = (|| {
        let mut file = fs::File::create(&temporary_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary_path, path)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }

    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut temporary_path = path.to_path_buf();

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!("{value}.tmp"))
        .unwrap_or_else(|| "tmp".to_string());

    temporary_path.set_extension(extension);
    temporary_path
}
