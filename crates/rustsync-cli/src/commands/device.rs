use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use rustsync_client::{ClientConfig, RustSyncClient};
use rustsync_core::{
    access::{open_key_envelope, save_access_state, seal_key_envelope},
    device::{DeviceIdentity, load_local_device_identity, save_local_device_identity},
    workspace::{KeyVisibility, Workspace, WorkspaceLayout},
};
use rustsync_protocol::{
    AccessEvent, AccessState, DeviceId, DeviceJoinRequest, DeviceStatus, JoinRequestId,
    JoinRequestSubmissionStatus, KeyEnvelope, KeyId, ObjectUploadStatus, SYSTEM_KEY_ID,
    SignedAccessEvent, UnixTimestamp, WorkspaceId, WorkspaceRole, id::AccessEventId,
};
use url::Url;

use crate::cli::DeviceRoleArg;
use crate::local_device_signer::LocalDeviceRequestSigner;

pub async fn request(
    path: PathBuf,
    workspace_id: WorkspaceId,
    device_name: Option<String>,
    base_url: &Url,
) -> Result<(), Box<dyn Error>> {
    let layout = WorkspaceLayout::new(&path);
    let identity = load_or_create_joining_identity(&layout, device_name)?;
    let join_request = identity.create_join_request(workspace_id.clone())?;
    let request_id = join_request.request_id.clone();
    let device_id = identity.device_id().clone();
    let device_name = identity.device_name().to_owned();
    let fingerprint = identity.fingerprint();

    let client = client_for_identity(identity, base_url)?;
    let response = client.submit_join_request(&join_request).await?;

    println!("submitted device join request");
    println!("workspace id: {workspace_id}");
    println!("join request id: {}", response.request_id);
    println!("status: {}", submission_status_name(response.status));
    println!("device id: {device_id}");
    println!("device name: {device_name}");
    println!("device fingerprint: {fingerprint}");
    println!("identity path: {}", layout.device_identity_path.display());
    if response.request_id != request_id {
        println!(
            "note: server reported an existing pending request id; local request id was {request_id}"
        );
    }
    println!(
        "pending approval: ask an active managing device to run `rustsync device approve {}` in the workspace",
        response.request_id
    );
    println!(
        "after approval and envelope delivery, run `rustsync device bootstrap {workspace_id}` in this pending directory before using sync commands."
    );

    Ok(())
}

pub async fn list_requests(path: PathBuf, base_url: &Url) -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::open(&path)?;
    let client = client_for_workspace(&workspace, base_url)?;
    let response = client.list_join_requests(workspace.workspace_id()).await?;

    if response.requests.is_empty() {
        println!(
            "no pending device join requests for workspace {}",
            workspace.workspace_id()
        );
        return Ok(());
    }

    println!(
        "pending device join requests for workspace {}:",
        workspace.workspace_id()
    );
    for request in response.requests {
        print_join_request(&request);
    }

    Ok(())
}

