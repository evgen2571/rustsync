use std::{fs, path::PathBuf};

use rustsync_client::{ClientConfig, RustSyncClient};
use rustsync_core::{
    device::load_local_device_identity,
    reconciliation::ReconciliationAction,
    workspace::{LocalWorkspaceEngine, Workspace},
};
use url::Url;

use crate::local_device_signer::LocalDeviceRequestSigner;
use crate::sync_workflow::{SyncMode, SyncReport, SyncWorkflow};

pub const SERVER_BASE_URL: &str = "http://127.0.0.1:3000";

type CommandResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub async fn remote_status(path: PathBuf, base_url: &Url) -> CommandResult {
    let engine = LocalWorkspaceEngine::open(path)?;
    let client = client_for_workspace(engine.workspace(), base_url)?;
    let head = SyncWorkflow::new(engine, client)
        .remote_status()
        .await
        .map_err(|error| -> Box<dyn std::error::Error> { error })?;
    match head.manifest_id {
        Some(manifest_id) => println!(
            "remote workspace {} is at revision {} with manifest {}",
            head.workspace_id, head.revision, manifest_id
        ),
        None => println!(
            "remote workspace {} is empty at revision {}",
            head.workspace_id, head.revision
        ),
    }
    Ok(())
}

pub async fn sync(
    path: PathBuf,
    dry_run: bool,
    discard_local: bool,
    base_url: &Url,
) -> CommandResult {
    let engine = LocalWorkspaceEngine::open(path)?;
    let client = client_for_workspace(engine.workspace(), base_url)?;
    let mode = if discard_local {
        SyncMode::DiscardLocal
    } else if dry_run {
        SyncMode::DryRun
    } else {
        SyncMode::Reconcile
    };
    let report = SyncWorkflow::new(engine, client)
        .sync(mode)
        .await
        .map_err(|error| -> Box<dyn std::error::Error> { error })?;
    print!("{}", sync_output(&report));
    Ok(())
}

pub fn conflicts(path: PathBuf) -> CommandResult {
    let engine = LocalWorkspaceEngine::open(path)?;
    let state = engine.load_sync_state()?;
    if state.conflicts.is_empty() {
        println!("no unresolved conflicts");
        return Ok(());
    }
    for (path, conflict) in state.conflicts {
        println!(
            "{path}: local `{}`, remote `{}`",
            conflict.local_copy_path, conflict.remote_copy_path
        );
    }
    Ok(())
}

pub fn resolve(
    workspace: PathBuf,
    path: String,
    keep_local: bool,
    keep_remote: bool,
) -> CommandResult {
    if keep_local == keep_remote {
        return Err(Into::into(
            "choose exactly one of `--keep-local` or `--keep-remote`",
        ));
    }
    let engine = LocalWorkspaceEngine::open(workspace)?;
    let mut state = engine.load_sync_state()?;
    let conflict = state
        .conflicts
        .remove(&path)
        .ok_or_else(|| format!("no unresolved conflict for `{path}`; run `rustsync conflicts`"))?;
    let local_path = engine
        .workspace()
        .layout
        .root
        .join(&conflict.local_copy_path);
    let remote_path = engine
        .workspace()
        .layout
        .root
        .join(&conflict.remote_copy_path);
    if keep_remote {
        fs::rename(&remote_path, &local_path)?;
        println!("resolved `{path}` by keeping remote; run `rustsync sync` to publish");
    } else {
        fs::remove_file(&remote_path)?;
        println!("resolved `{path}` by keeping local; run `rustsync sync` to publish");
    }
    engine.save_sync_state(&state)?;
    Ok(())
}

pub async fn doctor(path: PathBuf, base_url: &Url) -> CommandResult {
    let engine = LocalWorkspaceEngine::open(path)?;
    let state = engine.load_sync_state()?;
    let client = client_for_workspace(engine.workspace(), base_url)?;
    let head = SyncWorkflow::new(engine.clone(), client)
        .remote_status()
        .await
        .map_err(|error| -> Box<dyn std::error::Error> { error })?;
    let pending = state.pending_operation.map_or_else(
        || "none".to_string(),
        |pending| format!("{:?}", pending.phase),
    );
    println!("workspace metadata: ok");
    println!("device identity: ok");
    println!(
        "server connectivity and authentication: ok (remote revision {})",
        head.revision
    );
    println!(
        "cached objects directory: {}",
        engine
            .workspace()
            .layout
            .rustsync_dir
            .join("blobs")
            .display()
    );
    println!("pending synchronization: {pending}");
    println!("unresolved conflicts: {}", state.conflicts.len());
    Ok(())
}

fn sync_output(report: &SyncReport) -> String {
    let mut output = format!(
        "{} reconciliation against remote revision {}:\n",
        match report.mode {
            SyncMode::Reconcile => "sync",
            SyncMode::DryRun => "dry-run",
            SyncMode::DiscardLocal => "discard-local sync",
        },
        report.observed_remote_revision
    );
    for path in &report.plan.paths {
        let action = match path.action {
            ReconciliationAction::Unchanged => "unchanged",
            ReconciliationAction::Upload => "upload",
            ReconciliationAction::Download => "download",
            ReconciliationAction::Merge => "merge",
            ReconciliationAction::Conflict => "conflict",
        };
        output.push_str(&format!("  {action}: {}\n", path.path));
    }
    if report.mode == SyncMode::DryRun {
        output.push_str("no local files or remote state changed\n");
    } else if report.mode == SyncMode::DiscardLocal {
        output.push_str(&format!(
            "discarded local changes and applied remote revision {}; no publication occurred\n",
            report.observed_remote_revision
        ));
    } else if !report.plan.has_changes() {
        output.push_str("no reconciliation was needed; no publication occurred\n");
    } else if report.published {
        output
            .push_str("reconciliation published; run `rustsync conflicts` for unresolved paths\n");
    }
    output
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
