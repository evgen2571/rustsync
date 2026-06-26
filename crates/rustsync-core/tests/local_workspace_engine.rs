use std::{cell::Cell, fs, path::Path};

use rustsync_core::{
    device::DeviceIdentity,
    manifest::load_manifest,
    workspace::{LocalWorkspaceEngine, Workspace, staged_blob_path},
};
use rustsync_protocol::{Manifest, ManifestEntry, UnixTimestamp};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn init_workspace(root: &Path) -> Workspace {
    let owner = DeviceIdentity::generate("test laptop").expect("generate device identity");
    Workspace::init_with_device_identity(root, &owner).expect("initialize workspace")
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn file_entry(bytes: &[u8]) -> ManifestEntry {
    ManifestEntry::file(bytes.len() as u64, hash(bytes), UnixTimestamp::from_secs(0))
}

#[test]
fn stage_caches_file_blobs_saves_manifest_and_excludes_rustsync() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    fs::write(temp.path().join("document.txt"), b"hello").expect("write document");
    fs::write(
        workspace.layout.rustsync_dir.join("internal.txt"),
        b"metadata",
    )
    .expect("write metadata");

    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let report = engine.stage_all().expect("stage all");

    assert!(report.manifest.contains_path("document.txt"));
    assert!(
        report
            .manifest
            .entries
            .keys()
            .all(|path| path != ".rustsync" && !path.starts_with(".rustsync/")),
        "manifest must exclude .rustsync paths: {:?}",
        report.manifest.entries.keys().collect::<Vec<_>>()
    );
    let ManifestEntry::File(file) = report.manifest.get("document.txt").expect("document entry")
    else {
        panic!("document should be a file");
    };
    assert_eq!(
        fs::read(staged_blob_path(&workspace, &file.content_hash)).expect("cached blob"),
        b"hello"
    );
    assert_eq!(
        load_manifest(&workspace)
            .expect("load saved manifest")
            .expect("saved manifest"),
        report.manifest
    );
}

#[test]
fn apply_pulled_manifest_removes_stale_preserves_metadata_writes_caches_and_saves() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    fs::write(temp.path().join("stale.txt"), b"stale").expect("write stale");
    fs::write(workspace.layout.rustsync_dir.join("keep.txt"), b"keep").expect("write metadata");

    let bytes = b"remote contents";
    let mut manifest = Manifest::new(workspace.workspace_id().clone());
    manifest
        .insert("dir".to_string(), ManifestEntry::directory())
        .expect("insert dir");
    manifest
        .insert("dir/remote.txt".to_string(), file_entry(bytes))
        .expect("insert file");
    let content_hash = hash(bytes);

    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let report = engine
        .apply_pulled_manifest(&manifest, |requested| {
            assert_eq!(requested, content_hash);
            Ok::<_, std::io::Error>(bytes.to_vec())
        })
        .expect("apply manifest");

    assert!(!temp.path().join("stale.txt").exists());
    assert_eq!(
        fs::read(workspace.layout.rustsync_dir.join("keep.txt")).expect("metadata preserved"),
        b"keep"
    );
    assert_eq!(
        fs::read(temp.path().join("dir/remote.txt")).expect("remote file"),
        bytes
    );
    assert_eq!(
        fs::read(staged_blob_path(&workspace, &content_hash)).expect("cached blob"),
        bytes
    );
    assert_eq!(
        load_manifest(&workspace)
            .expect("load saved manifest")
            .expect("saved manifest"),
        manifest
    );
    assert_eq!(report.removed_paths, 1);
    assert_eq!(report.written_files, 1);
    assert_eq!(report.cached_blobs, 1);
}

#[test]
fn apply_rejects_rustsync_manifest_paths_before_calling_blob_source_and_keeps_metadata() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    fs::write(
        workspace.layout.manifest_path.clone(),
        b"old manifest bytes",
    )
    .expect("metadata");

    let mut manifest = Manifest::new(workspace.workspace_id().clone());
    manifest
        .entries
        .insert(".rustsync/config.toml".to_string(), file_entry(b"bad"));
    let called = Cell::new(false);

    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let error = engine
        .apply_pulled_manifest(&manifest, |_content_hash| {
            called.set(true);
            Ok::<_, std::io::Error>(b"bad".to_vec())
        })
        .expect_err("unsafe path rejected");

    assert!(
        error
            .to_string()
            .contains("unsafe path `.rustsync/config.toml`")
    );
    assert!(!called.get(), "blob source must not be called");
    assert_eq!(
        fs::read(workspace.layout.manifest_path).expect("metadata unchanged"),
        b"old manifest bytes"
    );
}

