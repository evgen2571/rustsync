use axum::{
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, Uri},
    middleware::Next,
    response::Response,
};
use rustsync_protocol::{
    DeviceId, UnixTimestamp, WorkspaceId, WorkspacePermission,
    WorkspaceSyncRouteClassificationError,
    auth::{
        AuthHeaders, DEVICE_ID_HEADER, NONCE_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER,
        canonical_request_payload, sha256_hex,
    },
    classify_workspace_sync_auth_target_with_method,
};

use crate::{
    AppState,
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
    let Some(target) = classify_auth_target(request.method(), request.uri())? else {
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
    let auth_headers = parse_auth_headers(headers)?;
    validate_timestamp(auth_headers.timestamp)?;

    let access_state = state.storage().get_access_state(workspace_id).await?;
    let device = access_state
        .active_device_record(&auth_headers.device_id)
        .map_err(ServerError::AuthProtocol)?;

    let path_and_query = uri
        .path_and_query()
        .map_or_else(|| uri.path(), |path_and_query| path_and_query.as_str());
    let payload = canonical_request_payload(
        method.as_str(),
        path_and_query,
        &sha256_hex(body),
        auth_headers.timestamp,
        &auth_headers.device_id,
        &auth_headers.nonce,
        body.len() as u64,
    );

    device
        .verify_signature(&payload, &auth_headers.signature)
        .map_err(ServerError::AuthProtocol)?;
    state.replay_cache.check_and_record(
        workspace_id,
        &auth_headers.device_id,
        &auth_headers.nonce,
        auth_headers.timestamp,
        MAX_TIMESTAMP_SKEW_SECONDS,
    )?;
    access_state
        .require_permission(&auth_headers.device_id, permission)
        .map_err(ServerError::AuthProtocol)?;

    Ok(AuthenticatedDevice {
        workspace_id: workspace_id.clone(),
        device_id: auth_headers.device_id,
        permission,
    })
}

fn parse_auth_headers(headers: &HeaderMap) -> ServerResult<AuthHeaders> {
    let device_id = required_header(headers, DEVICE_ID_HEADER)?;
    let timestamp = required_header(headers, TIMESTAMP_HEADER)?;
    let nonce = required_header(headers, NONCE_HEADER)?;
    let signature = required_header(headers, SIGNATURE_HEADER)?;

    AuthHeaders::from_header_values(device_id, timestamp, nonce, signature)
        .map_err(ServerError::AuthProtocol)
}

fn required_header<'a>(headers: &'a HeaderMap, name: &'static str) -> ServerResult<&'a str> {
    headers
        .get(name)
        .ok_or(ServerError::AuthenticationRequired)
        .and_then(header_value_to_str)
}

fn header_value_to_str(value: &HeaderValue) -> ServerResult<&str> {
    value.to_str().map_err(|_| ServerError::InvalidAuthHeader)
}

fn validate_timestamp(timestamp: UnixTimestamp) -> ServerResult<()> {
    let now = UnixTimestamp::now().as_secs();

    if now.abs_diff(timestamp.as_secs()) > MAX_TIMESTAMP_SKEW_SECONDS {
        return Err(ServerError::AuthTimestampOutsideWindow);
    }

    Ok(())
}

struct AuthTarget {
    workspace_id: WorkspaceId,
    permission: WorkspacePermission,
}

fn classify_auth_target(method: &Method, uri: &Uri) -> ServerResult<Option<AuthTarget>> {
    let Some(classified) =
        classify_workspace_sync_auth_target_with_method(method.as_str(), uri.path())
            .map_err(classification_error)?
    else {
        return Ok(None);
    };

    Ok(Some(AuthTarget {
        workspace_id: classified.workspace_id,
        permission: classified.required_permission,
    }))
}

fn classification_error(error: WorkspaceSyncRouteClassificationError) -> ServerError {
    match error {
        WorkspaceSyncRouteClassificationError::InvalidWorkspaceId(_) => {
            ServerError::InvalidWorkspaceId
        }
        WorkspaceSyncRouteClassificationError::InvalidBlobId(_)
        | WorkspaceSyncRouteClassificationError::InvalidManifestId(_)
        | WorkspaceSyncRouteClassificationError::MissingWorkspaceRouteTail
        | WorkspaceSyncRouteClassificationError::InvalidWorkspaceRouteTail { .. }
        | WorkspaceSyncRouteClassificationError::UnsupportedMethod { .. } => {
            ServerError::AuthenticationRequired
        }
    }
}
