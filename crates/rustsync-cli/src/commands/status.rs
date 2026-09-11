use rustsync_core::{manifest::ManifestChange, workspace::LocalWorkspaceEngine};

use std::error::Error;
use std::path::PathBuf;

/// Report local changes alongside a fresh authenticated server observation.
pub async fn report(path: PathBuf, base_url: &url::Url, json: bool) -> Result<(), Box<dyn Error>> {
    let engine = LocalWorkspaceEngine::open(&path)?;
    let local = engine.working_tree_status()?;
    let state = engine.load_sync_state()?;
    let workspace = engine.workspace();
    let identity =
        rustsync_core::device::load_local_device_identity(&workspace.layout.device_identity_path)?
            .ok_or("missing local device identity; run `rustsync init`")?;
    let cached_access =
        rustsync_core::access::load_access_state(&workspace.layout.access_control_path)?;
    let mut role = cached_access
        .as_ref()
        .and_then(|access| access.membership(identity.device_id()))
        .map(|membership| membership.role);
    let client = super::sync::client_for_workspace(workspace, base_url)?;
    let remote = async {
        let head = client.fetch_workspace_head(engine.workspace_id()).await?;
        let access = client.fetch_access_state(engine.workspace_id()).await?;
        role = access
            .access_state
            .membership(identity.device_id())
            .map(|membership| membership.role);
        Ok::<_, rustsync_client::ClientError>(head)
    }
    .await;
    let (revision, error) = match remote {
        Ok(head) => (Some(head.revision), None),
        Err(error) => (None, Some(error.to_string())),
    };
    let files = local
        .current
        .entries
        .values()
        .filter(|entry| matches!(entry, rustsync_protocol::ManifestEntry::File(_)))
        .count();
    // Staging can precede a failed upload. Pending changes must use the last synced
    // snapshot, not a staged snapshot which may never have reached the server.
    let empty = rustsync_protocol::Manifest::new(engine.workspace_id().clone());
    let base = state.last_synced_manifest.as_ref().unwrap_or(&empty);
    let pending_diff = rustsync_core::manifest::diff_manifests(base, &local.current);
    let pending_changes = pending_diff.changes.len();
    let local_revision = state.last_synced_revision.or_else(|| {
        if state.last_synced_manifest.is_none() {
            Some(0)
        } else {
            None
        }
    });
    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": error.is_none(),
                "workspace_id": engine.workspace_id(),
                "device": { "id": identity.device_id(), "name": identity.device_name(), "role": role },
                "local_revision": local_revision,
                "remote_revision": revision,
                "files": files,
                "conflicts": state.conflicts.len(),
                "pending_changes": pending_changes,
                "pending_operation": state.pending_operation,
                "server": { "reachable": error.is_none(), "error": error },
            })
        );
    } else {
        println!("Workspace: {}", engine.workspace_id());
        println!(
            "Device: {} ({})",
            identity.device_name(),
            identity.device_id()
        );
        println!(
            "Role: {}",
            role.map(|role| serde_json::json!(role)
                .as_str()
                .unwrap_or("unknown")
                .to_string())
                .unwrap_or_else(|| "unknown".into())
        );
        println!(
            "Local revision: {}",
            local_revision.map_or_else(
                || "unknown (sync to record)".into(),
                |value| value.to_string()
            )
        );
        println!(
            "Remote revision: {}",
            revision.map_or_else(|| "unknown".into(), |value| value.to_string())
        );
        println!("Files: {files}");
        println!("Conflicts: {}", state.conflicts.len());
        println!("Pending changes: {pending_changes}");
        match &error {
            Some(error) => println!("Server: unavailable ({error})"),
            None => println!("Server: reachable"),
        }
        print!("{}", changes_output(pending_diff.changes));
    }
    if error.is_some() {
        return Err(crate::app::ReportedError.into());
    }
    Ok(())
}

fn changes_output(changes: Vec<ManifestChange>) -> String {
    let mut output = String::new();
    if changes.is_empty() {
        return output;
    }
    let changes = ChangeGroups::from_changes(changes);
    output.push_str("Changes in the local working tree:\n");
    append_group(&mut output, "untracked", &changes.added);
    append_group(&mut output, "modified", &changes.modified);
    append_group(&mut output, "deleted", &changes.deleted);

    output
}

#[derive(Default)]
struct ChangeGroups {
    added: Vec<String>,
    modified: Vec<String>,
    deleted: Vec<String>,
}

impl ChangeGroups {
    fn from_changes(changes: Vec<ManifestChange>) -> Self {
        let mut groups = Self::default();

        for change in changes {
            match change {
                ManifestChange::Added { path } => groups.added.push(path),
                ManifestChange::Modified { path } => groups.modified.push(path),
                ManifestChange::Deleted { path } => groups.deleted.push(path),
            }
        }

        groups.added.sort();
        groups.modified.sort();
        groups.deleted.sort();

        groups
    }
}

fn append_group(output: &mut String, label: &str, paths: &[String]) {
    let label = format!("{label}:");

    for path in paths {
        output.push_str(&format!("  {label:<11}{path}\n"));
    }
}
