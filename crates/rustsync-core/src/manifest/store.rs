use std::{fs, io};

use crate::workspace::Workspace;

use super::Manifest;

pub fn manifest_to_json_bytes(manifest: &Manifest) -> io::Result<Vec<u8>> {
    serde_json::to_vec_pretty(manifest)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

pub fn manifest_from_json_bytes(bytes: &[u8]) -> io::Result<Manifest> {
    serde_json::from_slice(bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

pub fn save_manifest(workspace: &Workspace, manifest: &Manifest) -> io::Result<()> {
    let bytes = manifest_to_json_bytes(manifest)?;

    fs::write(&workspace.layout.manifest_path, bytes)
}

pub fn load_manifest(workspace: &Workspace) -> io::Result<Option<Manifest>> {
    let path = &workspace.layout.manifest_path;

    if !path.exists() {
        return Ok(None);
    }

    let bytes = fs::read(path)?;
    let manifest = manifest_from_json_bytes(&bytes)?;

    Ok(Some(manifest))
}
