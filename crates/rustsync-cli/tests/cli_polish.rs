use std::process::Command;
use tempfile::tempdir;

#[test]
fn versions_include_client_and_protocol() {
    for args in [vec!["version"], vec!["--version"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_rustsync-cli"))
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.contains("0.1.0") && text.contains("protocol 1"),
            "{text}"
        );
    }
}

#[test]
fn completions_support_requested_shells() {
    for shell in ["bash", "fish", "zsh"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rustsync-cli"))
            .args(["completions", shell])
            .output()
            .unwrap();
        assert!(output.status.success(), "{shell}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("rustsync") && text.contains("sync"));
    }
}

#[test]
fn missing_workspace_errors_are_json_and_actionable() {
    let dir = tempdir().unwrap();
    for command in ["status", "sync", "doctor"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rustsync-cli"))
            .args([command, "--json"])
            .arg(dir.path())
            .output()
            .unwrap();
        assert!(!output.status.success());
        let value: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("single JSON document");
        assert_eq!(value["ok"], false);
        assert!(value["error"].as_str().unwrap().contains("rustsync init"));
        assert!(output.stderr.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn json_commands_report_real_workspace_and_transfers() {
    use rustsync_server::{AppState, IndexedFsStorage, create_app};
    let temp = tempdir().unwrap();
    let root = temp.path().join("workspace");
    std::fs::create_dir(&root).unwrap();
    let storage = IndexedFsStorage::open(temp.path().join("server"))
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, create_app(AppState::new(storage)))
            .await
            .unwrap()
    });
    let run = |args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rustsync-cli"));
        if args == ["init"] {
            command.args(["--server-url", &url]);
        }
        command.args(args).arg(&root).output().unwrap()
    };
    assert!(run(&["init"]).status.success());
    std::fs::create_dir(root.join("notes")).unwrap();
    std::fs::write(root.join("notes/test.txt"), "hello").unwrap();
    let output = run(&["status", "--json"]);
    assert!(output.status.success(), "{:?}", output);
    let before: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(before["files"], 1);
    assert_eq!(before["device"]["role"], "owner");
    assert_eq!(before["remote_revision"], 0);
    assert!(before["pending_changes"].as_u64().unwrap() > 0);
    assert_eq!(before["server"]["reachable"], true);
    let output = run(&["sync", "--dry-run", "--json"]);
    assert!(output.status.success());
    let preview: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(preview["synced_revision"].is_null());
    assert_eq!(preview["mode"], "dry_run");
    assert_eq!(preview["published"], false);
    // A staged snapshot is not proof of a successful publication.
    rustsync_core::workspace::LocalWorkspaceEngine::open(&root)
        .unwrap()
        .stage_all()
        .unwrap();
    let output = run(&["status"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("notes/test.txt"), "{text}");
    assert!(
        !text.contains("working tree matches staged snapshot"),
        "{text}"
    );
    let output = run(&["sync", "--json"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let sync: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(sync["synced_revision"], 1);
    assert!(sync["uploaded_bytes"].as_u64().unwrap() > 0);
    let output = run(&["sync", "--json"]);
    let sync: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(sync["uploaded_blobs"], 0);
    let output = run(&["status", "--json"]);
    let after: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(after["local_revision"], 1);
    assert_eq!(after["pending_changes"], 0);
    let output = run(&["doctor", "--json"]);
    assert!(output.status.success());
    let doctor: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(doctor["ok"], true);
    assert!(doctor["pending_operation"].is_null());
    let output = run(&["sync", "--quiet"]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    std::fs::write(root.join("more.txt"), "more").unwrap();
    let output = run(&["sync", "--no-progress"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("Synced revision")
    );
    server.abort();
    let _ = server.await;
    let output = run(&["status", "--json"]);
    assert!(!output.status.success());
    let offline: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(offline["server"]["reachable"], false);
    assert!(offline["remote_revision"].is_null());
    assert_eq!(offline["files"], 2);
}
