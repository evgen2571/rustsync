use std::collections::BTreeMap;

use rustsync_core::{
    device::DeviceIdentity,
    reconciliation::{
        ReconciliationAction, SyncPhase, SyncState, merge_text_three_way, plan_reconciliation,
    },
    workspace::{LocalWorkspaceEngine, Workspace},
};
use rustsync_protocol::{Manifest, ManifestEntry, UnixTimestamp, WorkspaceId};
use tempfile::tempdir;

fn workspace_id() -> WorkspaceId {
    WorkspaceId::parse("workspace_reconciliation_test").expect("workspace id")
}

fn manifest(files: &[(&str, &str)]) -> Manifest {
    let mut manifest = Manifest::new(workspace_id());
    for (path, hash) in files {
        manifest
            .insert(
                (*path).to_string(),
                ManifestEntry::file(1, (*hash).to_string(), UnixTimestamp::from_secs(0)),
            )
            .expect("valid entry");
    }
    manifest
}

#[test]
fn planner_classifies_unchanged_local_only_remote_only_and_independent_changes() {
    let base = manifest(&[
        ("same.txt", "same"),
        ("local.txt", "base-local"),
        ("remote.txt", "base-remote"),
    ]);
    let local = manifest(&[
        ("same.txt", "same"),
        ("local.txt", "local"),
        ("remote.txt", "base-remote"),
    ]);
    let remote = manifest(&[
        ("same.txt", "same"),
        ("local.txt", "base-local"),
        ("remote.txt", "remote"),
    ]);

    let plan = plan_reconciliation(&base, &local, &remote).expect("plan");
    let actions: BTreeMap<_, _> = plan
        .paths
        .into_iter()
        .map(|item| (item.path, item.action))
        .collect();

    assert_eq!(actions["same.txt"], ReconciliationAction::Unchanged);
    assert_eq!(actions["local.txt"], ReconciliationAction::Upload);
    assert_eq!(actions["remote.txt"], ReconciliationAction::Download);
}

#[test]
fn planner_marks_same_path_file_changes_for_merge_without_blocking_other_paths() {
    let base = manifest(&[
        ("note.txt", "base"),
        ("local.txt", "base-local"),
        ("remote.txt", "base-remote"),
    ]);
    let local = manifest(&[
        ("note.txt", "local"),
        ("local.txt", "local"),
        ("remote.txt", "base-remote"),
    ]);
    let remote = manifest(&[
        ("note.txt", "remote"),
        ("local.txt", "base-local"),
        ("remote.txt", "remote"),
    ]);

    let plan = plan_reconciliation(&base, &local, &remote).expect("plan");
    let actions: BTreeMap<_, _> = plan
        .paths
        .into_iter()
        .map(|item| (item.path, item.action))
        .collect();

    assert_eq!(actions["note.txt"], ReconciliationAction::Merge);
    assert_eq!(actions["local.txt"], ReconciliationAction::Upload);
    assert_eq!(actions["remote.txt"], ReconciliationAction::Download);
}

#[test]
fn three_way_merge_combines_non_overlapping_text_changes() {
    let merged = merge_text_three_way(
        b"one\ntwo\nthree\n",
        b"ONE\ntwo\nthree\n",
        b"one\ntwo\nTHREE\n",
    )
    .expect("non-overlapping merge");

    assert_eq!(merged, b"ONE\ntwo\nTHREE\n");
}

#[test]
fn three_way_merge_rejects_overlapping_text_changes() {
    let merged = merge_text_three_way(b"one\n", b"local\n", b"remote\n");

    assert!(merged.is_none());
}

#[test]
fn state_records_pending_phase_and_round_trips() {
    let mut state = SyncState::default();
    state.begin_pending(SyncPhase::Upload, 7);
    let encoded = serde_json::to_vec(&state).expect("serialize state");
    let decoded: SyncState = serde_json::from_slice(&encoded).expect("deserialize state");

    assert_eq!(
        decoded.pending_operation.expect("pending").phase,
        SyncPhase::Upload
    );
}

#[test]
fn workspace_persists_sync_state_separately_from_staged_manifest() {
    let temp = tempdir().expect("temp dir");
    let identity = DeviceIdentity::generate("sync state test").expect("identity");
    let workspace =
        Workspace::init_with_device_identity(temp.path(), &identity).expect("workspace");
    let engine = LocalWorkspaceEngine::new(workspace.clone());
    let mut state = SyncState::default();
    state.begin_pending(SyncPhase::Publication, 4);

    engine.save_sync_state(&state).expect("save state");

    assert_eq!(engine.load_sync_state().expect("load state"), state);
    assert!(workspace.layout.sync_state_path.exists());
    assert!(!workspace.layout.manifest_path.exists());
}
