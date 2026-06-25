use rustsync_core::{device::DeviceIdentity, manifest::save_manifest, workspace::Workspace};
use rustsync_protocol::Manifest;

use std::error::Error;
use std::path::PathBuf;

pub fn run(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let identity = DeviceIdentity::generate("")?;
    let workspace = Workspace::init_with_device_identity(&path, &identity)?;
    let manifest = Manifest::new(workspace.config.workspace_id.clone());
    save_manifest(&workspace, &manifest)?;

    println!("initialized rustsync workspace");
    println!("workspace id: {}", workspace.config.workspace_id);
    println!("device id: {}", identity.device_id());
    println!("device name: {}", identity.device_name());
    println!("device fingerprint: {}", identity.fingerprint());
    println!("path: {}", workspace.layout.rustsync_dir.display());

    Ok(())
}
