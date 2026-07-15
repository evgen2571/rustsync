use rustsync_protocol::{
    AccessEvent, AccessEventApplicationResponse, AccessState, AccessStateResponse, ApiErrorCode,
    ApiErrorResponse, ApplyAccessEventRequest, ApproveJoinRequestRequest, BlobId,
    CreateWorkspaceRequest, CreateWorkspaceResponse, DeviceId, DeviceJoinRequest, DeviceRecord,
    DeviceStatus, JoinRequestId, JoinRequestSubmissionResponse, JoinRequestSubmissionStatus, KeyId,
    ListJoinRequestsResponse, ManifestId, ObjectUploadResponse, ObjectUploadStatus,
    SignedAccessEvent, UnixTimestamp, WORKSPACE_ACCESS_EVENTS_ROUTE, WORKSPACE_ACCESS_STATE_ROUTE,
    WORKSPACE_BLOB_ROUTE, WORKSPACE_HEAD_ROUTE, WORKSPACE_JOIN_REQUEST_APPROVAL_ROUTE,
    WORKSPACE_JOIN_REQUESTS_ROUTE, WORKSPACE_MANIFEST_ROUTE, WORKSPACES_ROUTE,
    WorkspaceAccessEndpoint, WorkspaceHead, WorkspaceId, WorkspacePermission, WorkspaceRole,
    WorkspaceSyncEndpoint, WorkspaceSyncMethod, WorkspaceSyncResource,
    WorkspaceSyncRouteClassificationError, classify_workspace_sync_auth_target_with_method,
    classify_workspace_sync_route, classify_workspace_sync_route_with_method,
    fingerprint_from_public_keys,
};

#[test]
fn api_error_response_uses_stable_machine_readable_code() {
    let response = ApiErrorResponse::new(
        ApiErrorCode::UnauthorizedDevice,
        "device is not authorized for this workspace",
    );

    let json = serde_json::to_value(&response).unwrap();

    assert_eq!(json["error"], "unauthorized_device");
    assert_eq!(
        json["message"],
        "device is not authorized for this workspace"
    );

    let round_trip: ApiErrorResponse = serde_json::from_value(json).unwrap();
    assert_eq!(round_trip.error, ApiErrorCode::UnauthorizedDevice);
    assert_eq!(
        round_trip.message,
        "device is not authorized for this workspace"
    );
}

#[test]
fn object_upload_response_reports_idempotent_outcome() {
    let created = ObjectUploadResponse::created();
    let already_exists = ObjectUploadResponse::already_exists();

    assert_eq!(created.status, ObjectUploadStatus::Created);
    assert_eq!(already_exists.status, ObjectUploadStatus::AlreadyExists);
    assert_eq!(
        serde_json::to_value(&already_exists).unwrap(),
        serde_json::json!({"status": "already_exists"})
    );
}

#[test]
fn create_workspace_request_and_response_round_trip_as_json() {
    let workspace_id = WorkspaceId::parse("workspace_test123").unwrap();
    let request = CreateWorkspaceRequest {
        workspace_id: workspace_id.clone(),
        access_state: AccessState::empty(workspace_id.clone()),
    };
    let response = CreateWorkspaceResponse {
        workspace_id: workspace_id.clone(),
        head: WorkspaceHead::empty(workspace_id),
    };

    let request_json = serde_json::to_value(&request).unwrap();
    let response_json = serde_json::to_value(&response).unwrap();

    assert_eq!(
        serde_json::from_value::<CreateWorkspaceRequest>(request_json).unwrap(),
        request
    );
    assert_eq!(
        serde_json::from_value::<CreateWorkspaceResponse>(response_json).unwrap(),
        response
    );
}

