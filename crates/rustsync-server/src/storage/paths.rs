use std::path::{Path, PathBuf};

pub(crate) fn workspace_dir(root: &Path, workspace_id: &str) -> PathBuf {
    root.join("workspaces").join(workspace_id)
}

pub(crate) fn workspace_state_path(root: &Path, workspace_id: &str) -> PathBuf {
    workspace_dir(root, workspace_id).join("state.sqlite3")
}

/// Only the read-only legacy importer may call this.
pub(crate) fn legacy_root_database_path(root: &Path) -> PathBuf {
    root.join("rustsync.sqlite3")
}

pub(crate) fn objects_dir(root: &Path, workspace_id: &str) -> PathBuf {
    workspace_dir(root, workspace_id).join("objects")
}

pub(crate) fn blob_path(root: &Path, workspace_id: &str, blob_id: &str) -> PathBuf {
    object_path(
        objects_dir(root, workspace_id),
        blob_id.strip_prefix("blob_").unwrap_or(blob_id),
    )
}

pub(crate) fn manifest_path(root: &Path, workspace_id: &str, manifest_id: &str) -> PathBuf {
    object_path(
        objects_dir(root, workspace_id),
        manifest_id.strip_prefix("manifest_").unwrap_or(manifest_id),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_paths_are_workspace_local() {
        let root = Path::new("/storage-root");

        assert_eq!(
            workspace_state_path(root, "workspace_test"),
            PathBuf::from("/storage-root/workspaces/workspace_test/state.sqlite3")
        );
        assert_eq!(
            legacy_root_database_path(root),
            PathBuf::from("/storage-root/rustsync.sqlite3")
        );
        assert_eq!(
            blob_path(root, "workspace_test", "blob_abcdef"),
            PathBuf::from("/storage-root/workspaces/workspace_test/objects/ab/cd/abcdef.enc")
        );
    }
}
