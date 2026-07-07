use std::error::Error;
use std::fs;
use std::path::PathBuf;

use rustsync_client::{ClientConfig, RustSyncClient};
use rustsync_core::{
    access::save_access_state,
    device::{DeviceIdentity, load_local_device_identity, save_local_device_identity},
    workspace::{Workspace, WorkspaceLayout},
};
use rustsync_protocol::{
    AccessEvent, DeviceJoinRequest, DeviceStatus, JoinRequestId, JoinRequestSubmissionStatus,
    SignedAccessEvent, UnixTimestamp, WorkspaceId, WorkspaceRole, id::AccessEventId,
};
use url::Url;

use crate::cli::DeviceRoleArg;
use crate::commands::sync::SERVER_BASE_URL;
use crate::local_device_signer::LocalDeviceRequestSigner;

pub async fn request(
    path: PathBuf,
    workspace_id: WorkspaceId,
    device_name: Option<String>,
) -> Result<(), Box<dyn Error>> {
    let layout = WorkspaceLayout::new(&path);
    let identity = load_or_create_joining_identity(&layout, device_name)?;
    let join_request = identity.create_join_request(workspace_id.clone())?;
    let request_id = join_request.request_id.clone();
    let device_id = identity.device_id().clone();
    let device_name = identity.device_name().to_owned();
    let fingerprint = identity.fingerprint();

    let client = client_for_identity(identity)?;
    let response = client.submit_join_request(&join_request).await?;

    println!("submitted device join request");
    println!("workspace id: {workspace_id}");
    println!("join request id: {}", response.request_id);
    println!("status: {}", submission_status_name(response.status));
    println!("device id: {device_id}");
    println!("device name: {device_name}");
    println!("device fingerprint: {fingerprint}");
    println!("identity path: {}", layout.device_identity_path.display());
    if response.request_id != request_id {
        println!(
            "note: server reported an existing pending request id; local request id was {request_id}"
        );
    }
    println!(
        "pending approval: ask an active managing device to run `rustsync device approve {}` in the workspace",
        response.request_id
    );
    println!(
        "note: approval activates server access for this device, but local workspace/key bootstrap is not implemented yet. Do not run sync commands from this pending directory until bootstrap support lands."
    );

    Ok(())
}

pub async fn list_requests(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::open(&path)?;
    let client = client_for_workspace(&workspace)?;
    let response = client.list_join_requests(workspace.workspace_id()).await?;

    if response.requests.is_empty() {
        println!(
            "no pending device join requests for workspace {}",
            workspace.workspace_id()
        );
        return Ok(());
    }

    println!(
        "pending device join requests for workspace {}:",
        workspace.workspace_id()
    );
    for request in response.requests {
        print_join_request(&request);
    }

    Ok(())
}

pub async fn approve(
    path: PathBuf,
    join_request_id: JoinRequestId,
    role: DeviceRoleArg,
) -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::open(&path)?;
    let identity = load_identity_for_workspace(&workspace)?;
    let client = client_for_identity(identity.clone())?;
    let response = client.list_join_requests(workspace.workspace_id()).await?;
    let join_request = response
        .requests
        .into_iter()
        .find(|request| request.request_id == join_request_id)
        .ok_or_else(|| {
            format!(
                "join request `{join_request_id}` is not pending for workspace {}",
                workspace.workspace_id()
            )
        })?;
    let access_state = client.fetch_access_state(workspace.workspace_id()).await?;
    save_access_state(
        &workspace.layout.access_control_path,
        &access_state.access_state,
    )?;

    let event = signed_device_joined_event(
        &workspace,
        &identity,
        &join_request,
        access_state.access_state.revision(),
        role.into(),
    )?;
    let approved = client
        .approve_join_request(workspace.workspace_id(), &join_request_id, &event)
        .await?;
    save_access_state(
        &workspace.layout.access_control_path,
        &approved.access_state,
    )?;

    println!("approved device join request");
    println!("workspace id: {}", workspace.workspace_id());
    println!("join request id: {join_request_id}");
    println!("device id: {}", join_request.device.device_id);
    println!("device name: {}", join_request.device.device_name);
    println!("device fingerprint: {}", join_request.device.fingerprint);
    println!("role: {}", role_name(role.into()));
    println!("new access revision: {}", approved.access_state.revision());
    println!(
        "note: server access is active for the approved device, but encrypted workspace key/bootstrap delivery is not implemented yet. Share no plaintext workspace keys manually unless you have an out-of-band secure process."
    );

    Ok(())
}

