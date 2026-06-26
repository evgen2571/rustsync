use std::{collections::HashMap, path::PathBuf};

use rustsync_client::{ClientConfig, ClientError, ClientResult, RequestSigner, RustSyncClient};
use rustsync_core::{
    device::{DeviceIdentity, load_local_device_identity},
    manifest::{manifest_from_json_bytes, manifest_to_json_bytes},
    workspace::{LocalWorkspaceEngine, Workspace},
};
use rustsync_protocol::{BlobId, DeviceId, Manifest, ManifestEntry, ManifestId};
use url::Url;

pub const SERVER_BASE_URL: &str = "http://127.0.0.1:3000";

type CommandResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub async fn push(path: PathBuf) -> CommandResult {
    let engine = LocalWorkspaceEngine::open(path)?;
    let workspace = engine.workspace();
    let client = client_for_workspace(workspace)?;
    let manifest = engine.load_staged_manifest()?;

    let mut uploaded_blobs = 0usize;
    for blob in engine.staged_blobs_for_manifest(&manifest)? {
        let blob_id = BlobId::parse(format!("blob_{}", blob.content_hash))?;
        client
            .upload_blob(workspace.workspace_id(), &blob_id, blob.bytes)
            .await?;
        uploaded_blobs += 1;
    }

    let manifest_bytes = manifest_to_json_bytes(&manifest)?;
    let manifest_id = ManifestId::from_content(&manifest_bytes);
    client
        .upload_manifest(workspace.workspace_id(), &manifest_id, &manifest_bytes)
        .await?;

    let head = client
        .fetch_workspace_head(workspace.workspace_id())
        .await?;
    let updated_head = client
        .update_workspace_head(workspace.workspace_id(), head.revision, &manifest_id)
        .await?;

    println!(
        "pushed staged snapshot: {uploaded_blobs} blob(s), manifest {manifest_id}; remote head is now revision {}",
        updated_head.revision
    );

    Ok(())
}

pub async fn pull(path: PathBuf) -> CommandResult {
    let engine = LocalWorkspaceEngine::open(path)?;
    let workspace = engine.workspace();
    let client = client_for_workspace(workspace)?;
    let head = client
        .fetch_workspace_head(workspace.workspace_id())
        .await?;
    let Some(manifest_id) = head.manifest_id else {
        let manifest = Manifest::new(workspace.workspace_id().clone());
        engine.apply_pulled_manifest(&manifest, |_content_hash| {
            Err::<Vec<u8>, _>(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "empty manifest has no blobs",
            ))
        })?;
        println!(
            "pulled empty remote workspace from revision {}",
            head.revision
        );
        return Ok(());
    };

    let manifest_bytes = client
        .download_manifest(workspace.workspace_id(), &manifest_id)
        .await?;
    let manifest = manifest_from_json_bytes(&manifest_bytes)?;
    engine.validate_pulled_manifest(&manifest)?;

    let mut blobs = HashMap::new();
    for entry in manifest.entries.values() {
        let ManifestEntry::File(file) = entry else {
            continue;
        };
        let blob_id = BlobId::parse(format!("blob_{}", file.content_hash))?;
        let bytes = client
            .download_blob(workspace.workspace_id(), &blob_id)
            .await?;
        blobs.insert(file.content_hash.clone(), bytes);
    }

    engine.apply_pulled_manifest(&manifest, |content_hash| {
        blobs.get(content_hash).cloned().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("missing downloaded blob {content_hash}"),
            )
        })
    })?;

    println!(
        "force-pulled manifest {manifest_id} from remote revision {}",
        head.revision
    );

    Ok(())
}

fn client_for_workspace(
    workspace: &Workspace,
) -> Result<RustSyncClient<DeviceIdentitySigner>, Box<dyn std::error::Error>> {
    let base_url = Url::parse(SERVER_BASE_URL)?;
    let identity = load_local_device_identity(&workspace.layout.device_identity_path)?
        .ok_or("missing local device identity; re-run `rustsync init` for this workspace")?;
    let config = ClientConfig::new(base_url);

    Ok(RustSyncClient::new(config, DeviceIdentitySigner(identity)))
}

struct DeviceIdentitySigner(DeviceIdentity);

impl RequestSigner for DeviceIdentitySigner {
    fn device_id(&self) -> &DeviceId {
        self.0.device_id()
    }

    fn sign(&self, canonical_request: &[u8]) -> ClientResult<Vec<u8>> {
        self.0
            .sign(canonical_request)
            .map_err(|error| ClientError::Signing(error.to_string()))
    }
}
