use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePermission {
    ReadObjects,
    WriteObjects,
    UpdateHead,
    ReadDevices,
    ReadAccessHistory,
    ManageDevices,
    ManageRoles,
    ManageKeys,
}
