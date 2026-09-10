use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
};

use rustsync_core::workspace::Workspace;
use rustsync_server::{AppState, IndexedFsStorage, create_app};

// Run the production HTTP application and SQLite backend in a separate process.
// Killing it between commands exercises database recovery as well as reopening.
#[tokio::test]
async fn server_process() {
    let Some(root) = std::env::var_os("RUSTSYNC_E2E_SERVER_ROOT") else {
        return;
    };
    let storage = IndexedFsStorage::open(root.into()).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    println!("SERVER_URL=http://{}", listener.local_addr().unwrap());
    axum::serve(listener, create_app(AppState::new(storage)))
        .await
        .unwrap();
}

struct Server {
    child: Child,
    url: String,
}

impl Server {
    fn start(root: &Path) -> Self {
        let mut server = Self {
            child: Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "server_process", "--nocapture"])
                .env("RUSTSYNC_E2E_SERVER_ROOT", root)
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
            url: String::new(),
        };
        let mut output = BufReader::new(server.child.stdout.take().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            assert_ne!(
                output.read_line(&mut line).unwrap(),
                0,
                "server exited before listening"
            );
            if let Some(url) = line.trim().strip_prefix("SERVER_URL=") {
                server.url = url.to_owned();
                return server;
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn cli(server: &Server, root: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_rustsync-cli"))
        .arg("--server-url")
        .arg(&server.url)
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn cli_failure(server: &Server, root: &Path, args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_rustsync-cli"))
        .arg("--server-url")
        .arg(&server.url)
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand"),
        "command must exist: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn doctor_checks_key_material_and_cached_file_contents() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    fs::create_dir(&root).unwrap();
    let server = Server::start(&temp.path().join("server"));
    cli(&server, &root, &["init"]);
    fs::write(root.join("note"), b"verified contents").unwrap();
    cli(&server, &root, &["sync"]);
    cli(&server, &root, &["doctor"]);
    let workspace = Workspace::open(&root).unwrap();
    let manifest = rustsync_core::manifest::load_manifest(&workspace).unwrap().unwrap();
    let rustsync_protocol::ManifestEntry::File(file) = &manifest.entries["note"] else { panic!("file"); };
    let cached = rustsync_core::workspace::staged_blob_path(&workspace, &file.content_hash);
    fs::write(&cached, b"corrupted cache").unwrap();
    cli_failure(&server, &root, &["doctor"]);
    assert_eq!(fs::read(&cached).unwrap(), b"corrupted cache", "doctor must not repair state");
    fs::write(cached, b"verified contents").unwrap();
    let key_path = workspace.keyring().unwrap().key_path(workspace.default_key_id());
    let key = fs::read(&key_path).unwrap();
    fs::remove_file(&key_path).unwrap();
    cli_failure(&server, &root, &["doctor"]);
    fs::write(key_path, key).unwrap();
    cli(&server, &root, &["doctor"]);
}

#[test]
fn two_cli_devices_sync_repeatedly_across_server_process_restarts() {
    let temp = tempfile::tempdir().unwrap();
    let owner = temp.path().join("owner");
    let laptop = temp.path().join("laptop");
    let storage = temp.path().join("server");
    fs::create_dir_all(&owner).unwrap();
    fs::create_dir_all(&laptop).unwrap();
    let server = Server::start(&storage);
    let initialized = cli(&server, &owner, &["init"]);
    let owner_id = initialized
        .lines()
        .find_map(|line| line.strip_prefix("device id: "))
        .unwrap();
    fs::write(owner.join("notes.txt"), b"one\ntwo\nthree\n").unwrap();
    cli(&server, &owner, &["sync"]);
    let workspace = Workspace::open(&owner).unwrap();
    let request = cli(
        &server,
        &laptop,
        &["device", "request", workspace.workspace_id().as_str()],
    );
    let request_id = request
        .lines()
        .find_map(|line| line.strip_prefix("join request id: "))
        .unwrap();
    let laptop_id = request
        .lines()
        .find_map(|line| line.strip_prefix("device id: "))
        .unwrap();
    cli(&server, &owner, &["device", "approve", request_id]);
    cli(
        &server,
        &laptop,
        &["device", "bootstrap", workspace.workspace_id().as_str()],
    );
    cli(&server, &laptop, &["sync"]);
    assert_eq!(
        fs::read(laptop.join("notes.txt")).unwrap(),
        b"one\ntwo\nthree\n"
    );

    drop(server);
    let server = Server::start(&storage);
    assert!(cli(&server, &owner, &["remote-status"]).contains("revision 1 "));
    fs::write(owner.join("notes.txt"), b"ONE\ntwo\nthree\n").unwrap();
    fs::write(laptop.join("notes.txt"), b"one\ntwo\nTHREE\n").unwrap();
    cli(&server, &owner, &["sync"]);
    cli(&server, &laptop, &["sync"]);
    cli(&server, &owner, &["sync"]);
    for root in [&owner, &laptop] {
        assert_eq!(
            fs::read(root.join("notes.txt")).unwrap(),
            b"ONE\ntwo\nTHREE\n"
        );
    }
    assert!(cli(&server, &owner, &["remote-status"]).contains("revision 3 "));

    // Fresh client processes with no sleeps must neither replay requests nor
    // create extra publications when both devices already agree.
    for _ in 0..3 {
        cli(&server, &owner, &["sync"]);
        cli(&server, &laptop, &["sync"]);
    }
    assert!(cli(&server, &laptop, &["remote-status"]).contains("revision 3 "));
    drop(server);
    let server = Server::start(&storage);
    fs::remove_file(laptop.join("notes.txt")).unwrap();
    cli(&server, &laptop, &["sync"]);
    cli(&server, &owner, &["sync"]);
    assert!(!owner.join("notes.txt").exists());
    assert!(cli(&server, &owner, &["remote-status"]).contains("revision 4 "));

    cli_failure(&server, &owner, &["device", "remove", owner_id]);
    cli_failure(
        &server,
        &laptop,
        &["device", "set-role", laptop_id, "owner"],
    );
    cli(&server, &owner, &["device", "set-role", laptop_id, "owner"]);
    cli(
        &server,
        &laptop,
        &["device", "set-role", owner_id, "member"],
    );
    cli_failure(&server, &owner, &["device", "remove", laptop_id]);
    cli_failure(
        &server,
        &laptop,
        &["device", "set-role", laptop_id, "member"],
    );
    cli_failure(&server, &laptop, &["device", "remove", laptop_id]);
    cli(&server, &laptop, &["device", "set-role", owner_id, "owner"]);
    cli(
        &server,
        &owner,
        &["device", "set-role", laptop_id, "member"],
    );
    cli(&server, &laptop, &["sync"]);
    cli(&server, &owner, &["device", "remove", laptop_id]);
    assert!(
        cli(&server, &owner, &["device", "remove", laptop_id])
            .contains("already has the requested membership")
    );
    cli_failure(&server, &laptop, &["sync"]);
    drop(server);
    let server = Server::start(&storage);
    cli_failure(&server, &laptop, &["remote-status"]);
    let listing = cli(&server, &owner, &["device", "list"]);
    assert!(
        listing
            .lines()
            .any(|line| line.contains(laptop_id) && line.contains("membership_status=Removed"))
    );
}
