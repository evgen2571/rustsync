use std::{
    fs,
    path::{Path, PathBuf},
};

use rustsync_client::{ClientConfig, ClientError, ClientResult, RequestSigner, RustSyncClient};
use rustsync_core::{
    device::{load_local_device_identity, DeviceIdentity},
    manifest::{
        load_manifest, manifest_from_json_bytes, manifest_to_json_bytes, save_manifest,
        validate_manifest_workspace,
    },
    workspace::{open_workspace, Workspace, WORKSPACE_DIR},
};
use rustsync_protocol::{BlobId, DeviceId, Manifest, ManifestEntry, ManifestId};
use url::Url;

use crate::commands::add::staged_blob_path;

pub const SERVER_BASE_URL: &str = "http://127.0.0.1:3000";

type CommandResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub async fn push(path: PathBuf) -> CommandResult {
    let workspace = open_workspace(path)?;
    let client = client_for_workspace(&workspace)?;
    let manifest =
        load_manifest(&workspace)?.ok_or("nothing staged for push; run `rustsync add -A` first")?;
    validate_manifest_workspace(&workspace, &manifest)?;

    let mut uploaded_blobs = 0usize;
    for entry in manifest.entries.values() {
        let ManifestEntry::File(file) = entry else {
            continue;
        };

        let blob_path = staged_blob_path(&workspace, &file.content_hash);
        let bytes = fs::read(&blob_path).map_err(|error| {
            format!(
                "missing staged blob for {}; run `rustsync add -A` again ({error})",
                file.content_hash
            )
        })?;
        let blob_id = BlobId::parse(format!("blob_{}", file.content_hash))?;
        client
            .upload_blob(workspace.workspace_id(), &blob_id, bytes)
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
    let workspace = open_workspace(path)?;
    let client = client_for_workspace(&workspace)?;
    let head = client
        .fetch_workspace_head(workspace.workspace_id())
        .await?;
    let Some(manifest_id) = head.manifest_id else {
        let manifest = Manifest::new(workspace.workspace_id().clone());
        apply_manifest(&workspace, &client, &manifest).await?;
        save_manifest(&workspace, &manifest)?;
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
    validate_manifest_workspace(&workspace, &manifest)?;

    apply_manifest(&workspace, &client, &manifest).await?;
    save_manifest(&workspace, &manifest)?;

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

async fn apply_manifest<S>(
    workspace: &Workspace,
    client: &RustSyncClient<S>,
    manifest: &Manifest,
) -> CommandResult
where
    S: RequestSigner,
{
    remove_entries_missing_from_remote(workspace, manifest)?;

    for (relative_path, entry) in &manifest.entries {
        let target = workspace_path(workspace, relative_path)?;
        match entry {
            ManifestEntry::Directory(_) => prepare_directory_path(&target)?,
            ManifestEntry::File(file) => {
                prepare_file_path(&target)?;

                let blob_id = BlobId::parse(format!("blob_{}", file.content_hash))?;
                let bytes = client
                    .download_blob(workspace.workspace_id(), &blob_id)
                    .await?;
                fs::write(&target, &bytes)?;
                cache_pulled_blob(workspace, &file.content_hash, &bytes)?;
            }
        }
    }

    Ok(())
}

fn remove_entries_missing_from_remote(workspace: &Workspace, remote: &Manifest) -> CommandResult {
    let mut local_paths = Vec::new();
    collect_workspace_paths(
        &workspace.layout.root,
        &workspace.layout.root,
        &mut local_paths,
    )?;
    local_paths.sort_by_key(|path| std::cmp::Reverse(path.matches('/').count()));

    for relative_path in local_paths {
        if remote.entries.contains_key(&relative_path) {
            continue;
        }

        let target = workspace_path(workspace, &relative_path)?;
        remove_path_if_exists(&target)?;
    }

    Ok(())
}

fn collect_workspace_paths(root: &Path, current: &Path, out: &mut Vec<String>) -> CommandResult {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        if path.parent() == Some(root) && file_name == WORKSPACE_DIR {
            continue;
        }

        let relative = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        out.push(relative);

        if entry.file_type()?.is_dir() {
            collect_workspace_paths(root, &path, out)?;
        }
    }

    Ok(())
}

fn prepare_directory_path(path: &Path) -> CommandResult {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            fs::remove_file(path)?;
            fs::create_dir_all(path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(path)?,
        Err(error) => return Err(error.into()),
    }

    Ok(())
}

fn prepare_file_path(path: &Path) -> CommandResult {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    Ok(())
}

fn remove_path_if_exists(path: &Path) -> CommandResult {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    Ok(())
}

fn cache_pulled_blob(workspace: &Workspace, content_hash: &str, bytes: &[u8]) -> CommandResult {
    let target = staged_blob_path(workspace, content_hash);
    if target.try_exists()? {
        return Ok(());
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, bytes)?;

    Ok(())
}

fn workspace_path(workspace: &Workspace, relative_path: &str) -> CommandResult<PathBuf> {
    let path = Path::new(relative_path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!("remote manifest contains unsafe path `{relative_path}`").into());
    }

    Ok(workspace.layout.root.join(path))
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
