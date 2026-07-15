use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::IntoResponse,
    routing::{get, post},
};
use rustsync_protocol::{
    AccessEvent, AccessEventApplicationResponse, AccessStateResponse, ApplyAccessEventRequest,
    ApproveJoinRequestRequest, DeviceId, DeviceJoinRequest, JoinRequestId,
    JoinRequestSubmissionResponse, KeyEnvelope, KeyId, ListJoinRequestsResponse,
    ObjectUploadResponse, ProtocolError, SignedAccessEvent, WORKSPACE_ACCESS_EVENTS_ROUTE,
    WORKSPACE_ACCESS_STATE_ROUTE, WORKSPACE_JOIN_REQUEST_APPROVAL_ROUTE,
    WORKSPACE_JOIN_REQUESTS_ROUTE, WorkspaceId, WorkspacePermission,
};

use crate::{
    AppState,
    auth::{
        MAX_AUTH_BODY_BYTES, MAX_TIMESTAMP_SKEW_SECONDS, authenticate,
        signed_http_request_from_headers, validate_timestamp,
    },
    error::{ServerError, ServerResult},
    storage::JoinRequestPutResult,
};

const MAX_PENDING_JOIN_REQUESTS_PER_WORKSPACE: usize = 128;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            WORKSPACE_JOIN_REQUESTS_ROUTE,
            post(submit_join_request).get(list_join_requests),
        )
        .route(
            WORKSPACE_JOIN_REQUEST_APPROVAL_ROUTE,
            post(approve_join_request),
        )
        .route(WORKSPACE_ACCESS_EVENTS_ROUTE, post(apply_access_event))
        .route(WORKSPACE_ACCESS_STATE_ROUTE, get(fetch_access_state))
        .route(
            "/workspaces/{workspace_id}/keys/{key_id}/envelopes/{recipient_device_id}",
            get(fetch_key_envelope).put(put_key_envelope),
        )
}