#[test]
fn apply_rejects_parent_dir_absolute_and_non_normalized_paths_without_outside_writes() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    let outside = temp.path().join("outside.txt");

    for unsafe_path in [
        "../outside.txt".to_string(),
        outside.display().to_string(),
        String::new(),
        ".".to_string(),
        "a//b.txt".to_string(),
        "a\\b.txt".to_string(),
    ] {
        let mut manifest = Manifest::new(workspace.workspace_id().clone());
        manifest
            .entries
            .insert(unsafe_path.clone(), file_entry(b"bad"));

        let engine = LocalWorkspaceEngine::new(workspace.clone());
        let error = engine
            .apply_pulled_manifest(&manifest, |_content_hash| {
                Ok::<_, std::io::Error>(b"bad".to_vec())
            })
            .expect_err("unsafe path rejected");
        assert!(
            error
                .to_string()
                .contains("remote manifest contains unsafe path"),
            "unexpected error: {error}"
        );
        assert!(!outside.exists(), "must not write outside workspace");
    }
}

#[test]
fn apply_pulled_manifest_requests_duplicate_content_hash_once_and_writes_both_files() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    let bytes = b"same contents";
    let content_hash = hash(bytes);
    let mut manifest = Manifest::new(workspace.workspace_id().clone());
    manifest
        .insert("a.txt".to_string(), file_entry(bytes))
        .expect("insert first file");
    manifest
        .insert("b.txt".to_string(), file_entry(bytes))
        .expect("insert second file");
    let calls = Cell::new(0);

    let engine = LocalWorkspaceEngine::new(workspace);
    engine
        .apply_pulled_manifest(&manifest, |requested| {
            assert_eq!(requested, content_hash);
            calls.set(calls.get() + 1);
            Ok::<_, std::io::Error>(bytes.to_vec())
        })
        .expect("apply duplicate blobs");

    assert_eq!(calls.get(), 1, "engine should fetch each unique blob once");
    assert_eq!(fs::read(temp.path().join("a.txt")).expect("a.txt"), bytes);
    assert_eq!(fs::read(temp.path().join("b.txt")).expect("b.txt"), bytes);
}

#[test]
fn staged_blobs_for_manifest_rejects_corrupt_cached_blob() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    fs::write(temp.path().join("document.txt"), b"hello").expect("write document");

    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let manifest = engine.stage_all().expect("stage all").manifest;
    let ManifestEntry::File(file) = manifest.get("document.txt").expect("document entry") else {
        panic!("document should be a file");
    };
    fs::write(staged_blob_path(&workspace, &file.content_hash), b"corrupt")
        .expect("corrupt staged blob");

    let error = engine
        .staged_blobs_for_manifest(&manifest)
        .expect_err("corrupt staged blob rejected");

    assert!(error.to_string().contains("content hash mismatch"));
}

#[test]
fn failed_blob_source_does_not_save_remote_manifest() {
    let temp = tempdir().expect("temp dir");
    let workspace = init_workspace(temp.path());
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    fs::write(temp.path().join("old.txt"), b"old").expect("old file");
    let old_manifest = engine.stage_all().expect("stage old").manifest;

    let mut remote = Manifest::new(workspace.workspace_id().clone());
    remote
        .insert("remote.txt".to_string(), file_entry(b"remote"))
        .expect("insert remote");

    let error = engine
        .apply_pulled_manifest(&remote, |_content_hash| {
            Err::<Vec<u8>, _>(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "missing blob",
            ))
        })
        .expect_err("blob source fails");

    assert!(error.to_string().contains("failed to fetch blob"));
    assert_eq!(
        fs::read(temp.path().join("old.txt")).expect("old file unchanged"),
        b"old",
        "failed apply must not remove local files before blob preflight succeeds"
    );
    assert!(!temp.path().join("remote.txt").exists());
    assert_eq!(
        load_manifest(&workspace)
            .expect("load manifest")
            .expect("saved manifest"),
        old_manifest,
        "failed apply must not save remote manifest"
    );
}
