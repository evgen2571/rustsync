use rustsync_core::error::Result;
use rustsync_core::manifest::{build_manifest, load_manifest, save_manifest};
use rustsync_core::workspace::Workspace;

use std::fs;
use std::path::PathBuf;

fn main() -> Result<()> {
    let root = PathBuf::from("target/rustsync-test-workspace");

    if root.exists() {
        fs::remove_dir_all(&root)?;
    }

    fs::create_dir_all(&root)?;

    let test_file = root.join("hello.txt");
    let nested_dir = root.join("notes");
    let nested_file = nested_dir.join("note.txt");

    fs::create_dir_all(&nested_dir)?;

    fs::write(&test_file, b"Hello from RustSync workspace manifest test!")?;

    fs::write(&nested_file, b"This is a nested file.")?;

    let workspace = Workspace::init(&root)?;

    println!("Workspace initialized");
    println!("workspace_id: {}", workspace.workspace_id());
    println!("active_key_id: {}", workspace.active_key_id());

    let opened_workspace = Workspace::open(&root)?;

    let manifest = build_manifest(&opened_workspace)?;

    println!();
    println!("Manifest created");
    println!("manifest workspace_id: {}", manifest.workspace_id);

    println!();
    println!("Manifest entries:");

    for (path, entry) in &manifest.entries {
        println!("{path}: {entry:?}");
    }

    assert!(manifest.contains_path("hello.txt"));
    assert!(manifest.contains_path("notes"));
    assert!(manifest.contains_path("notes/note.txt"));

    save_manifest(&opened_workspace, &manifest)?;

    println!();
    println!(
        "Local manifest saved to: {}",
        opened_workspace.layout.manifest_path.display()
    );

    let loaded_manifest =
        load_manifest(&opened_workspace)?.expect("local manifest should exist after save");

    println!("Local manifest loaded successfully");

    fs::write(
        &test_file,
        b"Hello from RustSync workspace manifest test! Modified version.",
    )?;

    Ok(())
}
