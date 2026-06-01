use rustsync_core::error::Result;
use rustsync_core::metadata::FilePackage;
use rustsync_core::workspace::Workspace;

use std::fs;
use std::path::PathBuf;

fn main() -> Result<()> {
    let root = PathBuf::from("target/rustsync-test-workspace");

    // Make test repeatable.
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }

    fs::create_dir_all(&root)?;

    let test_file = root.join("hello.txt");
    let original_text = b"Hello from RustSync workspace encryption test!";

    fs::write(&test_file, original_text)?;

    // 1. Initialize workspace.
    let workspace = Workspace::init(&root)?;

    println!("Workspace initialized");
    println!("workspace_id: {}", workspace.config.workspace_id);
    println!("active_key_id: {}", workspace.active_key_id());

    // 2. Create encrypted file package from normal file.
    let package = FilePackage::from_workspace(&workspace, &test_file)?;

    println!();
    println!("File packaged");
    println!("file_id: {}", package.metadata.file_id);
    println!("original_size: {}", package.metadata.original_size);
    println!("hash: {}", package.metadata.hash);
    println!("upload_time: {}", package.metadata.upload_time);
    println!("encrypted key_id: {}", package.encrypted_file.key_id);
    println!("nonce: {}", package.encrypted_file.nonce);
    println!(
        "encrypted size: {} bytes",
        package.encrypted_file.encrypted_data.len()
    );

    // 3. Open workspace again, like another command would do.
    let opened_workspace = Workspace::open(&root)?;

    // 4. Decrypt encrypted file.
    let decrypted = opened_workspace
        .crypto()
        .decrypt_file(&package.encrypted_file)?;

    assert_eq!(decrypted, original_text);

    println!();
    println!("Decryption successful");
    println!("decrypted text: {}", String::from_utf8_lossy(&decrypted));

    Ok(())
}
