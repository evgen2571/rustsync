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
fn ignore_rules_exclude_new_paths_but_keep_tracked_files() {
    let temp = tempdir().unwrap();
    let workspace = init_workspace(temp.path());
    let engine = LocalWorkspaceEngine::new(workspace);
    fs::write(temp.path().join("tracked.log"), b"tracked").unwrap();
    engine.stage_all().unwrap();
    fs::write(
        temp.path().join(".rustsyncignore"),
        "# generated files\n*.log\n!keep.log\n/target/\n**/cache/\n\\#secret\n",
    )
    .unwrap();
    for path in [
        "target/build",
        "src/cache/data",
        "nested/target/keep",
        "new.log",
        "keep.log",
        "#secret",
        ".rustsync-tmp-abandoned",
    ] {
        let path = temp.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"contents").unwrap();
    }
    let report = engine.stage_all().unwrap();
    for path in [
        "tracked.log",
        "keep.log",
        "nested/target/keep",
        ".rustsyncignore",
    ] {
        assert!(report.manifest.contains_path(path), "must include {path}");
    }
    for path in [
        "new.log",
        "target",
        "target/build",
        "src/cache",
        "src/cache/data",
        "#secret",
        ".rustsync-tmp-abandoned",
    ] {
        assert!(!report.manifest.contains_path(path), "must ignore {path}");
    }
}

#[test]
fn pull_preserves_ignored_files_when_removing_their_parent_directory() {
    let temp = tempdir().unwrap();
    let workspace = init_workspace(temp.path());
    fs::write(temp.path().join(".rustsyncignore"), "*.log\n").unwrap();
    fs::create_dir(temp.path().join("build")).unwrap();
    fs::write(temp.path().join("build/local.log"), b"local").unwrap();
    fs::write(temp.path().join("build/stale"), b"remove").unwrap();
    let mut remote = Manifest::new(workspace.workspace_id().clone());
    remote
        .insert(".rustsyncignore".into(), file_entry(b"*.log\n"))
        .unwrap();
    LocalWorkspaceEngine::new(workspace)
        .apply_pulled_manifest(&remote, |_| {
            panic!("unchanged ignore file needs no download");
            #[allow(unreachable_code)]
            Ok::<_, std::io::Error>(Vec::new())
        })
        .unwrap();
    assert_eq!(
        fs::read(temp.path().join("build/local.log")).unwrap(),
        b"local"
    );
    assert!(!temp.path().join("build/stale").exists());
}

#[test]
fn pull_refuses_to_overwrite_ignored_local_files_before_deleting_anything() {
    let temp = tempdir().unwrap();
    let workspace = init_workspace(temp.path());
    fs::write(temp.path().join(".rustsyncignore"), "private\n").unwrap();
    fs::write(temp.path().join("private"), b"local secret").unwrap();
    fs::write(temp.path().join("stale"), b"preserve on error").unwrap();
    let mut remote = Manifest::new(workspace.workspace_id().clone());
    remote
        .insert("private".into(), file_entry(b"remote"))
        .unwrap();
    let result = LocalWorkspaceEngine::new(workspace)
        .apply_pulled_manifest(&remote, |_| Ok::<_, std::io::Error>(b"remote".to_vec()));
    assert!(result.is_err(), "must refuse ignored path collision");
    assert_eq!(
        fs::read(temp.path().join("private")).unwrap(),
        b"local secret"
    );
    assert!(temp.path().join("stale").exists());
}

#[cfg(unix)]
#[test]
fn pull_replaces_file_without_truncating_open_handles_or_hard_links() {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    let root = temp.path().join("workspace");
    let workspace = init_workspace(&root);
    let target = root.join("script");
    fs::write(&target, b"old contents").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o750)).unwrap();
    let alias = temp.path().join("outside-link");
    fs::hard_link(&target, &alias).unwrap();
    let mut opened = fs::File::open(&target).unwrap();
    let mut remote = Manifest::new(workspace.workspace_id().clone());
    remote
        .insert("script".into(), file_entry(b"complete new contents"))
        .unwrap();

    LocalWorkspaceEngine::new(workspace)
        .apply_pulled_manifest(&remote, |_| {
            Ok::<_, std::io::Error>(b"complete new contents".to_vec())
        })
        .unwrap();

    let mut old = Vec::new();
    opened.read_to_end(&mut old).unwrap();
    assert_eq!(old, b"old contents");
    assert_eq!(fs::read(alias).unwrap(), b"old contents");
    assert_eq!(fs::read(&target).unwrap(), b"complete new contents");
    assert_eq!(
        fs::metadata(target).unwrap().permissions().mode() & 0o777,
        0o750
    );
    assert_eq!(
        fs::read_dir(root).unwrap().count(),
        2,
        "no temporary files left behind"
    );
}