async fn put_key_envelope(
    State(state): State<AppState>,
    Path((workspace_id, key_id, recipient_device_id)): Path<(WorkspaceId, KeyId, DeviceId)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ServerResult<impl IntoResponse> {
    ensure_body_limit(&body)?;
    let envelope: KeyEnvelope = parse_json(&body)?;
    let auth = authenticate(
        &state,
        &workspace_id,
        WorkspacePermission::ManageKeys,
        &method,
        &uri,
        &headers,
        &body,
    )
    .await?;
    if auth.device_id != envelope.sender_device_id {
        return Err(ServerError::AuthProtocol(
            ProtocolError::InvalidAccessEvent(
                "authenticated device does not match key envelope sender".to_string(),
            ),
        ));
    }
    if envelope.workspace_id != workspace_id
        || envelope.key_id != key_id
        || envelope.recipient_device_id != recipient_device_id
    {
        return Err(ServerError::InvalidRequest(
            "key envelope fields do not match request path".to_string(),
        ));
    }

    let access_state = state.storage().get_access_state(&workspace_id).await?;
    let sender = access_state
        .active_device_record(&envelope.sender_device_id)
        .map_err(ServerError::InvalidAccessState)?;
    access_state
        .active_device_record(&envelope.recipient_device_id)
        .map_err(ServerError::InvalidAccessState)?;
    envelope
        .verify_sender_signature(sender)
        .map_err(ServerError::InvalidAccessState)?;
    if envelope.access_revision != access_state.revision() {
        return Err(ServerError::InvalidRequest(
            "key envelope access revision does not match current access state".to_string(),
        ));
    }

    let result = state
        .storage()
        .put_key_envelope(&workspace_id, &key_id, &recipient_device_id, &body)
        .await?;
    let (status, response) = match result {
        crate::storage::PutResult::Created => {
            (StatusCode::CREATED, ObjectUploadResponse::created())
        }
        crate::storage::PutResult::AlreadyExists => {
            (StatusCode::OK, ObjectUploadResponse::already_exists())
        }
        crate::storage::PutResult::Conflict => return Err(ServerError::KeyEnvelopeConflict),
    };

    Ok((status, Json(response)))
}

async fn fetch_key_envelope(
    State(state): State<AppState>,
    Path((workspace_id, key_id, recipient_device_id)): Path<(WorkspaceId, KeyId, DeviceId)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> ServerResult<impl IntoResponse> {
    let auth = authenticate(
        &state,
        &workspace_id,
        WorkspacePermission::ReadObjects,
        &method,
        &uri,
        &headers,
        &Bytes::new(),
    )
    .await?;
    if auth.device_id != recipient_device_id {
        return Err(ServerError::KeyEnvelopeRecipientMismatch);
    }

    let bytes = state
        .storage()
        .get_key_envelope(&workspace_id, &key_id, &recipient_device_id)
        .await?
        .ok_or(ServerError::KeyEnvelopeNotFound)?;
    let envelope: KeyEnvelope = serde_json::from_slice(&bytes).map_err(|error| {
        ServerError::InvalidRequest(format!("stored key envelope is invalid JSON: {error}"))
    })?;

    Ok(Json(envelope))
}

async fn submit_join_request(
    State(state): State<AppState>,
    Path(workspace_id): Path<WorkspaceId>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ServerResult<impl IntoResponse> {
    ensure_body_limit(&body)?;
    let request: DeviceJoinRequest = parse_json(&body)?;

    request
        .verify_for_workspace(&workspace_id)
        .map_err(ServerError::InvalidAccessState)?;
    verify_join_request_http_signature(
        &state,
        &workspace_id,
        &method,
        &uri,
        &headers,
        &body,
        &request,
    )?;

    let _guard = state.lock_join_requests().await;
    let access_state = state.storage().get_access_state(&workspace_id).await?;
    if access_state.revision() == 0 {
        return Err(ServerError::InvalidRequest(
            "workspace has not been initialized".to_string(),
        ));
    }
    if access_state.is_active_member(&request.device.device_id) {
        return Err(ServerError::InvalidAccessState(
            ProtocolError::DeviceAlreadyMember(request.device.device_id),
        ));
    }

    if state
        .storage()
        .get_join_request(&workspace_id, &request.request_id)
        .await?
        .is_some()
    {
        let result = state
            .storage()
            .submit_join_request(&workspace_id, &request)
            .await?;
        let response = match result {
            JoinRequestPutResult::Submitted => {
                JoinRequestSubmissionResponse::submitted(request.request_id)
            }
            JoinRequestPutResult::AlreadyPending => {
                JoinRequestSubmissionResponse::already_pending(request.request_id)
            }
            JoinRequestPutResult::Conflict => return Err(ServerError::JoinRequestConflict),
        };
        let status = match result {
            JoinRequestPutResult::Submitted => StatusCode::CREATED,
            JoinRequestPutResult::AlreadyPending => StatusCode::OK,
            JoinRequestPutResult::Conflict => {
                unreachable!("conflicting join request returns early")
            }
        };
        return Ok((status, Json(response)));
    }

    let pending_requests = state.storage().list_join_requests(&workspace_id).await?;
    if let Some(existing_request) = pending_requests
        .iter()
        .find(|pending| pending.device.device_id == request.device.device_id)
    {
        return Ok((
            StatusCode::OK,
            Json(JoinRequestSubmissionResponse::already_pending(
                existing_request.request_id.clone(),
            )),
        ));
    }
    if pending_requests.len() >= MAX_PENDING_JOIN_REQUESTS_PER_WORKSPACE {
        return Err(ServerError::InvalidRequest(format!(
            "workspace already has the maximum of {MAX_PENDING_JOIN_REQUESTS_PER_WORKSPACE} pending join requests"
        )));
    }

    let result = state
        .storage()
        .submit_join_request(&workspace_id, &request)
        .await?;
    let response = match result {
        JoinRequestPutResult::Submitted => {
            JoinRequestSubmissionResponse::submitted(request.request_id)
        }
        JoinRequestPutResult::AlreadyPending => {
            JoinRequestSubmissionResponse::already_pending(request.request_id)
        }
        JoinRequestPutResult::Conflict => return Err(ServerError::JoinRequestConflict),
    };
    let status = match result {
        JoinRequestPutResult::Submitted => StatusCode::CREATED,
        JoinRequestPutResult::AlreadyPending => StatusCode::OK,
        JoinRequestPutResult::Conflict => unreachable!("conflicting join request returns early"),
    };

    Ok((status, Json(response)))
}

async fn list_join_requests(
    State(state): State<AppState>,
    Path(workspace_id): Path<WorkspaceId>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> ServerResult<impl IntoResponse> {
    authenticate_empty_body(
        &state,
        &workspace_id,
        WorkspacePermission::ManageDevices,
        &method,
        &uri,
        &headers,
    )
    .await?;

    let requests = state.storage().list_join_requests(&workspace_id).await?;
    Ok(Json(ListJoinRequestsResponse { requests }))
}

async fn approve_join_request(
    State(state): State<AppState>,
    Path((workspace_id, join_request_id)): Path<(WorkspaceId, JoinRequestId)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ServerResult<impl IntoResponse> {
    ensure_body_limit(&body)?;
    let request: ApproveJoinRequestRequest = parse_json(&body)?;
    if request.join_request_id != join_request_id {
        return Err(ServerError::InvalidRequest(
            "join request ID in body does not match path".to_string(),
        ));
    }

    let auth = authenticate(
        &state,
        &workspace_id,
        WorkspacePermission::ManageDevices,
        &method,
        &uri,
        &headers,
        &body,
    )
    .await?;
    if auth.device_id != request.event.actor_device_id {
        return Err(ServerError::AuthProtocol(
            ProtocolError::InvalidAccessEvent(
                "authenticated device does not match access event actor".to_string(),
            ),
        ));
    }

    let pending = state
        .storage()
        .get_join_request(&workspace_id, &join_request_id)
        .await?
        .ok_or_else(|| ServerError::InvalidRequest("join request is not pending".to_string()))?;
    pending
        .verify_for_workspace(&workspace_id)
        .map_err(ServerError::InvalidAccessState)?;
    request
        .event
        .verify_join_request(&pending)
        .map_err(ServerError::InvalidAccessState)?;

    let access_state = verify_and_apply_access_event(&state, &workspace_id, &request.event).await?;
    state
        .storage()
        .remove_join_request(&workspace_id, &join_request_id)
        .await?;

    Ok(Json(AccessEventApplicationResponse { access_state }))
}

async fn apply_access_event(
    State(state): State<AppState>,
    Path(workspace_id): Path<WorkspaceId>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ServerResult<impl IntoResponse> {
    ensure_body_limit(&body)?;
    let request: ApplyAccessEventRequest = parse_json(&body)?;
    let permission = request.event.event.required_permission().ok_or_else(|| {
        ServerError::InvalidRequest("workspace-created events cannot be applied here".to_string())
    })?;

    let auth = authenticate(
        &state,
        &workspace_id,
        permission,
        &method,
        &uri,
        &headers,
        &body,
    )
    .await?;
    if auth.device_id != request.event.actor_device_id {
        return Err(ServerError::AuthProtocol(
            ProtocolError::InvalidAccessEvent(
                "authenticated device does not match access event actor".to_string(),
            ),
        ));
    }

    if let AccessEvent::DeviceJoined {
        join_request_id, ..
    } = &request.event.event
    {
        let pending = state
            .storage()
            .get_join_request(&workspace_id, join_request_id)
            .await?
            .ok_or_else(|| {
                ServerError::InvalidRequest("join request is not pending".to_string())
            })?;
        pending
            .verify_for_workspace(&workspace_id)
            .map_err(ServerError::InvalidAccessState)?;
        request
            .event
            .verify_join_request(&pending)
            .map_err(ServerError::InvalidAccessState)?;

        let access_state =
            verify_and_apply_access_event(&state, &workspace_id, &request.event).await?;
        state
            .storage()
            .remove_join_request(&workspace_id, join_request_id)
            .await?;
        Ok(Json(AccessEventApplicationResponse { access_state }))
    } else {
        let access_state =
            verify_and_apply_access_event(&state, &workspace_id, &request.event).await?;
        Ok(Json(AccessEventApplicationResponse { access_state }))
    }
}

async fn fetch_access_state(
    State(state): State<AppState>,
    Path(workspace_id): Path<WorkspaceId>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> ServerResult<impl IntoResponse> {
    authenticate_empty_body(
        &state,
        &workspace_id,
        WorkspacePermission::ReadObjects,
        &method,
        &uri,
        &headers,
    )
    .await?;

    let access_state = state.storage().get_access_state(&workspace_id).await?;
    Ok(Json(AccessStateResponse { access_state }))
}

async fn verify_and_apply_access_event(
    state: &AppState,
    workspace_id: &WorkspaceId,
    event: &SignedAccessEvent,
) -> ServerResult<rustsync_protocol::AccessState> {
    let _guard = state.lock_access_events().await;

    if &event.workspace_id != workspace_id {
        return Err(ServerError::AccessStateWorkspaceMismatch {
            expected: workspace_id.clone(),
            actual: event.workspace_id.clone(),
        });
    }

    let mut access_state = state.storage().get_access_state(workspace_id).await?;
    let actor = access_state
        .active_device_record(&event.actor_device_id)
        .map_err(ServerError::InvalidAccessState)?;
    event
        .verify_signature(actor)
        .map_err(ServerError::InvalidAccessState)?;
    access_state
        .apply_verified_event(event)
        .map_err(ServerError::InvalidAccessState)?;
    state
        .storage()
        .save_access_state(workspace_id, &access_state)
        .await?;

    Ok(access_state)
}

async fn authenticate_empty_body(
    state: &AppState,
    workspace_id: &WorkspaceId,
    permission: WorkspacePermission,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
) -> ServerResult<()> {
    authenticate(
        state,
        workspace_id,
        permission,
        method,
        uri,
        headers,
        &Bytes::new(),
    )
    .await
    .map(|_| ())
}

fn verify_join_request_http_signature(
    state: &AppState,
    workspace_id: &WorkspaceId,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &Bytes,
    request: &DeviceJoinRequest,
) -> ServerResult<()> {
    let signed_request = signed_http_request_from_headers(method, uri, headers, body)?;
    validate_timestamp(signed_request.input.timestamp)?;
    if signed_request.input.device_id != request.device.device_id {
        return Err(ServerError::AuthProtocol(
            ProtocolError::InvalidAccessEvent(
                "authenticated device does not match join request device".to_string(),
            ),
        ));
    }

    signed_request
        .verify_with_device(&request.device)
        .map_err(ServerError::AuthProtocol)?;
    state.replay_cache.check_and_record(
        workspace_id,
        &signed_request.input.device_id,
        &signed_request.input.nonce,
        signed_request.input.timestamp,
        MAX_TIMESTAMP_SKEW_SECONDS,
    )?;

    Ok(())
}

fn ensure_body_limit(body: &Bytes) -> ServerResult<()> {
    if body.len() > MAX_AUTH_BODY_BYTES {
        return Err(ServerError::RequestBodyTooLarge);
    }

    Ok(())
}

fn parse_json<T>(body: &Bytes) -> ServerResult<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_slice(body)
        .map_err(|error| ServerError::InvalidRequest(format!("invalid JSON body: {error}")))
}