pub async fn approve(
    path: PathBuf,
    join_request_id: JoinRequestId,
    role: DeviceRoleArg,
    base_url: &Url,
) -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::open(&path)?;
    let identity = load_identity_for_workspace(&workspace)?;
    let client = client_for_identity(identity.clone(), base_url)?;
    let response = client.list_join_requests(workspace.workspace_id()).await?;
    let join_request = response
        .requests
        .into_iter()
        .find(|request| request.request_id == join_request_id)
        .ok_or_else(|| {
            format!(
                "join request `{join_request_id}` is not pending for workspace {}",
                workspace.workspace_id()
            )
        })?;
    let access_state = client.fetch_access_state(workspace.workspace_id()).await?;
    save_access_state(
        &workspace.layout.access_control_path,
        &access_state.access_state,
    )?;

    let event = signed_device_joined_event(
        &workspace,
        &identity,
        &join_request,
        access_state.access_state.revision(),
        role.into(),
    )?;
    let approved = client
        .approve_join_request(workspace.workspace_id(), &join_request_id, &event)
        .await?;
    save_access_state(
        &workspace.layout.access_control_path,
        &approved.access_state,
    )?;

    let envelope = seal_default_key_envelope(
        &workspace,
        &approved.access_state,
        &identity,
        &join_request.device.device_id,
    )?;
    let delivery = client.upload_key_envelope(&envelope).await.map_err(|error| {
        format!(
            "join request `{join_request_id}` was approved for workspace {}, but key-envelope delivery to device {} failed: {error}",
            workspace.workspace_id(),
            join_request.device.device_id,
        )
    })?;

    println!("approved device join request");
    println!("workspace id: {}", workspace.workspace_id());
    println!("join request id: {join_request_id}");
    println!("device id: {}", join_request.device.device_id);
    println!("new access revision: {}", approved.access_state.revision());
    println!("key envelope id: {}", envelope.key_id);
    println!(
        "key envelope delivery status: {}",
        upload_status_name(delivery.status)
    );

    Ok(())
}

pub async fn bootstrap(
    path: PathBuf,
    workspace_id: WorkspaceId,
    base_url: &Url,
) -> Result<(), Box<dyn Error>> {
    let layout = WorkspaceLayout::new(&path);
    require_pending_identity_layout(&layout)?;
    let identity = load_local_device_identity(&layout.device_identity_path)?.ok_or_else(|| {
        "missing pending local device identity; run `rustsync device request` first".to_string()
    })?;
    let client = client_for_identity(identity.clone(), base_url)?;
    let response = client.fetch_access_state(&workspace_id).await?;
    let state = response.access_state;

    if state.workspace_id() != &workspace_id {
        return Err(format!(
            "access state workspace `{}` does not match requested workspace `{workspace_id}`",
            state.workspace_id()
        )
        .into());
    }
    require_active_identity(&state, &identity)?;

    let key_id = KeyId::parse(SYSTEM_KEY_ID)?;
    let envelope = client
        .download_key_envelope(&workspace_id, &key_id, identity.device_id())
        .await?;
    let workspace = bootstrap_pending_workspace(path, identity, state, envelope)?;

    println!("bootstrapped workspace");
    println!("workspace id: {}", workspace.workspace_id());
    println!("device id: {}", workspace.local_device_id());
    println!("key id: {}", workspace.default_key_id());
    println!("status: complete");
    Ok(())
}

fn seal_default_key_envelope(
    workspace: &Workspace,
    state: &AccessState,
    sender: &DeviceIdentity,
    recipient_device_id: &DeviceId,
) -> Result<KeyEnvelope, Box<dyn Error>> {
    let key_id = workspace.default_key_id();
    if key_id.as_str() != SYSTEM_KEY_ID {
        return Err("default workspace key is not the system bootstrap key".into());
    }
    let record = workspace.keyring()?.get(key_id)?.clone();
    if record.visibility != KeyVisibility::Shared {
        return Err("system bootstrap key must have shared visibility".into());
    }
    let workspace_key = workspace.load_key(key_id)?;
    let sender_record = state.active_device_record(sender.device_id())?;
    let recipient_record = state.active_device_record(recipient_device_id)?;

    Ok(seal_key_envelope(
        state,
        KeyVisibility::Shared,
        key_id.clone(),
        record.generation,
        &workspace_key,
        sender,
        sender_record,
        recipient_record,
        UnixTimestamp::now(),
    )?)
}

fn require_active_identity(
    state: &AccessState,
    identity: &DeviceIdentity,
) -> Result<(), Box<dyn Error>> {
    let record = state.active_device_record(identity.device_id())?;
    if record.signing_public_key != *identity.signing_public_key()
        || record.exchange_public_key != *identity.exchange_public_key()
    {
        return Err("local device identity does not match the active access-state record".into());
    }
    Ok(())
}