pub async fn list(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::open(&path)?;
    let client = client_for_workspace(&workspace)?;
    let response = client.fetch_access_state(workspace.workspace_id()).await?;
    save_access_state(
        &workspace.layout.access_control_path,
        &response.access_state,
    )?;

    println!("devices for workspace {}:", workspace.workspace_id());
    for membership in response.access_state.all_memberships() {
        let Some(device) = response.access_state.device_record(&membership.device_id) else {
            println!(
                "- {} role={} membership_status={:?} device_record=<missing>",
                membership.device_id,
                role_name(membership.role),
                membership.status
            );
            continue;
        };
        println!(
            "- {} name=\"{}\" role={} membership_status={:?} device_status={} fingerprint={}",
            device.device_id,
            device.device_name,
            role_name(membership.role),
            membership.status,
            device_status_name(device.status),
            device.fingerprint
        );
    }

    Ok(())
}

fn load_or_create_joining_identity(
    layout: &WorkspaceLayout,
    device_name: Option<String>,
) -> Result<DeviceIdentity, Box<dyn Error>> {
    if let Some(identity) = load_local_device_identity(&layout.device_identity_path)? {
        return Ok(identity);
    }

    fs::create_dir_all(&layout.rustsync_dir)?;
    let identity = DeviceIdentity::generate(device_name.unwrap_or_default())?;
    save_local_device_identity(&layout.device_identity_path, &identity)?;
    Ok(identity)
}

fn load_identity_for_workspace(workspace: &Workspace) -> Result<DeviceIdentity, Box<dyn Error>> {
    load_local_device_identity(&workspace.layout.device_identity_path)?.ok_or_else(|| {
        "missing local device identity; re-run `rustsync init` for this workspace".into()
    })
}

fn client_for_workspace(
    workspace: &Workspace,
) -> Result<RustSyncClient<LocalDeviceRequestSigner>, Box<dyn Error>> {
    let identity = load_identity_for_workspace(workspace)?;
    client_for_identity(identity)
}

fn client_for_identity(
    identity: DeviceIdentity,
) -> Result<RustSyncClient<LocalDeviceRequestSigner>, Box<dyn Error>> {
    let base_url = Url::parse(SERVER_BASE_URL)?;
    Ok(RustSyncClient::new(
        ClientConfig::new(base_url),
        LocalDeviceRequestSigner::new(identity),
    ))
}

fn signed_device_joined_event(
    workspace: &Workspace,
    actor: &DeviceIdentity,
    join_request: &DeviceJoinRequest,
    expected_revision: u64,
    role: WorkspaceRole,
) -> Result<SignedAccessEvent, Box<dyn Error>> {
    let event_id = AccessEventId::parse(format!("event_{}", join_request.request_id.as_str()))?;
    let event = AccessEvent::DeviceJoined {
        join_request_id: join_request.request_id.clone(),
        device: join_request.device.clone(),
        role,
    };
    let unsigned = SignedAccessEvent::new_unsigned(
        event_id,
        workspace.workspace_id().clone(),
        expected_revision,
        actor.device_id().clone(),
        UnixTimestamp::now(),
        event,
    );
    let signature = actor.sign(&unsigned.signing_payload())?;
    Ok(unsigned.with_signature(signature))
}

fn print_join_request(request: &DeviceJoinRequest) {
    println!("- join request id: {}", request.request_id);
    println!("  device id: {}", request.device.device_id);
    println!("  device name: {}", request.device.device_name);
    println!("  fingerprint: {}", request.device.fingerprint);
    println!("  created at: {}", request.created_at.as_secs());
}

fn submission_status_name(status: JoinRequestSubmissionStatus) -> &'static str {
    match status {
        JoinRequestSubmissionStatus::Submitted => "submitted",
        JoinRequestSubmissionStatus::AlreadyPending => "already_pending",
    }
}

fn role_name(role: WorkspaceRole) -> &'static str {
    match role {
        WorkspaceRole::Owner => "owner",
        WorkspaceRole::Member => "member",
    }
}

fn device_status_name(status: DeviceStatus) -> &'static str {
    match status {
        DeviceStatus::Pending => "pending",
        DeviceStatus::Active => "active",
        DeviceStatus::Revoked => "revoked",
    }
}
