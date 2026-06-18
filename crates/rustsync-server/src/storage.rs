use std::path::PathBuf;

use rustsync_protocol::WorkspaceId;
use tokio::fs;

use crate::error::{ServerError, ServerResult};

#[derive(Clone)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub async fn save_manifest(
        &self,
        workspace_id: &WorkspaceId,
        bytes: &[u8],
    ) -> Result<(), ServerError> {
        validate_id(workspace_id.as_str())?;

        let workspace_dir = self.workspace_dir(workspace_id);
        fs::create_dir_all(&workspace_dir).await?;

        let manifest_path = workspace_dir.join("manifest.enc");
        fs::write(manifest_path, bytes).await?;

        Ok(())
    }

    pub async fn load_manifest(&self, workspace_id: &WorkspaceId) -> ServerResult<Vec<u8>> {
        validate_id(workspace_id.as_str())?;

        let manifest_path = self.workspace_dir(workspace_id).join("manifest.enc");

        match fs::read(manifest_path).await {
            Ok(bytes) => Ok(bytes),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Err(ServerError::ManifestNotFound)
            }
            Err(err) => Err(ServerError::Io(err)),
        }
    }

    fn workspace_dir(&self, workspace_id: &WorkspaceId) -> PathBuf {
        self.root.join("workspaces").join(workspace_id.as_str())
    }

    pub async fn save_blob(&self, blob_id: &str, bytes: &[u8]) -> Result<(), ServerError> {
        validate_id(blob_id)?;

        let blobs_dir = self.blobs_dir();
        fs::create_dir_all(&blobs_dir).await?;

        let blob_path = self.blob_path(blob_id);
        fs::write(blob_path, bytes).await?;

        Ok(())
    }

    pub async fn load_blob(&self, blob_id: &str) -> Result<Vec<u8>, ServerError> {
        validate_id(blob_id)?;

        let blob_path = self.blob_path(blob_id);

        match fs::read(blob_path).await {
            Ok(bytes) => Ok(bytes),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Err(ServerError::BlobNotFound)
            }
            Err(err) => Err(ServerError::Io(err)),
        }
    }

    fn blobs_dir(&self) -> PathBuf {
        self.root.join("blobs")
    }

    fn blob_path(&self, blob_id: &str) -> PathBuf {
        self.blobs_dir().join(format!("{blob_id}.enc"))
    }
}

fn validate_id(id: &str) -> Result<(), ServerError> {
    let is_valid = !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    if is_valid {
        Ok(())
    } else {
        Err(ServerError::InvalidWorkspaceId)
    }
}
