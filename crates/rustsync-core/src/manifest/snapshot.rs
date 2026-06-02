use serde::{Deserialize, Serialize};

use super::ManifestEntry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub workspace_id: String,
    pub entries: Vec<ManifestEntry>,
}
