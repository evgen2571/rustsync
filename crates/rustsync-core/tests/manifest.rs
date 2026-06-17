use std::fs;

use rustsync_core::{
    device::DeviceIdentity,
    manifest::{self, build_manifest},
    workspace::Workspace,
};
use tempfile::tempdir;

#[test]
fn build_manifest_excludes_workspace_metadata_directory() {
    let temp = tempdir().expect("temp dir");
    let owner = DeviceIdentity::generate("test laptop").expect("generate device identity");
    let workspace =
        Workspace::init_with_device_identity(temp.path(), &owner).expect("initialize workspace");

    fs::write(temp.path().join("document.txt"), b"hello").expect("write document");
    fs::write(
        workspace.layout.rustsync_dir.join("internal.txt"),
        b"secret metadata",
    )
    .expect("write metadata file");

    let manifest = build_manifest(&workspace).expect("build manfiest");

    assert!(manifest.contains_path("document.txt"));
    assert!(manifest.entries.keys().all(|path| path != ".rustsync"));
    assert!(
        manifest
            .entries
            .keys()
            .all(|path| !path.starts_with(".rustsync/")),
        "manifest must not include workspace metadata paths: {:?}",
        manifest.entries.keys().collect::<Vec<_>>()
    );
}

#[test]
fn build_manifest_uses_normalized_deterministic_relative_paths() {
    let temp = tempdir().expect("temp dir");
    let owner = DeviceIdentity::generate("test laptop").expect("generate device identity");
    let workspace =
        Workspace::init_with_device_identity(temp.path(), &owner).expect("initialize workspace");

    fs::create_dir(temp.path().join("notes")).expect("create notes dir");
    fs::write(temp.path().join("z.txt"), b"z").expect("write z file");
    fs::write(temp.path().join("notes/a.txt"), b"a").expect("write nested file");

    let manifest = build_manifest(&workspace).expect("build manifest");
    let paths = manifest.entries.keys().cloned().collect::<Vec<_>>();

    assert_eq!(paths, vec!["notes", "notes/a.txt", "z.txt"]);
    assert!(matches!(
        manifest.get("notes"),
        Some(manifest::ManifestEntry::Directory(_))
    ));
    assert!(matches!(
        manifest.get("notes/a.txt"),
        Some(manifest::ManifestEntry::File(_))
    ));
    assert!(matches!(
        manifest.get("z.txt"),
        Some(manifest::ManifestEntry::File(_))
    ));
}
