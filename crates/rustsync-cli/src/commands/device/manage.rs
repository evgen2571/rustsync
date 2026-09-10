use super::{client_for_identity, load_identity_for_workspace, require_active_identity};
use crate::cli::DeviceRoleArg;
use rustsync_core::{access::save_access_state, workspace::Workspace};
use rustsync_protocol::{
    AccessEvent, DeviceId, MembershipStatus, SignedAccessEvent, UnixTimestamp, WorkspacePermission,
    WorkspaceRole, id::AccessEventId,
};
use std::{error::Error, path::PathBuf};
use url::Url;

pub async fn remove(path: PathBuf, device: DeviceId, server: &Url) -> Result<(), Box<dyn Error>> {
    change_membership(path, device, None, server).await
}

pub async fn set_role(
    path: PathBuf,
    device: DeviceId,
    role: DeviceRoleArg,
    server: &Url,
) -> Result<(), Box<dyn Error>> {
    change_membership(path, device, Some(role.into()), server).await
}

async fn change_membership(
    path: PathBuf,
    device: DeviceId,
    role: Option<WorkspaceRole>,
    server: &Url,
) -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::open(path)?;
    let identity = load_identity_for_workspace(&workspace)?;
    let client = client_for_identity(identity.clone(), server)?;
    let mut state = client
        .fetch_access_state(workspace.workspace_id())
        .await?
        .access_state;
    if state.workspace_id() != workspace.workspace_id() {
        return Err("server returned access state for a different workspace".into());
    }
    require_active_identity(&state, &identity)?;
    let permission = if role.is_some() {
        WorkspacePermission::ManageRoles
    } else {
        WorkspacePermission::ManageDevices
    };
    state.require_permission(identity.device_id(), permission)?;
    let membership = state
        .membership(&device)
        .ok_or_else(|| format!("device `{device}` is not enrolled in this workspace"))?;
    let already_done = match role {
        Some(role) => state.active_membership(&device)?.role == role,
        None => membership.status == MembershipStatus::Removed,
    };
    if already_done {
        save_access_state(&workspace.layout.access_control_path, &state)?;
        println!(
            "device {device} already has the requested membership; access revision {}",
            state.revision()
        );
        return Ok(());
    }
    let event = match role {
        Some(new_role) => AccessEvent::DeviceRoleChanged {
            device_id: device.clone(),
            new_role,
        },
        None => AccessEvent::DeviceRemoved {
            device_id: device.clone(),
        },
    };
    let unsigned = SignedAccessEvent::new_unsigned(
        AccessEventId::parse(format!(
            "event_{}_{}",
            identity.device_id(),
            state.revision()
        ))?,
        workspace.workspace_id().clone(),
        state.revision(),
        identity.device_id().clone(),
        UnixTimestamp::now(),
        event,
    );
    let signature = identity.sign(&unsigned.signing_payload())?;
    let signed = unsigned.with_signature(signature);
    state.apply_verified_event(&signed)?;
    let response = client
        .apply_access_event(workspace.workspace_id(), &signed)
        .await?;
    if response.access_state != state {
        return Err("server returned unexpected access state after membership change; run `rustsync device list` to inspect it".into());
    }
    save_access_state(&workspace.layout.access_control_path, &state).map_err(|error| {
        format!("server accepted the membership change, but saving local access state failed: {error}; run `rustsync device list` to refresh it")
    })?;
    match role {
        Some(role) => println!(
            "device {device} now has role {}; access revision {}",
            super::role_name(role),
            state.revision()
        ),
        None => println!(
            "removed device {device}; access revision {}",
            state.revision()
        ),
    }
    Ok(())
}
