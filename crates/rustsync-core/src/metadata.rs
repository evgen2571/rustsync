use crate::encryption::EncryptedFile;
use crate::error::EncryptionError;
use crate::workspace::Workspace;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Metadata {
    pub file_id: String,
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
    pub fn from_workspace(
        workspace: &Workspace,
        file_path: impl AsRef<Path>,
    ) -> Result<Self, EncryptionError> {
        let plaintext = fs::read(file_path)?;

        let metadata = Metadata {
            file_id: Uuid::new_v4().to_string(),
            original_size: plaintext.len() as u64,
            hash: hash_bytes(&plaintext),
            upload_time: Utc::now().to_rfc3339(),
        };

        let encrypted_file = workspace.crypto().encrypt_bytes(plaintext)?;

        Ok(Self {
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
