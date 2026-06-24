use rustsync_protocol::{
    ApiErrorCode, ApiErrorResponse, BlobId, ManifestId, ObjectUploadResponse, ObjectUploadStatus,
    WORKSPACE_BLOB_ROUTE, WORKSPACE_HEAD_ROUTE, WORKSPACE_MANIFEST_ROUTE, WorkspaceId,
    WorkspacePermission, WorkspaceSyncEndpoint, WorkspaceSyncMethod, WorkspaceSyncResource,
    WorkspaceSyncRouteClassificationError, classify_workspace_sync_auth_target_with_method,
    classify_workspace_sync_route, classify_workspace_sync_route_with_method,
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
