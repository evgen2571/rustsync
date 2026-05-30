use crate::{encryption::encrypt, error::EncError};
use chrono::Utc;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum EntryKind {
    File,
    Dir,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Metadata {
    pub name: String,
    pub file_id: String,
    pub owner_id: String,
    pub original_size: u64,
    pub hash: String,
    pub encription: EncriptionInfo,
    pub upload_time: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EncriptionInfo {
    pub key_id: String,
    pub nonce: String,
}

#[derive(Debug)]
pub struct EncryptedPackage {
    pub metadata: Metadata,
    pub enc_file: File,
}

impl EncryptedPackage {
    pub fn new(
        name: String,
        // entry_type: EntryKind,
        owner_id: String,
        key_id: String,
        file_path: impl AsRef<Path>,
    ) -> Result<EncryptedPackage, EncError> {
        let chars = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz0123456789!@#$%";
        let nonce: String = (0..10)
            .map(|_| {
                let idx = rand::thread_rng().gen_range(0..chars.len());
                chars.chars().nth(idx).unwrap()
            })
            .collect();

        let enc_file = encrypt(&file_path, &key_id, &nonce)?;

        let mut hasher = Sha256::new();
        hasher.update(&name);
        let hash = format!("{:x}", hasher.finalize());

        let metadata = Metadata {
            name,
            file_id: "1".to_string(),
            owner_id,
            original_size: File::open(file_path)?.metadata()?.len(),
            hash,
            encription: EncriptionInfo { key_id, nonce },
            upload_time: Utc::now().to_rfc3339(),
        };

        Ok(EncryptedPackage { metadata, enc_file })
    }
}
