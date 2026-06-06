use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{AccessControl, AccessResult};

pub fn save_access_control(path: impl AsRef<Path>, access: &AccessControl) -> AccessResult<()> {
    access.validate()?;

    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let text = toml::to_string_pretty(access)?;

    atomic_write(path, text.as_bytes())
}

pub fn load_access_control(path: impl AsRef<Path>) -> AccessResult<Option<AccessControl>> {
    let path = path.as_ref();

    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(path)?;
    let access: AccessControl = toml::from_str(&text)?;

    access.validate()?;

    Ok(Some(access))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> AccessResult<()> {
    let temporary_path = temporary_path(path);

    fs::write(&temporary_path, bytes)?;
    fs::rename(&temporary_path, path)?;

    Ok(())
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