#[cfg(test)]
fn bootstrap_from_envelope(
    path: PathBuf,
    identity: DeviceIdentity,
    state: AccessState,
    envelope: KeyEnvelope,
) -> Result<Workspace, Box<dyn Error>> {
    let key_id = KeyId::parse(SYSTEM_KEY_ID)?;
    if envelope.key_id != key_id {
        return Err("bootstrap requires the system key envelope".into());
    }
    let recipient_record = state.active_device_record(identity.device_id())?;
    let sender_record = state.active_device_record(&envelope.sender_device_id)?;
    let workspace_key = open_key_envelope(
        &state,
        KeyVisibility::Shared,
        &envelope,
        &identity,
        recipient_record,
        sender_record,
    )?;

    Ok(Workspace::bootstrap_with_key(
        path,
        &identity,
        &state,
        key_id,
        envelope.key_generation,
        &workspace_key,
    )?)
}

fn bootstrap_pending_workspace(
    path: PathBuf,
    identity: DeviceIdentity,
    state: AccessState,
    envelope: KeyEnvelope,
) -> Result<Workspace, Box<dyn Error>> {
    bootstrap_pending_workspace_with_filesystem(
        path,
        identity,
        state,
        envelope,
        &StandardBootstrapFilesystem,
    )
}

trait BootstrapFilesystem {
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()>;
}

struct StandardBootstrapFilesystem;

impl BootstrapFilesystem for StandardBootstrapFilesystem {
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_dir_all(path)
    }
}

fn bootstrap_pending_workspace_with_filesystem(
    path: PathBuf,
    identity: DeviceIdentity,
    state: AccessState,
    envelope: KeyEnvelope,
    filesystem: &impl BootstrapFilesystem,
) -> Result<Workspace, Box<dyn Error>> {
    let layout = WorkspaceLayout::new(&path);
    require_pending_identity_layout(&layout)?;

    // Verify the remotely supplied envelope before changing the pending local state.
    let key_id = KeyId::parse(SYSTEM_KEY_ID)?;
    let recipient_record = state.active_device_record(identity.device_id())?;
    let sender_record = state.active_device_record(&envelope.sender_device_id)?;
    let workspace_key = open_key_envelope(
        &state,
        KeyVisibility::Shared,
        &envelope,
        &identity,
        recipient_record,
        sender_record,
    )?;
    if envelope.key_id != key_id {
        return Err("bootstrap requires the system key envelope".into());
    }

    let backup = path.join(".rustsync.bootstrap-backup");
    if backup.try_exists()? {
        return Err(format!("bootstrap backup path already exists: {}", backup.display()).into());
    }
    filesystem.rename(&layout.rustsync_dir, &backup)?;
    let result = Workspace::bootstrap_with_key(
        &path,
        &identity,
        &state,
        key_id,
        envelope.key_generation,
        &workspace_key,
    );
    match result {
        Ok(workspace) => {
            if let Err(error) = filesystem.remove_dir_all(&backup) {
                eprintln!(
                    "warning: workspace bootstrap succeeded, but stale pending identity backup remains at {}: {error}",
                    backup.display()
                );
            }
            Ok(workspace)
        }
        Err(error) => restore_pending_identity(&layout, &backup, error, filesystem),
    }
}

fn restore_pending_identity(
    layout: &WorkspaceLayout,
    backup: &Path,
    bootstrap_error: impl std::fmt::Display,
    filesystem: &impl BootstrapFilesystem,
) -> Result<Workspace, Box<dyn Error>> {
    if layout.rustsync_dir.try_exists()? {
        filesystem.remove_dir_all(&layout.rustsync_dir).map_err(|error| {
            format!(
                "bootstrap failed: {bootstrap_error}; failed to remove partial workspace metadata at {} before restoring pending local identity from {}: {error}",
                layout.rustsync_dir.display(),
                backup.display()
            )
        })?;
    }

    filesystem
        .rename(backup, &layout.rustsync_dir)
        .map_err(|error| {
            format!(
                "bootstrap failed: {bootstrap_error}; failed to restore pending local identity from {} to {}: {error}. The pending identity remains in the backup directory.",
                backup.display(),
                layout.rustsync_dir.display()
            )
        })?;

    Err(
        format!("bootstrap failed and pending local identity was restored: {bootstrap_error}")
            .into(),
    )
}

