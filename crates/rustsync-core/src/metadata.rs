use crate::encryption::EncryptedFile;
use crate::{encryption::encrypt, error::EncryptionError};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Metadata {
    pub name: String,
    pub file_id: String,
    pub owner_id: String,
    pub original_size: u64,
    pub hash: String,
    pub upload_time: String,
}

#[derive(Debug)]
pub struct FilePackage {
    pub metadata: Metadata,
    pub encrypted_file: EncryptedFile,
}

impl FilePackage {
    pub fn new(
        name: String,
        owner_id: String,
        key_id: String,
        file_path: impl AsRef<Path>,
    ) -> Result<FilePackage, EncryptionError> {
        let path = file_path.as_ref();

        let plain_data = fs::read(path)?;

        let metadata = Metadata {
            name,
            file_id: "1".to_string(), // ?
            owner_id,
            original_size: plain_data.len() as u64,
            hash: hash_bytes(&plain_data),
            upload_time: Utc::now().to_rfc3339(),
        };

        let encrypted_file = encrypt(plain_data, &key_id)?;

        Ok(FilePackage {
            metadata,
            encrypted_file,
        })
    }
}

fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}
