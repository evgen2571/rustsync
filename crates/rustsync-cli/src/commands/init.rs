use rustsync_core::{device::DeviceIdentity, workspace::Workspace};

use std::error::Error;
use std::path::PathBuf;

pub fn run(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let identity = DeviceIdentity::generate("")?;
    let workspace = Workspace::init_with_device_identity(&path, &identity)?;

    println!("initialized rustsync workspace");
    println!("workspace id: {}", workspace.config.workspace_id);
    println!("device id: {}", identity.device_id());
    println!("device name: {}", identity.device_name());
    println!("device fingerprint: {}", identity.fingerprint());
    println!("path: {}", workspace.layout.rustsync_dir.display());

    Ok(())
}
