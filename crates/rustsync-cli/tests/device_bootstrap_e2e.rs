use std::fs;

use rustsync_cli::commands::{device, init, sync};
use rustsync_client::{ClientConfig, ClientError, ClientResult, RequestSigner, RustSyncClient};
use rustsync_core::{
    device::{DeviceIdentity, load_local_device_identity},
    workspace::Workspace,
};
use rustsync_protocol::DeviceId;
use rustsync_server::{AppState, IndexedFsStorage, create_app};
use tempfile::tempdir;
use tokio::{net::TcpListener, task::JoinHandle};
use url::Url;

struct IdentitySigner(DeviceIdentity);

impl RequestSigner for IdentitySigner {
    fn device_id(&self) -> &DeviceId {
        self.0.device_id()
    }

    fn sign(&self, canonical_request: &[u8]) -> ClientResult<Vec<u8>> {
        self.0
            .sign(canonical_request)
            .map_err(|error| ClientError::Signing(error.to_string()))
    }
}

async fn spawn_server(storage_root: std::path::PathBuf) -> (Url, JoinHandle<()>) {
    let storage = IndexedFsStorage::open(storage_root).await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let base_url = Url::parse(&format!(
        "http://{}",
        listener.local_addr().expect("test server address")
    ))
    .expect("parse server URL");
    let server = tokio::spawn(async move {
        axum::serve(listener, create_app(AppState::new(storage)))
            .await
            .expect("serve test server");
    });

    (base_url, server)
}

#[tokio::test]
async fn second_device_bootstraps_from_owner_envelope_then_syncs() {
    let temp = tempdir().expect("temp dir");
    let owner_dir = temp.path().join("owner");
    let joining_dir = temp.path().join("joining");
    fs::create_dir_all(&owner_dir).expect("create owner directory");
    fs::create_dir_all(&joining_dir).expect("create joining directory");
    let (server_url, server) = spawn_server(temp.path().join("server-storage")).await;

    init::run(owner_dir.clone(), &server_url)
        .await
        .expect("owner initializes remote workspace");
    fs::write(owner_dir.join("shared.txt"), b"owner's first version").expect("write owner file");
    sync::sync(
        owner_dir.clone(),
        false,
        false,
        &server_url,
        sync::OutputOptions::default(),
    )
    .await
    .expect("owner syncs initial snapshot");

    let owner_workspace = Workspace::open(&owner_dir).expect("open owner workspace");
    let owner_identity = load_local_device_identity(&owner_workspace.layout.device_identity_path)
        .expect("load owner identity")
        .expect("owner identity exists");
    let workspace_id = owner_workspace.workspace_id().clone();

    device::request(
        joining_dir.clone(),
        workspace_id.clone(),
        Some("second device".to_string()),
        &server_url,
    )
    .await
    .expect("second device requests access");

    let owner_client = RustSyncClient::new(
        ClientConfig::new(server_url.clone()),
        IdentitySigner(owner_identity),
    );
    let requests = owner_client
        .list_join_requests(&workspace_id)
        .await
        .expect("owner lists pending requests");
    let join_request = requests
        .requests
        .into_iter()
        .next()
        .expect("pending request");

    device::approve(
        owner_dir.clone(),
        join_request.request_id,
        rustsync_cli::cli::DeviceRoleArg::Member,
        &server_url,
    )
    .await
    .expect("owner approves request and uploads key envelope");

    device::bootstrap(joining_dir.clone(), workspace_id.clone(), &server_url)
        .await
        .expect("second device bootstraps with delivered envelope");
    sync::sync(
        joining_dir.clone(),
        false,
        false,
        &server_url,
        sync::OutputOptions::default(),
    )
    .await
    .expect("second device syncs owner's snapshot");
    assert_eq!(
        fs::read(joining_dir.join("shared.txt")).expect("read pulled file"),
        b"owner's first version"
    );

    fs::write(joining_dir.join("shared.txt"), b"second device edit").expect("edit pulled file");
    sync::sync(
        joining_dir.clone(),
        false,
        false,
        &server_url,
        sync::OutputOptions::default(),
    )
    .await
    .expect("second device syncs edit");

    let head = owner_client
        .fetch_workspace_head(&workspace_id)
        .await
        .expect("fetch remote head after second device sync");
    assert_eq!(head.revision, 2);
    assert!(head.manifest_id.is_some());

    sync::sync(
        owner_dir.clone(),
        false,
        false,
        &server_url,
        sync::OutputOptions::default(),
    )
    .await
    .unwrap();
    fs::write(owner_dir.join("shared.txt"), b"one\ntwo\nthree\n").unwrap();
    sync::sync(
        owner_dir.clone(),
        false,
        false,
        &server_url,
        sync::OutputOptions::default(),
    )
    .await
    .unwrap();
    sync::sync(
        joining_dir.clone(),
        false,
        false,
        &server_url,
        sync::OutputOptions::default(),
    )
    .await
    .unwrap();
    fs::write(owner_dir.join("shared.txt"), b"ONE\ntwo\nthree\n").unwrap();
    fs::write(joining_dir.join("shared.txt"), b"one\ntwo\nTHREE\n").unwrap();
    sync::sync(
        owner_dir.clone(),
        false,
        false,
        &server_url,
        sync::OutputOptions::default(),
    )
    .await
    .unwrap();
    sync::sync(
        joining_dir.clone(),
        false,
        false,
        &server_url,
        sync::OutputOptions::default(),
    )
    .await
    .unwrap();
    sync::sync(
        owner_dir.clone(),
        false,
        false,
        &server_url,
        sync::OutputOptions::default(),
    )
    .await
    .unwrap();
    for root in [&owner_dir, &joining_dir] {
        assert_eq!(
            fs::read(root.join("shared.txt")).unwrap(),
            b"ONE\ntwo\nTHREE\n"
        );
        assert!(
            rustsync_core::workspace::LocalWorkspaceEngine::open(root)
                .unwrap()
                .load_sync_state()
                .unwrap()
                .conflicts
                .is_empty()
        );
    }

    fs::remove_file(owner_dir.join("shared.txt")).unwrap();
    fs::write(joining_dir.join("shared.txt"), b"unsynced edit").unwrap();
    sync::sync(
        owner_dir.clone(),
        false,
        false,
        &server_url,
        sync::OutputOptions::default(),
    )
    .await
    .unwrap();
    sync::sync(
        joining_dir.clone(),
        false,
        false,
        &server_url,
        sync::OutputOptions::default(),
    )
    .await
    .unwrap();
    let joining_engine =
        rustsync_core::workspace::LocalWorkspaceEngine::open(&joining_dir).unwrap();
    assert!(
        joining_engine
            .load_sync_state()
            .unwrap()
            .conflicts
            .contains_key("shared.txt")
    );
    sync::resolve(joining_dir.clone(), "shared.txt".into(), false, true).unwrap();
    sync::sync(
        joining_dir.clone(),
        false,
        false,
        &server_url,
        sync::OutputOptions::default(),
    )
    .await
    .unwrap();
    sync::sync(
        owner_dir.clone(),
        false,
        false,
        &server_url,
        sync::OutputOptions::default(),
    )
    .await
    .unwrap();
    assert!(!owner_dir.join("shared.txt").exists());
    assert!(!joining_dir.join("shared.txt").exists());
    assert!(
        joining_engine
            .load_sync_state()
            .unwrap()
            .conflicts
            .is_empty()
    );

    server.abort();
}
