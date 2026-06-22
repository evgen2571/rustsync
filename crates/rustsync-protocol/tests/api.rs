use rustsync_protocol::{ApiErrorCode, ApiErrorResponse, ObjectUploadResponse, ObjectUploadStatus};

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
