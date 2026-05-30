use rustsync_core::metadata::EncryptedPackage;
// use rustsync_core::scanner::scan_dir;
// use serde::{Deserialize, Serialize};

fn main() {
    // match scan_dir(".") {
    //     Ok(entries) => {
    //         for entry in entries {
    //             println!("{:?} {:?}", entry.kind, entry.relative_path);
    //         }
    //     }
    //     Err(error) => {
    //         eprintln!("Failed to scan folder: {error}");
    //     }
    // }
    match EncryptedPackage::new(
        "TEST".to_string(),
        "owner123".to_string(),
        "23".to_string(),
        ".\\test.txt",
    ) {
        Ok(package) => {
            println!(
                "Encrypted package created successfully: {:?}",
                package.metadata
            );
        }
        Err(error) => {
            eprintln!("Failed to create encrypted package: {error}");
        }
    }
}