fn require_pending_identity_layout(layout: &WorkspaceLayout) -> Result<(), Box<dyn Error>> {
    if !layout.rustsync_dir.try_exists()? || !layout.device_identity_path.try_exists()? {
        return Err(
            "missing pending local identity in `.rustsync`; run `rustsync device request` first"
                .into(),
        );
    }
    if layout.config_path.try_exists()? {
        return Err(
            "workspace is already initialized; refusing to bootstrap over existing metadata".into(),
        );
    }
    let mut entries = fs::read_dir(&layout.rustsync_dir)?;
    let only_identity = matches!(entries.next(), Some(Ok(entry)) if entry.path() == layout.device_identity_path)
        && entries.next().is_none();
    if !only_identity {
        return Err(
            "pending `.rustsync` contains unexpected workspace metadata; refusing to overwrite it"
                .into(),
        );
    }
    Ok(())
}

pub async fn list(path: PathBuf, base_url: &Url) -> Result<(), Box<dyn Error>> {
    let workspace = Workspace::open(&path)?;
    let client = client_for_workspace(&workspace, base_url)?;
    let response = client.fetch_access_state(workspace.workspace_id()).await?;
    save_access_state(
        &workspace.layout.access_control_path,
        &response.access_state,
    )?;

    println!("devices for workspace {}:", workspace.workspace_id());
    for membership in response.access_state.all_memberships() {
        let Some(device) = response.access_state.device_record(&membership.device_id) else {
            println!(
                "- {} role={} membership_status={:?} device_record=<missing>",
                membership.device_id,
                role_name(membership.role),
                membership.status
            );
            continue;
        };
        println!(
            "- {} name=\"{}\" role={} membership_status={:?} device_status={} fingerprint={}",
            device.device_id,
            device.device_name,
            role_name(membership.role),
            membership.status,
            device_status_name(device.status),
            device.fingerprint
        );
    }

    Ok(())
}

fn load_or_create_joining_identity(
    layout: &WorkspaceLayout,
    device_name: Option<String>,
) -> Result<DeviceIdentity, Box<dyn Error>> {
    if let Some(identity) = load_local_device_identity(&layout.device_identity_path)? {
        return Ok(identity);
    }

    fs::create_dir_all(&layout.rustsync_dir)?;
    let identity = DeviceIdentity::generate(device_name.unwrap_or_default())?;
    save_local_device_identity(&layout.device_identity_path, &identity)?;
    Ok(identity)
}

fn load_identity_for_workspace(workspace: &Workspace) -> Result<DeviceIdentity, Box<dyn Error>> {
    load_local_device_identity(&workspace.layout.device_identity_path)?.ok_or_else(|| {
        "missing local device identity; re-run `rustsync init` for this workspace".into()
    })
}

fn client_for_workspace(
    workspace: &Workspace,
    base_url: &Url,
) -> Result<RustSyncClient<LocalDeviceRequestSigner>, Box<dyn Error>> {
    let identity = load_identity_for_workspace(workspace)?;
    client_for_identity(identity, base_url)
}

fn client_for_identity(
    identity: DeviceIdentity,
    base_url: &Url,
) -> Result<RustSyncClient<LocalDeviceRequestSigner>, Box<dyn Error>> {
    Ok(RustSyncClient::new(
        ClientConfig::new(base_url.clone()),
        LocalDeviceRequestSigner::new(identity),
    ))
}

