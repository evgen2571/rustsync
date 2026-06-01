use std::path::{Path, PathBuf};

use super::WORKSPACE_DIR;

#[derive(Debug, Clone)]
pub struct WorkspaceLayout {
    pub root: PathBuf,
    pub rustsync_dir: PathBuf,
    pub config_path: PathBuf,
    pub keys_dir: PathBuf,
    pub main_key_path: PathBuf,
}

impl WorkspaceLayout {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();

        let rustsync_dir = root.join(WORKSPACE_DIR);
        let keys_dir = rustsync_dir.join("keys");

        let config_path = rustsync_dir.join("workspace.toml");

        Self {
            root,
            rustsync_dir,
            config_path,
            keys_dir,
        }
    }
}
