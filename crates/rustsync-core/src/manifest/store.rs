use rustsync_protocol::Manifest;
use std::fs;

use crate::workspace::Workspace;

use super::{ManifestError, ManifestResult};

pub fn manifest_to_json_bytes(manifest: &Manifest) -> ManifestResult<Vec<u8>> {
    serde_json::to_vec_pretty(manifest).map_err(|source| ManifestError::Serialize { source })
}

pub fn manifest_from_json_bytes(bytes: &[u8]) -> ManifestResult<Manifest> {
    serde_json::from_slice(bytes).map_err(|source| ManifestError::Deserialize { source })
}

pub fn save_manifest(workspace: &Workspace, manifest: &Manifest) -> ManifestResult<()> {
    let bytes = manifest_to_json_bytes(manifest)?;

    fs::write(&workspace.layout.manifest_path, bytes)?;

    Ok(())
}

pub fn load_manifest(workspace: &Workspace) -> ManifestResult<Option<Manifest>> {
    let path = &workspace.layout.manifest_path;

    if !path.exists() {
        return Ok(None);
    }

    let bytes = fs::read(path)?;
    let manifest = manifest_from_json_bytes(&bytes)?;

    Ok(Some(manifest))
}

pub fn validate_manifest_workspace(
    workspace: &Workspace,
    manifest: &Manifest,
) -> ManifestResult<()> {
    if manifest.workspace_id != workspace.config.workspace_id {
        return Err(ManifestError::WorkspaceIdMismatch {
            manifest_workspace_id: manifest.workspace_id.clone(),
            current_workspace_id: workspace.config.workspace_id.clone(),
        });
    }

    Ok(())
}