fn signed_device_joined_event(
    workspace: &Workspace,
    actor: &DeviceIdentity,
    join_request: &DeviceJoinRequest,
    expected_revision: u64,
    role: WorkspaceRole,
) -> Result<SignedAccessEvent, Box<dyn Error>> {
    let event_id = AccessEventId::parse(format!("event_{}", join_request.request_id.as_str()))?;
    let event = AccessEvent::DeviceJoined {
        join_request_id: join_request.request_id.clone(),
        device: join_request.device.clone(),
        role,
    };
    let unsigned = SignedAccessEvent::new_unsigned(
        event_id,
        workspace.workspace_id().clone(),
        expected_revision,
        actor.device_id().clone(),
        UnixTimestamp::now(),
        event,
    );
    let signature = actor.sign(&unsigned.signing_payload())?;
    Ok(unsigned.with_signature(signature))
}

fn print_join_request(request: &DeviceJoinRequest) {
    println!("- join request id: {}", request.request_id);
    println!("  device id: {}", request.device.device_id);
    println!("  device name: {}", request.device.device_name);
    println!("  fingerprint: {}", request.device.fingerprint);
    println!("  created at: {}", request.created_at.as_secs());
}

fn submission_status_name(status: JoinRequestSubmissionStatus) -> &'static str {
    match status {
        JoinRequestSubmissionStatus::Submitted => "submitted",
        JoinRequestSubmissionStatus::AlreadyPending => "already_pending",
    }
}

fn upload_status_name(status: ObjectUploadStatus) -> &'static str {
    match status {
        ObjectUploadStatus::Created => "created",
        ObjectUploadStatus::AlreadyExists => "already_exists",
    }
}

fn role_name(role: WorkspaceRole) -> &'static str {
    match role {
        WorkspaceRole::Owner => "owner",
        WorkspaceRole::Member => "member",
    }
}

