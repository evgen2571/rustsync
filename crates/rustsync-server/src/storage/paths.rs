use std::path::{Path, PathBuf};

pub(crate) fn workspace_dir(root: &Path, workspace_id: &str) -> PathBuf {
    root.join("workspaces").join(workspace_id)
}

pub(crate) fn blobs_dir(root: &Path, workspace_id: &str) -> PathBuf {
    workspace_dir(root, workspace_id).join("blobs")
}

pub(crate) fn manifests_dir(root: &Path, workspace_id: &str) -> PathBuf {
    workspace_dir(root, workspace_id).join("manifests")
}

pub(crate) fn blob_path(root: &Path, workspace_id: &str, blob_id: &str) -> PathBuf {
    object_path(blobs_dir(root, workspace_id), blob_id)
}

pub(crate) fn manifest_path(root: &Path, workspace_id: &str, manifest_id: &str) -> PathBuf {
    object_path(manifests_dir(root, workspace_id), manifest_id)
}

fn object_path(base_dir: PathBuf, object_id: &str) -> PathBuf {
    let first = object_id.get(0..2).unwrap_or("_");
    let second = object_id.get(2..4).unwrap_or("_");

    base_dir
        .join(first)
        .join(second)
        .join(format!("{object_id}.enc"))
}
