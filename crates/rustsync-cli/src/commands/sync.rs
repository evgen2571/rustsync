use std::path::PathBuf;

use rustsync_client::{ClientConfig, RustSyncClient};
use rustsync_core::{
    device::load_local_device_identity,
    workspace::{LocalWorkspaceEngine, Workspace},
};
use url::Url;

use crate::local_device_signer::LocalDeviceRequestSigner;
use crate::sync_workflow::{PullMode, PullReport, PushReport, SyncWorkflow};

pub const SERVER_BASE_URL: &str = "http://127.0.0.1:3000";

type CommandResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub async fn push(path: PathBuf) -> CommandResult {
    let engine = LocalWorkspaceEngine::open(path)?;
    let client = client_for_workspace(engine.workspace())?;
    let report = SyncWorkflow::new(engine, client)
        .push()
        .await
        .map_err(|error| -> Box<dyn std::error::Error> { error })?;

    println!("{}", push_output(&report));

    Ok(())
}

pub async fn pull(path: PathBuf, force: bool) -> CommandResult {
    let engine = LocalWorkspaceEngine::open(path)?;
    let client = client_for_workspace(engine.workspace())?;
    let mode = if force {
        PullMode::Force
    } else {
        PullMode::Safe
    };
    let report = SyncWorkflow::new(engine, client)
        .pull_with_mode(mode)
        .await
        .map_err(|error| -> Box<dyn std::error::Error> { error })?;

    println!("{}", pull_output(&report));

    Ok(())
}

fn push_output(report: &PushReport) -> String {
    format!(
        "pushed staged snapshot: {} uploaded blob(s), {} reused blob(s), manifest {} ({} uploaded, {} reused); remote head advanced from revision {} to {}",
        report.uploaded_blobs,
        report.reused_blobs,
        report.manifest_id,
        report.uploaded_manifests,
        report.reused_manifests,
        report.previous_head_revision,
        report.updated_head_revision
    )
}

fn pull_output(report: &PullReport) -> String {
    match &report.manifest_id {
        Some(manifest_id) => format!(
            "force-pulled manifest {manifest_id} from remote revision {}; downloaded {} blob(s), wrote {} file(s), removed {} path(s), cached {} blob(s)",
            report.remote_head_revision,
            report.downloaded_blobs,
            report.written_files,
            report.removed_paths,
            report.cached_blobs
        ),
        None => format!(
            "pulled empty remote workspace from revision {}; removed {} path(s)",
            report.remote_head_revision, report.removed_paths
        ),
    }
}

fn client_for_workspace(
    workspace: &Workspace,
) -> Result<RustSyncClient<LocalDeviceRequestSigner>, Box<dyn std::error::Error>> {
    let base_url = Url::parse(SERVER_BASE_URL)?;
    let identity = load_local_device_identity(&workspace.layout.device_identity_path)?
        .ok_or("missing local device identity; re-run `rustsync init` for this workspace")?;
    let config = ClientConfig::new(base_url);

    Ok(RustSyncClient::new(
        config,
        LocalDeviceRequestSigner::new(identity),
    ))
}
