use rustsync_core::workspace::Workspace;
use rustsync_protocol::DeviceId;

use std::error::Error;
use std::path::PathBuf;

pub fn run(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let temp_device_id: DeviceId = "device_temp-device-id".parse()?;

    let workspace = Workspace::init(&path, &temp_device_id)?;

    println!("initialized rustsync workspace");
    println!("workspace id: {}", workspace.config.workspace_id);
    println!("path: {}", workspace.layout.rustsync_dir.display());

    Ok(())
}
