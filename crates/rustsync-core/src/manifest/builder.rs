use rustsync_protocol::{Manifest, ManifestEntry, UnixTimestamp};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::{Component, Path},
};
use walkdir::WalkDir;

use crate::workspace::{Workspace, ignore::IgnoreRules};

use super::{ManifestError, ManifestResult};

pub fn build_manifest(workspace: &Workspace) -> ManifestResult<Manifest> {
    let root = workspace.layout.root.canonicalize()?;
    let ignores = IgnoreRules::load(workspace)?;

    let mut manifest = Manifest::new(workspace.config.workspace_id.clone());

    for item in WalkDir::new(&root).into_iter().filter_entry(|entry| {
        entry.path() == root
            || !ignores.excludes(
                entry
                    .path()
                    .strip_prefix(&root)
                    .expect("walk stays under root"),
                entry.file_type().is_dir(),
            )
    }) {
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

    // Keep genuinely empty directories, but do not publish directories whose
    // only contents are ignored. Otherwise a remote deletion reappears on sync.
    let mut directories: Vec<_> = manifest
        .entries
        .iter()
        .filter_map(|(path, entry)| {
            matches!(entry, ManifestEntry::Directory(_)).then_some(path.clone())
        })
        .collect();
    directories.sort_by_key(|path| std::cmp::Reverse(path.matches('/').count()));
    for path in directories {
        let prefix = format!("{path}/");
        let has_visible_child = manifest
            .entries
            .range(prefix.clone()..)
            .next()
            .is_some_and(|(child, _)| child.starts_with(&prefix));
        if !has_visible_child
            && fs::read_dir(root.join(&path))?
                .next()
                .transpose()?
                .is_some()
        {
            manifest.entries.remove(&path);
        }
    }
    Ok(manifest)
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
