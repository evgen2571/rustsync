use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{DeviceError, DeviceIdentity, DeviceRegistry, DeviceResult};

pub const DEVICE_IDENTITY_FILE_NAME: &str = "device.identity.toml";
pub const DEVICE_REGISTRY_FILE_NAME: &str = "devices.toml";

pub fn save_local_device_identity(
    path: impl AsRef<Path>,
    identity: &DeviceIdentity,
) -> DeviceResult<()> {
    identity.validate()?;

    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let text = toml::to_string_pretty(identity)?;

    write_private_new_file(path, text.as_bytes())?;

    Ok(())
}

pub fn load_local_device_identity(path: impl AsRef<Path>) -> DeviceResult<Option<DeviceIdentity>> {
    let path = path.as_ref();

    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(path)?;
    let identity: DeviceIdentity = toml::from_str(&text)?;

    identity.validate()?;

    Ok(Some(identity))
}

pub fn save_device_registry(path: impl AsRef<Path>, registry: &DeviceRegistry) -> DeviceResult<()> {
    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let text = toml::to_string_pretty(registry)?;
    atomic_write(path, text.as_bytes())?;

    Ok(())
}

pub fn load_device_registry(path: impl AsRef<Path>) -> DeviceResult<Option<DeviceRegistry>> {
    let path = path.as_ref();

    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(path)?;
    let registry: DeviceRegistry = toml::from_str(&text)?;

    Ok(Some(registry))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> DeviceResult<()> {
    let tmp_path = temporary_path(path);

    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, path)?;

    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!("{extension}.tmp"))
        .unwrap_or_else(|| "tmp".to_string());

    tmp.set_extension(extension);
    tmp
}

fn write_private_new_file(path: &Path, bytes: &[u8]) -> DeviceResult<()> {
    #[cfg(unix)]
    {
        use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt};

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;

        file.write_all(bytes)?;

        Ok(())
    }

    #[cfg(not(unix))]
    {
        use std::{fs::OpenOptions, io::Write};

        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;

        file.write_all(bytes)?;

        Ok(())
    }
}
