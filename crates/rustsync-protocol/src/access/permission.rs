use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePermission {
    Sync,
    ReadDevices,
    ReadAccessHistory,
    ManageDevices,
    ManageRoles,
    ManageKeys,
}
