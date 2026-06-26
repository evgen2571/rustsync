use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::IntoResponse,
    routing::post,
};
use rustsync_protocol::{
    CreateWorkspaceRequest, CreateWorkspaceResponse, ProtocolError, WORKSPACES_ROUTE,
    WorkspaceHead, WorkspacePermission, WorkspaceRole,
    auth::{canonical_request_payload, sha256_hex},
};

use crate::{
    AppState,
    auth::{MAX_AUTH_BODY_BYTES, parse_auth_headers, validate_timestamp},
    error::{ServerError, ServerResult},
};

pub fn routes() -> Router<AppState> {
    Router::new().route(WORKSPACES_ROUTE, post(create_workspace))
}

pub async fn create_workspace(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ServerResult<impl IntoResponse> {
    if body.len() > MAX_AUTH_BODY_BYTES {
        return Err(ServerError::RequestBodyTooLarge);
    }

    let request: CreateWorkspaceRequest = serde_json::from_slice(&body)
        .map_err(|error| ServerError::InvalidRequest(format!("invalid JSON body: {error}")))?;
    verify_bootstrap_signature(&state, &method, &uri, &headers, &body, &request)?;

    if request.access_state.workspace_id() != &request.workspace_id {
        return Err(ServerError::AccessStateWorkspaceMismatch {
            expected: request.workspace_id,
            actual: request.access_state.workspace_id().clone(),
        });
    }

    state
        .storage()
        .create_access_state(&request.workspace_id, &request.access_state)
        .await?;

    let head = WorkspaceHead::empty(request.workspace_id.clone());
    Ok((
        StatusCode::CREATED,
        Json(CreateWorkspaceResponse {
            workspace_id: request.workspace_id,
            head,
        }),
    ))
}

fn verify_bootstrap_signature(
    state: &AppState,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &Bytes,
    request: &CreateWorkspaceRequest,
) -> ServerResult<()> {
    let auth_headers = parse_auth_headers(headers)?;
    validate_timestamp(auth_headers.timestamp)?;

    let device = request
        .access_state
        .active_device_record(&auth_headers.device_id)
        .map_err(ServerError::AuthProtocol)?;
    let role = request
        .access_state
        .role(&auth_headers.device_id)
        .map_err(ServerError::AuthProtocol)?;
    if role != WorkspaceRole::Owner {
        return Err(ServerError::AuthProtocol(ProtocolError::PermissionDenied {
            device_id: auth_headers.device_id,
            permission: WorkspacePermission::ManageDevices,
        }));
    }

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
        &request.workspace_id,
        &auth_headers.device_id,
        &auth_headers.nonce,
        auth_headers.timestamp,
        5 * 60,
    )?;

    Ok(())
}
