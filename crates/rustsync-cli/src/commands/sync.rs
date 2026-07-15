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

pub async fn push(path: PathBuf, base_url: &Url) -> CommandResult {
    let engine = LocalWorkspaceEngine::open(path)?;
    let client = client_for_workspace(engine.workspace(), base_url)?;
    let report = SyncWorkflow::new(engine, client)
        .push()
        .await
        .map_err(|error| -> Box<dyn std::error::Error> { error })?;

    println!("{}", push_output(&report));

    Ok(())
}

pub async fn pull(path: PathBuf, force: bool, base_url: &Url) -> CommandResult {
    let engine = LocalWorkspaceEngine::open(path)?;
    let client = client_for_workspace(engine.workspace(), base_url)?;
    let mode = if force {
        PullMode::Force
    } else {
        PullMode::Safe
    };
    let report = SyncWorkflow::new(engine, client)
        .pull_with_mode(mode)
        .await
        .map_err(|error| -> Box<dyn std::error::Error> { error })?;

    println!("{}", pull_output(&report, mode));

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

fn pull_output(report: &PullReport, mode: PullMode) -> String {
    let verb = match mode {
        PullMode::Safe => "pulled",
        PullMode::Force => "force-pulled",
    };

    match &report.manifest_id {
        Some(manifest_id) => format!(
            "{verb} manifest {manifest_id} from remote revision {}; downloaded {} blob(s), wrote {} file(s), removed {} path(s), cached {} blob(s)",
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
    base_url: &Url,
) -> Result<RustSyncClient<LocalDeviceRequestSigner>, Box<dyn std::error::Error>> {
    let identity = load_local_device_identity(&workspace.layout.device_identity_path)?
        .ok_or("missing local device identity; re-run `rustsync init` for this workspace")?;
    let config = ClientConfig::new(base_url.clone());

    Ok(RustSyncClient::new(
        config,
        LocalDeviceRequestSigner::new(identity),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustsync_protocol::{ManifestId, WorkspaceId};

    fn pull_report() -> PullReport {
        PullReport {
            workspace_id: WorkspaceId::parse("workspace-output-test").expect("workspace id"),
            remote_head_revision: 42,
            manifest_id: Some(ManifestId::from_content(b"manifest")),
            downloaded_blobs: 2,
            changed_files: vec!["document.txt".to_string()],
            removed_paths: 1,
            prepared_directories: 0,
            written_files: 1,
            cached_blobs: 2,
        }
    }

    #[test]
    fn pull_output_uses_safe_wording_by_default() {
        let output = pull_output(&pull_report(), PullMode::Safe);

        assert!(output.starts_with("pulled manifest "), "{output}");
        assert!(!output.starts_with("force-pulled manifest "), "{output}");
    }

    #[test]
    fn pull_output_keeps_force_wording_for_force_mode() {
        let output = pull_output(&pull_report(), PullMode::Force);

        assert!(output.starts_with("force-pulled manifest "), "{output}");
    }
}
