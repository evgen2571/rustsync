use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::{Method, Uri},
    middleware::Next,
    response::Response,
};
use rustsync_protocol::{DeviceId, WorkspaceId, WorkspacePermission};

use crate::{
    AppState,
    auth::{
        canonical::{canonical_request_payload, sha256_hex},
        headers::AuthHeaders,
    },
    error::{ServerError, ServerResult},
};

const MAX_AUTH_BODY_BYTES: usize = 1024 * 1024;
const MAX_TIMESTAMP_SKEW_SECONDS: u64 = 5 * 60;

#[derive(Debug, Clone)]
pub struct AuthenticatedDevice {
    pub workspace_id: WorkspaceId,
    pub device_id: DeviceId,
    pub permission: WorkspacePermission,
}

pub async fn workspace_auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> ServerResult<Response> {
    let Some(target) = AuthTarget::from_request(request.method(), request.uri())? else {
        return Ok(next.run(request).await);
    };

    let (mut parts, body) = request.into_parts();
    let body = to_bytes(body, MAX_AUTH_BODY_BYTES)
        .await
        .map_err(|_| ServerError::RequestBodyTooLarge)?;

    let auth = authenticate(
        &state,
        &target.workspace_id,
        target.permission,
        &parts.method,
        &parts.uri,
        &parts.headers,
        &body,
    )
    .await?;

    parts.extensions.insert(auth);
    Ok(next.run(Request::from_parts(parts, Body::from(body))).await)
}

pub async fn authenticate(
    state: &AppState,
    workspace_id: &WorkspaceId,
    permission: WorkspacePermission,
    method: &Method,
    uri: &Uri,
    headers: &axum::http::HeaderMap,
    body: &Bytes,
) -> ServerResult<AuthenticatedDevice> {
    let auth_headers = AuthHeaders::parse(headers)?;
    validate_timestamp(auth_headers.timestamp_unix_seconds)?;

    let access_state = state.storage.get_access_state(workspace_id).await?;
    let device = access_state
        .active_device_record(&auth_headers.device_id)
        .map_err(ServerError::AuthProtocol)?;

    let payload = canonical_request_payload(
        method,
        uri,
        &sha256_hex(body),
        auth_headers.timestamp_unix_seconds,
        auth_headers.device_id.as_str(),
        &auth_headers.request_id,
        body.len() as u64,
    );

    device
        .verify_signature(&payload, &auth_headers.signature)
        .map_err(ServerError::AuthProtocol)?;
    access_state
        .require_permission(&auth_headers.device_id, permission)
        .map_err(ServerError::AuthProtocol)?;

    Ok(AuthenticatedDevice {
        workspace_id: workspace_id.clone(),
        device_id: auth_headers.device_id,
        permission,
    })
}

fn validate_timestamp(timestamp_unix_seconds: u64) -> ServerResult<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ServerError::InvalidAuthTimestamp)?
        .as_secs();

    if now.abs_diff(timestamp_unix_seconds) > MAX_TIMESTAMP_SKEW_SECONDS {
        return Err(ServerError::AuthTimestampOutsideWindow);
    }

    Ok(())
}

struct AuthTarget {
    workspace_id: WorkspaceId,
    permission: WorkspacePermission,
}

impl AuthTarget {
    fn from_request(method: &Method, uri: &Uri) -> ServerResult<Option<Self>> {
        let path = uri.path();

        if path == "/health" {
            return Ok(None);
        }

        let Some(rest) = path.strip_prefix("/workspaces/") else {
            return Ok(None);
        };

        let Some((workspace_id, route_tail)) = rest.split_once('/') else {
            return Ok(None);
        };

        let is_supported_method = *method == Method::GET || *method == Method::PUT;
        let is_sync_route = route_tail == "head"
            || route_tail.starts_with("blobs/")
            || route_tail.starts_with("manifests/");

        if !is_supported_method || !is_sync_route {
            return Ok(None);
        }

        let permission = WorkspacePermission::Sync;

        let workspace_id =
            WorkspaceId::parse(workspace_id).map_err(|_| ServerError::InvalidWorkspaceId)?;
        Ok(Some(Self {
            workspace_id,
            permission,
        }))
    }
}
