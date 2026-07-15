use std::fmt;

use crate::{
    BlobId, DeviceId, JoinRequestId, KeyId, ManifestId, ProtocolError, WorkspaceId,
    WorkspacePermission,
};

pub const WORKSPACES_ROUTE: &str = "/workspaces";
pub const WORKSPACE_BLOB_ROUTE: &str = "/workspaces/{workspace_id}/blobs/{blob_id}";
pub const WORKSPACE_MANIFEST_ROUTE: &str = "/workspaces/{workspace_id}/manifests/{manifest_id}";
pub const WORKSPACE_HEAD_ROUTE: &str = "/workspaces/{workspace_id}/head";
pub const WORKSPACE_JOIN_REQUESTS_ROUTE: &str = "/workspaces/{workspace_id}/devices/join-requests";
pub const WORKSPACE_JOIN_REQUEST_APPROVAL_ROUTE: &str =
    "/workspaces/{workspace_id}/devices/join-requests/{join_request_id}/approval";
pub const WORKSPACE_ACCESS_EVENTS_ROUTE: &str = "/workspaces/{workspace_id}/access/events";
pub const WORKSPACE_ACCESS_STATE_ROUTE: &str = "/workspaces/{workspace_id}/access/state";
pub const WORKSPACE_KEY_ENVELOPE_ROUTE: &str = "/workspaces/{workspace_id}/key-envelope";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceSyncMethod {
    Get,
    Put,
}

impl WorkspaceSyncMethod {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Put => "PUT",
        }
    }
}

