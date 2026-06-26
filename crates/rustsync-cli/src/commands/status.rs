use rustsync_core::{manifest::ManifestChange, workspace::LocalWorkspaceEngine};

use std::error::Error;
use std::path::PathBuf;

pub fn run(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let engine = LocalWorkspaceEngine::open(&path)?;
    print!("{}", status_output(&engine)?);

    Ok(())
}

fn status_output(engine: &LocalWorkspaceEngine) -> Result<String, Box<dyn Error>> {
    let status = engine.working_tree_status()?;

    let mut output = String::new();
    output.push_str(&format!("On workspace {}\n", engine.workspace_id()));

    if !status.has_baseline {
        output.push_str("No staged manifest found; showing all workspace files as untracked.\n");
    }

    if status.diff.is_empty() {
        if status.has_baseline {
            output.push_str("working tree matches staged snapshot\n");
        } else {
            output.push_str("no workspace files found\n");
        }
        return Ok(output);
    }

    let changes = ChangeGroups::from_changes(status.diff.changes);
    output.push_str("Changes not staged for push:\n");
    append_group(&mut output, "untracked", &changes.added);
    append_group(&mut output, "modified", &changes.modified);
    append_group(&mut output, "deleted", &changes.deleted);

    Ok(output)
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
