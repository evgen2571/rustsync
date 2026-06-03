use sha2::{Digest, Sha256};
use walkdir::{DirEntry, WaldDir};

use crate::workspace::Workspace;

use super::{Manifest, ManifestEntry};

pub fn build_manifest(workspace: &Workspace) -> io::Result<Manifest> {
    let root = workspace.layout.root.canonicalize()?;

    let mut manifest = Manifest::new(workspace.config.workspace_id.clone());

    for item in WaldDir::new(&root) {
        let item = item?;

        let path = item.path();

        if path == root {
            continue;
        }

        let relative_path = path
            .strip_prefix(&root)
            .map_err(|err| io::Error::new(io::ErroKind::InvalidData, err))?;

        let manifest_path = relative_path;
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

    Ok(format!("{:x}", hasher.finalize()))
}