impl TryFrom<&str> for WorkspaceSyncMethod {
    type Error = WorkspaceSyncRouteClassificationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "GET" => Ok(Self::Get),
            "PUT" => Ok(Self::Put),
            _ => Err(WorkspaceSyncRouteClassificationError::UnsupportedMethod {
                method: value.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceSyncResource {
    Blob(BlobId),
    Manifest(ManifestId),
    Head,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSyncEndpoint {
    pub workspace_id: WorkspaceId,
    pub resource: WorkspaceSyncResource,
}

impl WorkspaceSyncEndpoint {
    #[must_use]
    pub fn blob(workspace_id: WorkspaceId, blob_id: BlobId) -> Self {
        Self {
            workspace_id,
            resource: WorkspaceSyncResource::Blob(blob_id),
        }
    }

    #[must_use]
    pub fn manifest(workspace_id: WorkspaceId, manifest_id: ManifestId) -> Self {
        Self {
            workspace_id,
            resource: WorkspaceSyncResource::Manifest(manifest_id),
        }
    }

    #[must_use]
    pub fn head(workspace_id: WorkspaceId) -> Self {
        Self {
            workspace_id,
            resource: WorkspaceSyncResource::Head,
        }
    }

    #[must_use]
    pub fn relative_path(&self) -> String {
        match &self.resource {
            WorkspaceSyncResource::Blob(blob_id) => {
                format!("workspaces/{}/blobs/{}", self.workspace_id, blob_id)
            }
            WorkspaceSyncResource::Manifest(manifest_id) => {
                format!("workspaces/{}/manifests/{}", self.workspace_id, manifest_id)
            }
            WorkspaceSyncResource::Head => format!("workspaces/{}/head", self.workspace_id),
        }
    }

    #[must_use]
    pub fn absolute_path(&self) -> String {
        format!("/{}", self.relative_path())
    }

    pub fn classify(
        method: WorkspaceSyncMethod,
        path: &str,
    ) -> Result<Option<ClassifiedWorkspaceSyncEndpoint>, WorkspaceSyncRouteClassificationError>
    {
        classify_workspace_sync_route(method, path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceAccessResource {
    JoinRequests,
    JoinRequestApproval(JoinRequestId),
    AccessEvents,
    AccessState,
    KeyEnvelope {
        key_id: KeyId,
        recipient_device_id: DeviceId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAccessEndpoint {
    pub workspace_id: WorkspaceId,
    pub resource: WorkspaceAccessResource,
}

impl WorkspaceAccessEndpoint {
    #[must_use]
    pub fn join_requests(workspace_id: WorkspaceId) -> Self {
        Self {
            workspace_id,
            resource: WorkspaceAccessResource::JoinRequests,
        }
    }

    #[must_use]
    pub fn join_request_approval(
        workspace_id: WorkspaceId,
        join_request_id: JoinRequestId,
    ) -> Self {
        Self {
            workspace_id,
            resource: WorkspaceAccessResource::JoinRequestApproval(join_request_id),
        }
    }

    #[must_use]
    pub fn access_events(workspace_id: WorkspaceId) -> Self {
        Self {
            workspace_id,
            resource: WorkspaceAccessResource::AccessEvents,
        }
    }

    #[must_use]
    pub fn access_state(workspace_id: WorkspaceId) -> Self {
        Self {
            workspace_id,
            resource: WorkspaceAccessResource::AccessState,
        }
    }

    #[must_use]
    pub fn key_envelope(
        workspace_id: WorkspaceId,
        key_id: KeyId,
        recipient_device_id: DeviceId,
    ) -> Self {
        Self {
            workspace_id,
            resource: WorkspaceAccessResource::KeyEnvelope {
                key_id,
                recipient_device_id,
            },
        }
    }

    #[must_use]
    pub fn relative_path(&self) -> String {
        match &self.resource {
            WorkspaceAccessResource::JoinRequests => {
                format!("workspaces/{}/devices/join-requests", self.workspace_id)
            }
            WorkspaceAccessResource::JoinRequestApproval(join_request_id) => format!(
                "workspaces/{}/devices/join-requests/{}/approval",
                self.workspace_id, join_request_id
            ),
            WorkspaceAccessResource::AccessEvents => {
                format!("workspaces/{}/access/events", self.workspace_id)
            }
            WorkspaceAccessResource::AccessState => {
                format!("workspaces/{}/access/state", self.workspace_id)
            }
            WorkspaceAccessResource::KeyEnvelope {
                key_id,
                recipient_device_id,
            } => {
                format!(
                    "workspaces/{}/keys/{key_id}/envelopes/{recipient_device_id}",
                    self.workspace_id
                )
            }
        }
    }

    #[must_use]
    pub fn absolute_path(&self) -> String {
        format!("/{}", self.relative_path())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedWorkspaceSyncEndpoint {
    pub endpoint: WorkspaceSyncEndpoint,
    pub required_permission: WorkspacePermission,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSyncAuthTarget {
    pub workspace_id: WorkspaceId,
    pub required_permission: WorkspacePermission,
}

#[derive(Debug)]
pub enum WorkspaceSyncRouteClassificationError {
    UnsupportedMethod { method: String },
    MissingWorkspaceRouteTail,
    InvalidWorkspaceId(ProtocolError),
    InvalidBlobId(ProtocolError),
    InvalidManifestId(ProtocolError),
    InvalidWorkspaceRouteTail { tail: String },
}

impl fmt::Display for WorkspaceSyncRouteClassificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMethod { method } => {
                write!(formatter, "unsupported workspace sync method `{method}`")
            }
            Self::MissingWorkspaceRouteTail => {
                formatter.write_str("workspace sync route is missing a resource path")
            }
            Self::InvalidWorkspaceId(error) => write!(formatter, "invalid workspace id: {error}"),
            Self::InvalidBlobId(error) => write!(formatter, "invalid blob id: {error}"),
            Self::InvalidManifestId(error) => write!(formatter, "invalid manifest id: {error}"),
            Self::InvalidWorkspaceRouteTail { tail } => {
                write!(formatter, "invalid workspace sync route tail `{tail}`")
            }
        }
    }
}

impl std::error::Error for WorkspaceSyncRouteClassificationError {}

pub fn classify_workspace_sync_route_with_method(
    method: &str,
    path: &str,
) -> Result<Option<ClassifiedWorkspaceSyncEndpoint>, WorkspaceSyncRouteClassificationError> {
    if !path.starts_with("/workspaces/") {
        return Ok(None);
    }

    let method = WorkspaceSyncMethod::try_from(method)?;
    classify_workspace_sync_route(method, path)
}

pub fn classify_workspace_sync_route(
    method: WorkspaceSyncMethod,
    path: &str,
) -> Result<Option<ClassifiedWorkspaceSyncEndpoint>, WorkspaceSyncRouteClassificationError> {
    let Some((workspace_id, route_tail)) = split_workspace_route(path)? else {
        return Ok(None);
    };

    let workspace_id = WorkspaceId::parse(workspace_id)
        .map_err(WorkspaceSyncRouteClassificationError::InvalidWorkspaceId)?;

    let endpoint = match route_tail.split_once('/') {
        None if route_tail == "head" => WorkspaceSyncEndpoint::head(workspace_id),
        Some(("blobs", blob_id)) if !blob_id.contains('/') => WorkspaceSyncEndpoint::blob(
            workspace_id,
            BlobId::parse(blob_id).map_err(WorkspaceSyncRouteClassificationError::InvalidBlobId)?,
        ),
        Some(("manifests", manifest_id)) if !manifest_id.contains('/') => {
            WorkspaceSyncEndpoint::manifest(
                workspace_id,
                ManifestId::parse(manifest_id)
                    .map_err(WorkspaceSyncRouteClassificationError::InvalidManifestId)?,
            )
        }
        _ => {
            return Err(
                WorkspaceSyncRouteClassificationError::InvalidWorkspaceRouteTail {
                    tail: route_tail.to_string(),
                },
            );
        }
    };

    let required_permission = permission_for(method, &endpoint.resource);

    Ok(Some(ClassifiedWorkspaceSyncEndpoint {
        endpoint,
        required_permission,
    }))
}

pub fn classify_workspace_sync_auth_target_with_method(
    method: &str,
    path: &str,
) -> Result<Option<WorkspaceSyncAuthTarget>, WorkspaceSyncRouteClassificationError> {
    if !path.starts_with("/workspaces/") {
        return Ok(None);
    }

    if is_non_sync_workspace_route(path)? {
        return Ok(None);
    }

    let method = WorkspaceSyncMethod::try_from(method)?;
    classify_workspace_sync_auth_target(method, path)
}

pub fn classify_workspace_sync_auth_target(
    method: WorkspaceSyncMethod,
    path: &str,
) -> Result<Option<WorkspaceSyncAuthTarget>, WorkspaceSyncRouteClassificationError> {
    let Some((workspace_id, route_tail)) = split_workspace_route(path)? else {
        return Ok(None);
    };

    if route_tail.starts_with("devices/")
        || route_tail.starts_with("access/")
        || route_tail.starts_with("keys/")
    {
        return Ok(None);
    }

    let workspace_id = WorkspaceId::parse(workspace_id)
        .map_err(WorkspaceSyncRouteClassificationError::InvalidWorkspaceId)?;
    let required_permission = auth_permission_for_route_tail(method, route_tail)?;

    Ok(Some(WorkspaceSyncAuthTarget {
        workspace_id,
        required_permission,
    }))
}

fn split_workspace_route(
    path: &str,
) -> Result<Option<(&str, &str)>, WorkspaceSyncRouteClassificationError> {
    let Some(rest) = path.strip_prefix("/workspaces/") else {
        return Ok(None);
    };

    rest.split_once('/')
        .ok_or(WorkspaceSyncRouteClassificationError::MissingWorkspaceRouteTail)
        .map(Some)
}

fn is_non_sync_workspace_route(path: &str) -> Result<bool, WorkspaceSyncRouteClassificationError> {
    let Some((_, route_tail)) = split_workspace_route(path)? else {
        return Ok(false);
    };

    Ok(route_tail.starts_with("devices/") || route_tail.starts_with("access/"))
}

fn permission_for(
    method: WorkspaceSyncMethod,
    resource: &WorkspaceSyncResource,
) -> WorkspacePermission {
    match (method, resource) {
        (WorkspaceSyncMethod::Get, _) => WorkspacePermission::ReadObjects,
        (WorkspaceSyncMethod::Put, WorkspaceSyncResource::Head) => WorkspacePermission::UpdateHead,
        (
            WorkspaceSyncMethod::Put,
            WorkspaceSyncResource::Blob(_) | WorkspaceSyncResource::Manifest(_),
        ) => WorkspacePermission::WriteObjects,
    }
}

fn auth_permission_for_route_tail(
    method: WorkspaceSyncMethod,
    route_tail: &str,
) -> Result<WorkspacePermission, WorkspaceSyncRouteClassificationError> {
    let protected_resource = match route_tail {
        "head" => Some(WorkspaceSyncResourceKind::Head),
        tail if tail.starts_with("blobs/") => Some(WorkspaceSyncResourceKind::Blob),
        tail if tail.starts_with("manifests/") => Some(WorkspaceSyncResourceKind::Manifest),
        _ => None,
    };

    let Some(resource) = protected_resource else {
        return Err(
            WorkspaceSyncRouteClassificationError::InvalidWorkspaceRouteTail {
                tail: route_tail.to_string(),
            },
        );
    };

    match (method, resource) {
        (WorkspaceSyncMethod::Get, _) => Ok(WorkspacePermission::ReadObjects),
        (WorkspaceSyncMethod::Put, WorkspaceSyncResourceKind::Head) => {
            Ok(WorkspacePermission::UpdateHead)
        }
        (
            WorkspaceSyncMethod::Put,
            WorkspaceSyncResourceKind::Blob | WorkspaceSyncResourceKind::Manifest,
        ) => Ok(WorkspacePermission::WriteObjects),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceSyncResourceKind {
    Blob,
    Manifest,
    Head,
}
