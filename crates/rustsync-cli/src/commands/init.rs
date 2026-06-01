use rustsync_core::workspace::Workspace;

use std::error::Error;
use std::path::PathBuf;

pub fn run(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::init(&path)?;

    println!("initialized rustsync workspace");
    println!("workspace id: {}", workspace.config.workspace_id);
    println!("path: {}", workspace.layout.rustsync_dir.display());

    Ok(())
}