#[test]
fn join_request_and_access_api_dtos_round_trip_as_json() {
    let workspace_id = WorkspaceId::parse("workspace_test123").unwrap();
    let join_request_id = JoinRequestId::parse("join_test123").unwrap();
    let device = test_device_record("device_joining", DeviceStatus::Pending);
    let join_request = DeviceJoinRequest::new_unsigned(
        join_request_id.clone(),
        workspace_id.clone(),
        device.clone(),
        UnixTimestamp::from_secs(123),
    )
    .with_signature(vec![1, 2, 3]);
    let event = SignedAccessEvent::new_unsigned(
        rustsync_protocol::id::AccessEventId::parse("event_test123").unwrap(),
        workspace_id.clone(),
        7,
        DeviceId::parse("device_owner").unwrap(),
        UnixTimestamp::from_secs(456),
        AccessEvent::DeviceJoined {
            join_request_id: join_request_id.clone(),
            device,
            role: WorkspaceRole::Member,
        },
    )
    .with_signature(vec![4, 5, 6]);

    let submit_response = JoinRequestSubmissionResponse::submitted(join_request_id.clone());
    assert_eq!(
        submit_response.status,
        JoinRequestSubmissionStatus::Submitted
    );
    assert_eq!(
        serde_json::to_value(&submit_response).unwrap(),
        serde_json::json!({"request_id": join_request_id, "status": "submitted"})
    );

    let list_response = ListJoinRequestsResponse {
        requests: vec![join_request.clone()],
    };
    assert_eq!(
        serde_json::from_value::<ListJoinRequestsResponse>(
            serde_json::to_value(&list_response).unwrap()
        )
        .unwrap(),
        list_response
    );

    let approve = ApproveJoinRequestRequest {
        join_request_id: join_request.request_id.clone(),
        event: event.clone(),
    };
    let apply = ApplyAccessEventRequest { event };
    assert_eq!(
        serde_json::from_value::<ApproveJoinRequestRequest>(
            serde_json::to_value(&approve).unwrap()
        )
        .unwrap(),
        approve
    );
    assert_eq!(
        serde_json::from_value::<ApplyAccessEventRequest>(serde_json::to_value(&apply).unwrap())
            .unwrap(),
        apply
    );

    let access_state = AccessState::empty(workspace_id);
    let application = AccessEventApplicationResponse {
        access_state: access_state.clone(),
    };
    let state = AccessStateResponse { access_state };
    assert_eq!(
        serde_json::from_value::<AccessEventApplicationResponse>(
            serde_json::to_value(&application).unwrap()
        )
        .unwrap(),
        application
    );
    assert_eq!(
        serde_json::from_value::<AccessStateResponse>(serde_json::to_value(&state).unwrap())
            .unwrap(),
        state
    );
}

#[test]
fn workspace_access_route_constructors_build_relative_and_absolute_paths() {
    let workspace_id = WorkspaceId::parse("workspace_test123").unwrap();
    let join_request_id = JoinRequestId::parse("join_test123").unwrap();

    let join_requests = WorkspaceAccessEndpoint::join_requests(workspace_id.clone());
    assert_eq!(
        join_requests.relative_path(),
        format!("workspaces/{workspace_id}/devices/join-requests")
    );
    assert_eq!(
        join_requests.absolute_path(),
        format!("/workspaces/{workspace_id}/devices/join-requests")
    );

    let approval = WorkspaceAccessEndpoint::join_request_approval(
        workspace_id.clone(),
        join_request_id.clone(),
    );
    assert_eq!(
        approval.relative_path(),
        format!("workspaces/{workspace_id}/devices/join-requests/{join_request_id}/approval")
    );
    assert_eq!(
        approval.absolute_path(),
        format!("/workspaces/{workspace_id}/devices/join-requests/{join_request_id}/approval")
    );

    let events = WorkspaceAccessEndpoint::access_events(workspace_id.clone());
    assert_eq!(
        events.relative_path(),
        format!("workspaces/{workspace_id}/access/events")
    );
    assert_eq!(
        events.absolute_path(),
        format!("/workspaces/{workspace_id}/access/events")
    );

    let state = WorkspaceAccessEndpoint::access_state(workspace_id.clone());
    assert_eq!(
        state.relative_path(),
        format!("workspaces/{workspace_id}/access/state")
    );
    assert_eq!(
        state.absolute_path(),
        format!("/workspaces/{workspace_id}/access/state")
    );

    let key_id = KeyId::parse("main").unwrap();
    let recipient_device_id = DeviceId::parse("device_recipient").unwrap();
    let envelope = WorkspaceAccessEndpoint::key_envelope(
        workspace_id.clone(),
        key_id.clone(),
        recipient_device_id.clone(),
    );
    assert_eq!(
        envelope.relative_path(),
        format!("workspaces/{workspace_id}/keys/{key_id}/envelopes/{recipient_device_id}")
    );
    assert_eq!(
        envelope.absolute_path(),
        format!("/workspaces/{workspace_id}/keys/{key_id}/envelopes/{recipient_device_id}")
    );

    assert_eq!(
        WORKSPACE_JOIN_REQUESTS_ROUTE,
        "/workspaces/{workspace_id}/devices/join-requests"
    );
    assert_eq!(
        WORKSPACE_JOIN_REQUEST_APPROVAL_ROUTE,
        "/workspaces/{workspace_id}/devices/join-requests/{join_request_id}/approval"
    );
    assert_eq!(
        WORKSPACE_ACCESS_EVENTS_ROUTE,
        "/workspaces/{workspace_id}/access/events"
    );
    assert_eq!(
        WORKSPACE_ACCESS_STATE_ROUTE,
        "/workspaces/{workspace_id}/access/state"
    );
}

