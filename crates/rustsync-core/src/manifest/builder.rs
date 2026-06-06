use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::{Component, Path},
};
use walkdir::WalkDir;

use crate::workspace::Workspace;

use super::{Manifest, ManifestEntry, ManifestError, ManifestResult};

pub fn build_manifest(workspace: &Workspace) -> ManifestResult<Manifest> {
    let root = workspace.layout.root.canonicalize()?;

    let mut manifest = Manifest::new(workspace.config.workspace_id.clone());

    for item in WalkDir::new(&root).into_iter() {
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
        let metadata = fs::metadata(path)?;

        if metadata.is_dir() {
            manifest.insert(manifest_path, ManifestEntry::directory());
            continue;
        }

        if metadata.is_file() {
            let size = metadata.len();
            let content_hash = hash_file(path)?;

            manifest.insert(manifest_path, ManifestEntry::file(size, content_hash))
        }
    }

    Ok(manifest)
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
