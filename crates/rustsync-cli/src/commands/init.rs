use rustsync_core::workspace::Workspace;

use std::error::Error;
use std::path::PathBuf;

pub fn run(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let temp_device_id = "temp-device-id";

    let workspace = Workspace::init(&path, temp_device_id)?;

    println!("initialized rustsync workspace");
    println!("workspace id: {}", workspace.config.workspace_id);
    println!("path: {}", workspace.layout.rustsync_dir.display());

    Ok(())
}