fn test_device_record(device_id: &str, status: DeviceStatus) -> DeviceRecord {
    let signing_public_key = [1; 32];
    let exchange_public_key = [2; 32];
    DeviceRecord {
        device_id: DeviceId::parse(device_id).unwrap(),
        device_name: "test device".to_string(),
        signing_public_key,
        exchange_public_key,
        fingerprint: fingerprint_from_public_keys(&signing_public_key, &exchange_public_key),
        status,
    }
}

fn route_test_ids() -> (WorkspaceId, BlobId, ManifestId) {
    (
        WorkspaceId::parse("workspace_test123").unwrap(),
        BlobId::from_content(b"blob route test"),
        ManifestId::from_content(b"manifest route test"),
    )
}

#[test]
fn workspace_sync_route_constructors_build_relative_and_absolute_paths() {
    let (workspace_id, blob_id, manifest_id) = route_test_ids();

    let blob = WorkspaceSyncEndpoint::blob(workspace_id.clone(), blob_id.clone());
    assert_eq!(
        blob.relative_path(),
        format!("workspaces/{workspace_id}/blobs/{blob_id}")
    );
    assert_eq!(
        blob.absolute_path(),
        format!("/workspaces/{workspace_id}/blobs/{blob_id}")
    );

    let manifest = WorkspaceSyncEndpoint::manifest(workspace_id.clone(), manifest_id.clone());
    assert_eq!(
        manifest.relative_path(),
        format!("workspaces/{workspace_id}/manifests/{manifest_id}")
    );
    assert_eq!(
        manifest.absolute_path(),
        format!("/workspaces/{workspace_id}/manifests/{manifest_id}")
    );

    let head = WorkspaceSyncEndpoint::head(workspace_id.clone());
    assert_eq!(
        head.relative_path(),
        format!("workspaces/{workspace_id}/head")
    );
    assert_eq!(
        head.absolute_path(),
        format!("/workspaces/{workspace_id}/head")
    );

    assert_eq!(
        WORKSPACE_BLOB_ROUTE,
        "/workspaces/{workspace_id}/blobs/{blob_id}"
    );
    assert_eq!(
        WORKSPACE_MANIFEST_ROUTE,
        "/workspaces/{workspace_id}/manifests/{manifest_id}"
    );
    assert_eq!(WORKSPACE_HEAD_ROUTE, "/workspaces/{workspace_id}/head");
    assert_eq!(WORKSPACES_ROUTE, "/workspaces");
}

