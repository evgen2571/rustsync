use serde::{Deserialize, Serialize};

use super::WorkspacePermission;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRole {
    #[serde(alias = "Owner")]
    Owner,
    #[serde(alias = "Member")]
    Member,
}

impl WorkspaceRole {
    pub fn allows(self, permission: WorkspacePermission) -> bool {
        match self {
            Self::Owner => true,
            Self::Member => matches!(
                permission,
                WorkspacePermission::Sync
                    | WorkspacePermission::ReadDevices
                    | WorkspacePermission::ReadAccessHistory
            ),
        }
    }

    pub fn can_manage_access(self) -> bool {
        self == Self::Owner
    }

    pub fn can_sync(self) -> bool {
        self.allows(WorkspacePermission::Sync)
    }
}
