use rustsync_core::{
    manifest::{
        ManifestChange, build_manifest, diff_manifests, load_manifest, validate_manifest_workspace,
    },
    workspace::Workspace,
};
use rustsync_protocol::Manifest;

use std::error::Error;
use std::path::PathBuf;

pub fn run(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::open(&path)?;
    print!("{}", status_output(&workspace)?);

    Ok(())
}

fn status_output(workspace: &Workspace) -> Result<String, Box<dyn Error>> {
    let current = build_manifest(workspace)?;
    let (baseline, has_baseline) = match load_manifest(workspace)? {
        Some(manifest) => {
            validate_manifest_workspace(workspace, &manifest)?;
            (manifest, true)
        }
        None => (Manifest::new(workspace.config.workspace_id.clone()), false),
    };
    let diff = diff_manifests(&baseline, &current);

    let mut output = String::new();
    output.push_str(&format!("On workspace {}\n", workspace.workspace_id()));

    if !has_baseline {
        output.push_str("No saved manifest found; showing all workspace files as untracked.\n");
    }

    if diff.is_empty() {
        if has_baseline {
            output.push_str("nothing to sync, working tree clean\n");
        } else {
            output.push_str("no workspace files found\n");
        }
        return Ok(output);
    }

    let changes = ChangeGroups::from_changes(diff.changes);
    output.push_str("Changes not yet recorded:\n");
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

#[cfg(test)]
mod tests {
    use super::status_output;
    use rustsync_core::{
        device::DeviceIdentity,
        manifest::{build_manifest, save_manifest},
        workspace::Workspace,
    };
    use rustsync_protocol::Manifest;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn status_output_shows_all_files_untracked_without_saved_manifest() {
        let (_temp_dir, workspace) = init_workspace();
        fs::write(workspace.layout.root.join("file.txt"), "hello").expect("write working file");

        let output = status_output(&workspace).expect("render status output");

        assert!(output.contains(&format!("On workspace {}\n", workspace.workspace_id())));
        assert!(
            output.contains("No saved manifest found; showing all workspace files as untracked.\n")
        );
        assert!(output.contains("Changes not yet recorded:\n"));
        assert!(output.contains("  untracked: file.txt\n"));
        assert!(!output.contains("local device id"));
        assert!(!output.contains("default key id"));
        assert!(!output.contains("artifacts:"));
    }

    #[test]
    fn status_output_shows_no_files_message_without_saved_manifest_or_files() {
        let (_temp_dir, workspace) = init_workspace();

        let output = status_output(&workspace).expect("render status output");

        assert!(output.contains(&format!("On workspace {}\n", workspace.workspace_id())));
        assert!(
            output.contains("No saved manifest found; showing all workspace files as untracked.\n")
        );
        assert!(output.contains("no workspace files found\n"));
        assert!(!output.contains("nothing to sync, working tree clean"));
        assert!(!output.contains("Changes not yet recorded:"));
    }

    #[test]
    fn status_output_shows_files_untracked_with_empty_saved_manifest() {
        let (_temp_dir, workspace) = init_workspace();
        save_empty_manifest(&workspace);
        fs::write(workspace.layout.root.join("file.txt"), "hello").expect("write working file");

        let output = status_output(&workspace).expect("render status output");

        assert!(output.contains(&format!("On workspace {}\n", workspace.workspace_id())));
        assert!(!output.contains("No saved manifest found"));
        assert!(output.contains("Changes not yet recorded:\n"));
        assert!(output.contains("  untracked: file.txt\n"));
        assert!(!output.contains("  added:"));
    }

    #[test]
    fn status_output_shows_modified_untracked_and_deleted_changes() {
        let (_temp_dir, workspace) = init_workspace();
        fs::write(workspace.layout.root.join("modified.txt"), "before")
            .expect("write baseline file");
        fs::write(workspace.layout.root.join("deleted.txt"), "remove me")
            .expect("write baseline file");
        save_current_manifest(&workspace);

        fs::write(workspace.layout.root.join("modified.txt"), "after").expect("modify file");
        fs::write(workspace.layout.root.join("added.txt"), "new").expect("add file");
        fs::remove_file(workspace.layout.root.join("deleted.txt")).expect("delete file");

        let output = status_output(&workspace).expect("render status output");

        assert!(output.contains("Changes not yet recorded:\n"));
        assert!(output.contains("  untracked: added.txt\n"));
        assert!(output.contains("  modified:  modified.txt\n"));
        assert!(output.contains("  deleted:   deleted.txt\n"));
        assert!(!output.contains("nothing to sync, working tree clean"));
        assert!(!output.contains("No saved manifest found"));
    }

    #[test]
    fn status_output_shows_clean_message_when_unchanged() {
        let (_temp_dir, workspace) = init_workspace();
        fs::write(workspace.layout.root.join("file.txt"), "hello").expect("write baseline file");
        save_current_manifest(&workspace);

        let output = status_output(&workspace).expect("render status output");

        assert!(output.contains(&format!("On workspace {}\n", workspace.workspace_id())));
        assert!(output.contains("nothing to sync, working tree clean\n"));
        assert!(!output.contains("Changes not yet recorded:"));
    }

    fn init_workspace() -> (TempDir, Workspace) {
        let temp_dir = TempDir::new().expect("create temp dir");
        let identity = DeviceIdentity::generate("test-device").expect("generate device identity");
        Workspace::init_with_device_identity(temp_dir.path(), &identity).expect("init workspace");
        let workspace = Workspace::open(temp_dir.path()).expect("open workspace");

        (temp_dir, workspace)
    }

    fn save_empty_manifest(workspace: &Workspace) {
        let manifest = Manifest::new(workspace.config.workspace_id.clone());
        save_manifest(workspace, &manifest).expect("save empty manifest");
    }

    fn save_current_manifest(workspace: &Workspace) {
        let manifest = build_manifest(workspace).expect("build manifest");
        save_manifest(workspace, &manifest).expect("save manifest");
    }
}
