use rustsync_client::{ClientConfig, ClientError, RustSyncClient};
use rustsync_core::{
    access::load_access_state, device::DeviceIdentity, manifest::save_manifest,
    workspace::Workspace,
};
use rustsync_protocol::{CreateWorkspaceRequest, Manifest};
use url::Url;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use crate::commands::sync::SERVER_BASE_URL;
use crate::local_device_signer::LocalDeviceRequestSigner;

pub async fn run(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let identity = DeviceIdentity::generate("")?;
    let workspace = Workspace::init_with_device_identity(&path, &identity)?;
    let manifest = Manifest::new(workspace.config.workspace_id.clone());
    save_manifest(&workspace, &manifest)?;

    let access_state =
        load_access_state(&workspace.layout.access_control_path)?.ok_or_else(|| {
            format!(
                "workspace access state was not created at {}",
                workspace.layout.access_control_path.display()
            )
        })?;
    let request = CreateWorkspaceRequest {
        workspace_id: workspace.config.workspace_id.clone(),
        access_state,
    };
    let base_url = Url::parse(SERVER_BASE_URL)?;
    let device_id = identity.device_id().clone();
    let device_name = identity.device_name().to_owned();
    let device_fingerprint = identity.fingerprint();
    let client = RustSyncClient::new(
        ClientConfig::new(base_url),
        LocalDeviceRequestSigner::new(identity),
    );
    let remote = client
        .create_workspace(&request)
        .await
        .map_err(|error| init_remote_error(error, &workspace.layout.rustsync_dir))?;
    if remote.workspace_id != workspace.config.workspace_id
        || remote.head.workspace_id != workspace.config.workspace_id
    {
        return Err(format!(
            "server returned workspace `{}` with head for `{}` while initializing `{}`",
            remote.workspace_id, remote.head.workspace_id, workspace.config.workspace_id
        )
        .into());
    }

    println!("initialized rustsync workspace");
    println!("workspace id: {}", workspace.config.workspace_id);
    println!("device id: {device_id}");
    println!("device name: {device_name}");
    println!("device fingerprint: {device_fingerprint}");
    println!("path: {}", workspace.layout.rustsync_dir.display());
    println!("remote workspace created at {SERVER_BASE_URL}");
    println!("remote head revision: {}", remote.head.revision);

    Ok(())
}

fn init_remote_error(error: ClientError, rustsync_dir: &Path) -> Box<dyn Error> {
    match error {
        ClientError::Network(_) | ClientError::Timeout => {
            let cleanup_message = match fs::remove_dir_all(rustsync_dir) {
                Ok(()) => format!(
                    "local workspace metadata was removed from {}; start rustsync-server and retry `rustsync init`",
                    rustsync_dir.display()
                ),
                Err(cleanup_error) => format!(
                    "local workspace metadata remains at {} and could not be removed automatically: {cleanup_error}",
                    rustsync_dir.display()
                ),
            };
            format!("server is not running at {SERVER_BASE_URL}; {cleanup_message}").into()
        }
        other => other.into(),
    }
}