fn device_status_name(status: DeviceStatus) -> &'static str {
    match status {
        DeviceStatus::Pending => "pending",
        DeviceStatus::Active => "active",
        DeviceStatus::Revoked => "revoked",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustsync_core::access::load_access_state;
    use rustsync_protocol::{AccessEvent, KeyId, WorkspaceRole, id::AccessEventId};
    use tempfile::tempdir;

    fn approved_state(
        workspace: &Workspace,
        owner: &DeviceIdentity,
        joining: &DeviceIdentity,
    ) -> rustsync_protocol::AccessState {
        let mut state = load_access_state(&workspace.layout.access_control_path)
            .expect("load access state")
            .expect("access state");
        let event = AccessEvent::DeviceJoined {
            join_request_id: JoinRequestId::parse("join_bootstrap_test").expect("join id"),
            device: joining.public_record(DeviceStatus::Pending),
            role: WorkspaceRole::Member,
        };
        let unsigned = SignedAccessEvent::new_unsigned(
            AccessEventId::parse("event_bootstrap_test").expect("event id"),
            workspace.workspace_id().clone(),
            state.revision(),
            owner.device_id().clone(),
            UnixTimestamp::from_secs(1),
            event,
        );
        let signature = owner.sign(&unsigned.signing_payload()).expect("sign event");
        state
            .apply_verified_event(&unsigned.with_signature(signature))
            .expect("approve joining device");
        state
    }

    #[test]
    fn seals_default_key_envelope_for_approved_active_recipient() {
        let source = tempdir().expect("source directory");
        let owner = DeviceIdentity::generate("owner").expect("owner identity");
        let workspace = Workspace::init_with_device_identity(source.path(), &owner)
            .expect("initialize source workspace");
        let joining = DeviceIdentity::generate("joining").expect("joining identity");
        let state = approved_state(&workspace, &owner, &joining);

        let envelope = seal_default_key_envelope(&workspace, &state, &owner, joining.device_id())
            .expect("seal approved envelope");

        assert_eq!(envelope.workspace_id, *workspace.workspace_id());
        assert_eq!(
            envelope.key_id,
            KeyId::parse(SYSTEM_KEY_ID).expect("system key id")
        );
        assert_eq!(envelope.recipient_device_id, *joining.device_id());
        assert_eq!(envelope.access_revision, state.revision());
        assert!(!envelope.encrypted_workspace_key.is_empty());
    }

    #[test]
    fn bootstrap_from_approved_envelope_writes_workspace_metadata() {
        let source = tempdir().expect("source directory");
        let owner = DeviceIdentity::generate("owner").expect("owner identity");
        let workspace = Workspace::init_with_device_identity(source.path(), &owner)
            .expect("initialize source workspace");
        let joining = DeviceIdentity::generate("joining").expect("joining identity");
        let state = approved_state(&workspace, &owner, &joining);
        let envelope = seal_default_key_envelope(&workspace, &state, &owner, joining.device_id())
            .expect("seal approved envelope");
        let target = tempdir().expect("target directory");

        let bootstrapped =
            bootstrap_from_envelope(target.path().to_path_buf(), joining, state, envelope)
                .expect("bootstrap from approved envelope");

        assert_eq!(bootstrapped.workspace_id(), workspace.workspace_id());
        assert_eq!(bootstrapped.default_key_id().as_str(), SYSTEM_KEY_ID);
        assert!(
            bootstrapped
                .keyring()
                .expect("keyring")
                .contains(bootstrapped.default_key_id())
        );
    }

    #[test]
    fn invalid_bootstrap_envelope_leaves_pending_identity_untouched() {
        let source = tempdir().expect("source directory");
        let owner = DeviceIdentity::generate("owner").expect("owner identity");
        let workspace = Workspace::init_with_device_identity(source.path(), &owner)
            .expect("initialize source workspace");
        let joining = DeviceIdentity::generate("joining").expect("joining identity");
        let state = approved_state(&workspace, &owner, &joining);
        let mut envelope =
            seal_default_key_envelope(&workspace, &state, &owner, joining.device_id())
                .expect("seal approved envelope");
        envelope.signature[0] ^= 1;
        let target = tempdir().expect("target directory");
        let layout = WorkspaceLayout::new(target.path());
        fs::create_dir(&layout.rustsync_dir).expect("create pending metadata directory");
        save_local_device_identity(&layout.device_identity_path, &joining)
            .expect("save pending identity");

        bootstrap_pending_workspace(target.path().to_path_buf(), joining, state, envelope)
            .expect_err("invalid envelope must fail before changing pending metadata");

        assert!(layout.device_identity_path.exists());
        assert!(!layout.config_path.exists());
        assert!(!target.path().join(".rustsync.bootstrap-backup").exists());
    }

    #[test]
    fn bootstrap_refuses_existing_workspace_without_overwriting_metadata() {
        let target = tempdir().expect("target directory");
        let owner = DeviceIdentity::generate("owner").expect("owner identity");
        let existing = Workspace::init_with_device_identity(target.path(), &owner)
            .expect("initialize existing workspace");
        let original_config = fs::read(&existing.layout.config_path).expect("read config");

        require_pending_identity_layout(&existing.layout)
            .expect_err("existing workspace must not be treated as pending");

        assert_eq!(
            fs::read(&existing.layout.config_path).expect("read config"),
            original_config
        );
    }

    struct FailingBackupCleanupFilesystem;

    impl BootstrapFilesystem for FailingBackupCleanupFilesystem {
        fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
            fs::rename(from, to)
        }

        fn remove_dir_all(&self, path: &std::path::Path) -> std::io::Result<()> {
            if path
                .file_name()
                .is_some_and(|name| name == ".rustsync.bootstrap-backup")
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected stale backup cleanup failure",
                ));
            }
            fs::remove_dir_all(path)
        }
    }

    #[test]
    fn bootstrap_succeeds_when_stale_backup_cleanup_fails() {
        let source = tempdir().expect("source directory");
        let owner = DeviceIdentity::generate("owner").expect("owner identity");
        let workspace = Workspace::init_with_device_identity(source.path(), &owner)
            .expect("initialize source workspace");
        let joining = DeviceIdentity::generate("joining").expect("joining identity");
        let state = approved_state(&workspace, &owner, &joining);
        let envelope = seal_default_key_envelope(&workspace, &state, &owner, joining.device_id())
            .expect("seal approved envelope");
        let target = tempdir().expect("target directory");
        let layout = WorkspaceLayout::new(target.path());
        fs::create_dir(&layout.rustsync_dir).expect("create pending metadata directory");
        save_local_device_identity(&layout.device_identity_path, &joining)
            .expect("save pending identity");

        let bootstrapped = bootstrap_pending_workspace_with_filesystem(
            target.path().to_path_buf(),
            joining,
            state,
            envelope,
            &FailingBackupCleanupFilesystem,
        )
        .expect("a usable workspace must be reported as bootstrapped");

        assert_eq!(bootstrapped.workspace_id(), workspace.workspace_id());
        assert!(layout.config_path.exists());
        assert!(target.path().join(".rustsync.bootstrap-backup").exists());
    }

    struct FailingRollbackFilesystem;

    impl BootstrapFilesystem for FailingRollbackFilesystem {
        fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
            if from
                .file_name()
                .is_some_and(|name| name == ".rustsync.bootstrap-backup")
                && to.file_name().is_some_and(|name| name == ".rustsync")
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected rollback restore failure",
                ));
            }
            fs::rename(from, to)
        }

        fn remove_dir_all(&self, path: &std::path::Path) -> std::io::Result<()> {
            fs::remove_dir_all(path)
        }
    }

    #[test]
    fn bootstrap_reports_rollback_failure_and_retains_pending_identity_in_backup() {
        let joining = DeviceIdentity::generate("joining").expect("joining identity");
        let target = tempdir().expect("target directory");
        let layout = WorkspaceLayout::new(target.path());
        fs::create_dir(&layout.rustsync_dir).expect("create pending metadata directory");
        save_local_device_identity(&layout.device_identity_path, &joining)
            .expect("save pending identity");

        let backup = target.path().join(".rustsync.bootstrap-backup");
        fs::rename(&layout.rustsync_dir, &backup).expect("move pending identity to backup");
        fs::create_dir(&layout.rustsync_dir).expect("create partial metadata");

        let error = restore_pending_identity(
            &layout,
            &backup,
            "injected bootstrap failure",
            &FailingRollbackFilesystem,
        )
        .expect_err("a rollback failure must be reported");

        assert!(
            error
                .to_string()
                .contains("failed to restore pending local identity")
        );
        let backup_identity = backup.join(
            layout
                .device_identity_path
                .file_name()
                .expect("identity file name"),
        );
        assert!(backup_identity.exists());
        assert!(!layout.rustsync_dir.exists());
    }

    #[test]
    fn bootstrap_failure_restores_pending_identity_after_removing_partial_metadata() {
        let joining = DeviceIdentity::generate("joining").expect("joining identity");
        let target = tempdir().expect("target directory");
        let layout = WorkspaceLayout::new(target.path());
        fs::create_dir(&layout.rustsync_dir).expect("create pending metadata directory");
        save_local_device_identity(&layout.device_identity_path, &joining)
            .expect("save pending identity");
        let backup = target.path().join(".rustsync.bootstrap-backup");
        fs::rename(&layout.rustsync_dir, &backup).expect("move pending identity to backup");
        fs::create_dir(&layout.rustsync_dir).expect("create partial metadata");
        fs::write(layout.rustsync_dir.join("partial"), "partial").expect("write partial metadata");

        restore_pending_identity(
            &layout,
            &backup,
            "injected bootstrap failure",
            &StandardBootstrapFilesystem,
        )
        .expect_err("the original bootstrap failure must be returned after restoration");

        let restored = load_local_device_identity(&layout.device_identity_path)
            .expect("load restored identity")
            .expect("pending identity restored");
        assert_eq!(restored.device_id(), joining.device_id());
        assert!(!backup.exists());
    }
}
