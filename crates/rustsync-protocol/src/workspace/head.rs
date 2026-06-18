use serde::{Deserialize, Serialize};

use crate::{DeviceId, ManifestId, UnixTimestamp, WorkspaceId};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceHead {
    pub workspace_id: WorkspaceId,
    pub manifest_id: Option<ManifestId>,
    pub revision: u64,
    pub updated_by: Option<DeviceId>,
    pub updated_at: Option<UnixTimestamp>,
}

impl WorkspaceHead {
    pub fn empty(workspace_id: WorkspaceId) -> Self {
        Self {
            workspace_id,
            manifest_id: None,
            revision: 0,
            updated_by: None,
            updated_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateHeadRequest {
    pub expected_revision: u64,
    pub manifest_id: ManifestId,
}
