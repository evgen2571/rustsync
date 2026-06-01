mod key;
mod layout;

pub use key::{generate_workspace_key, load_workspace_key, save_workspace_key};
pub use layout::WorkspaceLayout;

use std::fs;
use std::io;
use std::path::Path;

use uuid::Uuid;

pub const WORKSPACE_DIR: &str = ".rustsync";
pub const ACTIVE_KEY_ID: &str = "main-key";

pub fn init_workspace(root: impl AsRef<Path>) -> io::Result<WorkspaceLayout> {
    let layout = WorkspaceLayout::new(root);

    if layout.rustsync_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "workspace is already initialized",
        ));
    }

    fs::create_dir_all(&layout.keys_dir)?;

    let workspace_id = Uuid::new_v4().to_string();

    let config = format!(
        r#"version = 1
        workspace_id = {workspace_id}"#
    );

    fs::write(&layout.config_path, config)?;

    let key = generate_workspace_key();
    save_workspace_key(&layout)
}
