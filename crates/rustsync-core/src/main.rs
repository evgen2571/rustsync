use rustsync_core::encryption::decrypt;
use rustsync_core::metadata::FilePackage;
use std::fs;
use std::path::Path;

fn main() {
    let test_path = Path::new("test.txt");
    let enc_path = Path::new(test_path).with_extension("enc");

    if let Err(error) = fs::write(test_path, "Hello from RustSync test file!") {
        eprintln!("Failed to create test file: {error}");
        return;
    }

    match FilePackage::new(
        "test.txt".to_string(),
        "owner123".to_string(),
        "main-key".to_string(),
        test_path,
    ) {
        Ok(package) => {
            println!("File package created successfully");

            println!("\nMetadata:");
            println!("Name: {}", package.metadata.name);
            println!("File ID: {}", package.metadata.file_id);
            println!("Owner ID: {}", package.metadata.owner_id);
            println!("Original size: {} bytes", package.metadata.original_size);
            println!("Hash: {}", package.metadata.hash);
            println!("Upload time: {}", package.metadata.upload_time);

            println!("\nEncrypted file:");
            println!("Key ID: {}", package.encrypted_file.key_id);
            println!("Nonce: {:?}", package.encrypted_file.nonce);
            println!(
                "Encrypted data size: {} bytes",
                package.encrypted_file.encrypted_data.len()
            );
            println!(
                "Encrypted data: {:?}",
                package.encrypted_file.encrypted_data
            );

            if let Err(error) = fs::write(enc_path, &package.encrypted_file.encrypted_data) {
                eprintln!("Failed to write encrypted file: {error}");
            } else {
                println!("Encrypted file written successfully");
            }
            match decrypt(
                &package.encrypted_file,
                &package.encrypted_file.key_id,
                &package.encrypted_file.nonce,
            ) {
                Ok(decrypted_data) => {
                    println!("Decrypted data: {:?}", decrypted_data);
                    let path = Path::new("decrypted_test.txt");
                    if let Err(error) = fs::write(path, &decrypted_data) {
                        eprintln!("Failed to write decrypted file: {error}");
                    } else {
                        println!("Decrypted file written successfully");
                    }
                }
                Err(error) => {
                    eprintln!("Failed to decrypt file: {error}");
                }
            }
        }

        Err(error) => {
            eprintln!("Failed to create file package: {error}");
        }
    }
}