#[test]
fn workspace_sync_route_classifies_blob_manifest_and_head_permissions() {
    let (workspace_id, blob_id, manifest_id) = route_test_ids();

    let get_blob = classify_workspace_sync_route(
        WorkspaceSyncMethod::Get,
        &WorkspaceSyncEndpoint::blob(workspace_id.clone(), blob_id.clone()).absolute_path(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        get_blob.required_permission,
        WorkspacePermission::ReadObjects
    );
    assert_eq!(
        get_blob.endpoint.resource,
        WorkspaceSyncResource::Blob(blob_id.clone())
    );

    let put_blob = classify_workspace_sync_route(
        WorkspaceSyncMethod::Put,
        &WorkspaceSyncEndpoint::blob(workspace_id.clone(), blob_id).absolute_path(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        put_blob.required_permission,
        WorkspacePermission::WriteObjects
    );

    let get_manifest = classify_workspace_sync_route(
        WorkspaceSyncMethod::Get,
        &WorkspaceSyncEndpoint::manifest(workspace_id.clone(), manifest_id.clone()).absolute_path(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        get_manifest.endpoint.resource,
        WorkspaceSyncResource::Manifest(manifest_id.clone())
    );
    assert_eq!(
        get_manifest.required_permission,
        WorkspacePermission::ReadObjects
    );

    let put_manifest = classify_workspace_sync_route(
        WorkspaceSyncMethod::Put,
        &WorkspaceSyncEndpoint::manifest(workspace_id.clone(), manifest_id).absolute_path(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        put_manifest.required_permission,
        WorkspacePermission::WriteObjects
    );

    let get_head = classify_workspace_sync_route(
        WorkspaceSyncMethod::Get,
        &WorkspaceSyncEndpoint::head(workspace_id.clone()).absolute_path(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(get_head.endpoint.workspace_id, workspace_id.clone());
    assert_eq!(get_head.endpoint.resource, WorkspaceSyncResource::Head);
    assert_eq!(
        get_head.required_permission,
        WorkspacePermission::ReadObjects
    );

    let put_head = classify_workspace_sync_route(
        WorkspaceSyncMethod::Put,
        &WorkspaceSyncEndpoint::head(workspace_id).absolute_path(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        put_head.required_permission,
        WorkspacePermission::UpdateHead
    );
}

#[test]
fn workspace_sync_route_classification_handles_non_workspace_and_invalid_routes() {
    assert_eq!(
        classify_workspace_sync_route_with_method("POST", "/health").unwrap(),
        None
    );
    assert_eq!(
        classify_workspace_sync_route_with_method("POST", "/workspaces").unwrap(),
        None
    );
    assert_eq!(
        classify_workspace_sync_auth_target_with_method("POST", "/workspaces").unwrap(),
        None
    );

    assert!(matches!(
        classify_workspace_sync_route(WorkspaceSyncMethod::Get, "/workspaces/not valid/head"),
        Err(WorkspaceSyncRouteClassificationError::InvalidWorkspaceId(_))
    ));
    assert!(matches!(
        classify_workspace_sync_route(WorkspaceSyncMethod::Get, "/workspaces/workspace_test123"),
        Err(WorkspaceSyncRouteClassificationError::MissingWorkspaceRouteTail)
    ));
    assert!(matches!(
        classify_workspace_sync_route(
            WorkspaceSyncMethod::Get,
            "/workspaces/workspace_test123/unknown/resource"
        ),
        Err(WorkspaceSyncRouteClassificationError::InvalidWorkspaceRouteTail { .. })
    ));
    assert!(matches!(
        classify_workspace_sync_route(
            WorkspaceSyncMethod::Get,
            "/workspaces/workspace_test123/blobs/nope"
        ),
        Err(WorkspaceSyncRouteClassificationError::InvalidBlobId(_))
    ));
    assert!(matches!(
        classify_workspace_sync_route_with_method("DELETE", "/workspaces/workspace_test123/head"),
        Err(WorkspaceSyncRouteClassificationError::UnsupportedMethod { method }) if method == "DELETE"
    ));
}

#[test]
fn workspace_sync_auth_target_classification_does_not_parse_resource_ids() {
    let target = classify_workspace_sync_auth_target_with_method(
        "GET",
        "/workspaces/workspace_test123/blobs/not-a-blob-id",
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        target.workspace_id,
        WorkspaceId::parse("workspace_test123").unwrap()
    );
    assert_eq!(target.required_permission, WorkspacePermission::ReadObjects);

    let target = classify_workspace_sync_auth_target_with_method(
        "PUT",
        "/workspaces/workspace_test123/manifests/not-a-manifest-id/extra",
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        target.required_permission,
        WorkspacePermission::WriteObjects
    );

    assert!(matches!(
        classify_workspace_sync_auth_target_with_method(
            "GET",
            "/workspaces/workspace_test123/unknown/resource"
        ),
        Err(WorkspaceSyncRouteClassificationError::InvalidWorkspaceRouteTail { .. })
    ));
}
