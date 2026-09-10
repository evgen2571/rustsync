use rustsync_protocol::{Manifest, ManifestEntry, UnixTimestamp};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::{Component, Path},
};
use walkdir::WalkDir;

use crate::workspace::{WORKSPACE_DIR, Workspace};

use super::{ManifestError, ManifestResult};

pub fn build_manifest(workspace: &Workspace) -> ManifestResult<Manifest> {
    let root = workspace.layout.root.canonicalize()?;

    let mut manifest = Manifest::new(workspace.config.workspace_id.clone());

    for item in WalkDir::new(&root)
        .into_iter()
        .filter_entry(|entry| !is_workspace_metadata_dir(entry.path(), &root))
    {
        let item = item?;

        let path = item.path();

        if path == root {
            continue;
        }

        let relative_path =
            path.strip_prefix(&root)
                .map_err(|_| ManifestError::StripRootError {
                    root: root.clone(),
                    path: path.to_path_buf(),
                })?;

        let manifest_path = normalize_relative_path(relative_path)?;
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,
                format!("unsupported workspace entry `{}`: only regular files and directories can be synced", path.display())).into());
        }

        if metadata.is_dir() {
            manifest.insert(manifest_path, ManifestEntry::directory())?;
            continue;
        }

        if metadata.is_file() {
            let size = metadata.len();
            let content_hash = hash_file(path)?;
            let modified_at = modified_at(path, &metadata)?;

            manifest.insert(
                manifest_path,
                ManifestEntry::file(size, content_hash, modified_at),
            )?;
        }
    }

    Ok(manifest)
}

fn is_workspace_metadata_dir(path: &Path, root: &Path) -> bool {
    path != root && path.file_name().is_some_and(|name| name == WORKSPACE_DIR)
}

fn modified_at(path: &Path, metadata: &fs::Metadata) -> ManifestResult<UnixTimestamp> {
    UnixTimestamp::from_system_time(metadata.modified()?).map_err(|_| {
        ManifestError::InvalidModifiedTime {
            path: path.to_path_buf(),
        }
    })
}

fn normalize_relative_path(path: &Path) -> ManifestResult<String> {
    let mut parts = Vec::new();

    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part
                    .to_str()
                    .ok_or_else(|| ManifestError::InvalidUtf8Path {
                        path: path.to_path_buf(),
                    })?;

                parts.push(part.to_string());
            }

            Component::CurDir => {}

            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ManifestError::InvalidRelativePath {
                    path: path.to_path_buf(),
                });
            }
        }
    }

    Ok(parts.join("/"))
}

fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();

    let mut buffer = [0u8; 8 * 1024];

    loop {
        let read = file.read(&mut buffer)?;

        if read == 0 {
            break;
        }

        hasher.update(&buffer[..read]);
    }

    let digest = hasher.finalize();
    Ok(hex::encode(digest))
}