#[test]
fn pull_preserves_unchanged_file_metadata_and_fetches_only_changed_contents() {
    let temp = tempdir().unwrap();
    let workspace = init_workspace(temp.path());
    let unchanged = temp.path().join("unchanged.txt");
    fs::write(&unchanged, b"keep").unwrap();
    fs::write(temp.path().join("changed.txt"), b"old").unwrap();
    let modified = std::time::UNIX_EPOCH + std::time::Duration::from_secs(123);
    fs::File::options()
        .write(true)
        .open(&unchanged)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let mut remote = Manifest::new(workspace.workspace_id().clone());
    remote
        .insert("unchanged.txt".into(), file_entry(b"keep"))
        .unwrap();
    remote
        .insert("changed.txt".into(), file_entry(b"new"))
        .unwrap();
    let fetched = Cell::new(0);
    let report = LocalWorkspaceEngine::new(workspace)
        .apply_pulled_manifest(&remote, |requested| {
            assert_eq!(
                requested,
                hash(b"new"),
                "unchanged content must use the local file"
            );
            fetched.set(fetched.get() + 1);
            Ok::<_, std::io::Error>(b"new".to_vec())
        })
        .unwrap();
    assert_eq!(fetched.get(), 1);
    assert_eq!(report.written_files, 1);
    assert_eq!(
        fs::metadata(&unchanged).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(fs::read(unchanged).unwrap(), b"keep");
    assert_eq!(fs::read(temp.path().join("changed.txt")).unwrap(), b"new");
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

#[test]
fn apply_rejects_a_file_with_children_before_removing_local_files() {
    let temp = tempdir().unwrap();
    let workspace = init_workspace(temp.path());
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    fs::write(temp.path().join("precious"), b"keep me").unwrap();
    let mut manifest = Manifest::new(workspace.workspace_id().clone());
    manifest
        .insert("parent".into(), file_entry(b"file"))
        .unwrap();
    manifest
        .insert("parent/child".into(), file_entry(b"file"))
        .unwrap();
    assert!(
        engine
            .apply_pulled_manifest(&manifest, |_| Ok::<_, std::io::Error>(b"file".to_vec()))
            .is_err()
    );
    assert_eq!(fs::read(temp.path().join("precious")).unwrap(), b"keep me");
}

#[cfg(unix)]
#[test]
fn apply_replaces_symlinks_without_writing_outside_workspace() {
    let temp = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("precious"), b"keep me").unwrap();
    let workspace = init_workspace(temp.path());
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    std::os::unix::fs::symlink(outside.path().join("precious"), temp.path().join("link")).unwrap();
    let mut manifest = Manifest::new(workspace.workspace_id().clone());
    manifest
        .insert("link".into(), file_entry(b"remote"))
        .unwrap();
    engine
        .apply_pulled_manifest(&manifest, |_| Ok::<_, std::io::Error>(b"remote".to_vec()))
        .unwrap();
    assert_eq!(
        fs::read(outside.path().join("precious")).unwrap(),
        b"keep me"
    );
    assert_eq!(fs::read(temp.path().join("link")).unwrap(), b"remote");
    assert!(
        !fs::symlink_metadata(temp.path().join("link"))
            .unwrap()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn stage_rejects_symlinks_instead_of_uploading_external_contents() {
    let temp = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("secret"), b"secret").unwrap();
    let workspace = init_workspace(temp.path());
    std::os::unix::fs::symlink(outside.path().join("secret"), temp.path().join("link")).unwrap();
    let engine = LocalWorkspaceEngine::new(workspace);
    assert!(engine.stage_all().is_err());
}

#[test]
fn apply_rejects_incorrect_file_sizes_before_removing_local_files() {
    let temp = tempdir().unwrap();
    let workspace = init_workspace(temp.path());
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    fs::write(temp.path().join("precious"), b"keep me").unwrap();
    let mut manifest = Manifest::new(workspace.workspace_id().clone());
    manifest.insert("a".into(), file_entry(b"file")).unwrap();
    manifest
        .insert(
            "b".into(),
            ManifestEntry::file(123, hash(b"file"), UnixTimestamp::from_secs(0)),
        )
        .unwrap();
    assert!(
        engine
            .apply_pulled_manifest(&manifest, |_| Ok::<_, std::io::Error>(b"file".to_vec()))
            .is_err()
    );
    assert_eq!(fs::read(temp.path().join("precious")).unwrap(), b"keep me");
}
