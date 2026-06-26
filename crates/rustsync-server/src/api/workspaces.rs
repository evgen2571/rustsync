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
};

use crate::{
    AppState,
    auth::{
        MAX_AUTH_BODY_BYTES, MAX_TIMESTAMP_SKEW_SECONDS, signed_http_request_from_headers,
        validate_timestamp,
    },
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
    let signed_request = signed_http_request_from_headers(method, uri, headers, body)?;
    validate_timestamp(signed_request.input.timestamp)?;

    let device = request
        .access_state
        .active_device_record(&signed_request.input.device_id)
        .map_err(ServerError::AuthProtocol)?;
    let role = request
        .access_state
        .role(&signed_request.input.device_id)
        .map_err(ServerError::AuthProtocol)?;
    if role != WorkspaceRole::Owner {
        return Err(ServerError::AuthProtocol(ProtocolError::PermissionDenied {
            device_id: signed_request.input.device_id,
            permission: WorkspacePermission::ManageDevices,
        }));
    }

    signed_request
        .verify_with_device(device)
        .map_err(ServerError::AuthProtocol)?;
    state.replay_cache.check_and_record(
        &request.workspace_id,
        &signed_request.input.device_id,
        &signed_request.input.nonce,
        signed_request.input.timestamp,
        MAX_TIMESTAMP_SKEW_SECONDS,
    )?;

    Ok(())
}
