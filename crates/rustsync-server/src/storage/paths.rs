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

pub(crate) fn head_path(root: &Path, workspace_id: &str) -> PathBuf {
    workspace_dir(root, workspace_id).join("head.json")
}

pub(crate) fn access_state_path(root: &Path, workspace_id: &str) -> PathBuf {
    workspace_dir(root, workspace_id)
        .join("access")
        .join("state.json")
}

pub(crate) fn join_requests_dir(root: &Path, workspace_id: &str) -> PathBuf {
    workspace_dir(root, workspace_id)
        .join("devices")
        .join("join-requests")
}

pub(crate) fn join_request_path(root: &Path, workspace_id: &str, join_request_id: &str) -> PathBuf {
    join_requests_dir(root, workspace_id).join(format!("{join_request_id}.json"))
}

fn object_path(base_dir: PathBuf, object_id: &str) -> PathBuf {
    let first = object_id.get(0..2).unwrap_or("_");
    let second = object_id.get(2..4).unwrap_or("_");

    base_dir
        .join(first)
        .join(second)
        .join(format!("{object_id}.enc"))
}
