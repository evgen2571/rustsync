use std::{error::Error, fs, io::Write, path::PathBuf};

use rustsync_core::workspace::{Workspace, WorkspaceLayout};
use rustsync_protocol::WorkspaceId;
use url::Url;

pub fn invite(path: PathBuf, output: PathBuf, server_url: &Url) -> Result<(), Box<dyn Error>> {
    validate_server(server_url)?;
    let workspace = Workspace::open(path)?;
    let payload = serde_json::json!({
        "format": "rustsync-invite-v1",
        "workspace_id": workspace.workspace_id(),
        "server_url": server_url.as_str(),
    });
    let bytes = serde_json::to_vec_pretty(&payload)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    file.write_all(&bytes)?;
    println!("invite saved to {}", output.display());
    println!("Send this file to the new device. Owner approval is still required.");
    Ok(())
}

fn read_invite(path: &std::path::Path) -> Result<(WorkspaceId, Url), Box<dyn Error>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(16_385).read_to_end(&mut bytes)?;
    if bytes.len() > 16_384 {
        return Err("invite exceeds 16 KiB".into());
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if value["format"].as_str() != Some("rustsync-invite-v1") {
        return Err("unsupported invite format".into());
    }
    let workspace_id = WorkspaceId::parse(
        value["workspace_id"]
            .as_str()
            .ok_or("invite is missing workspace_id")?,
    )?;
    let server_url = Url::parse(
        value["server_url"]
            .as_str()
            .ok_or("invite is missing server_url")?,
    )?;
    validate_server(&server_url)?;
    Ok((workspace_id, server_url))
}

fn validate_server(url: &Url) -> Result<(), Box<dyn Error>> {
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("server URL must be HTTP(S), without credentials, query, or fragment".into());
    }
    Ok(())
}

pub async fn join(
    invite: PathBuf,
    path: PathBuf,
    device_name: Option<String>,
    finish: bool,
    server_override: Option<&Url>,
) -> Result<(), Box<dyn Error>> {
    let (workspace_id, invited_server) = read_invite(&invite)?;
    let server_url = server_override.unwrap_or(&invited_server);
    validate_server(server_url)?;
    if WorkspaceLayout::new(&path).config_path.try_exists()? {
        return Err("workspace is already initialized; choose a new directory".into());
    }
    println!("joining workspace {workspace_id} at {server_url}");
    if finish {
        super::device::bootstrap(path, workspace_id, server_url).await
    } else {
        super::device::request(path, workspace_id, device_name, server_url).await?;
        println!(
            "After owner approval, rerun join with the same invite and directory, adding --finish and omitting --device-name."
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn malformed_invites_do_not_create_device_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("invite");
        let target = temp.path().join("new-device");
        for bytes in [
            b"not json".to_vec(),
            vec![b' '; 16_385],
            br#"{"format":"rustsync-invite-v2","workspace_id":"workspace_test","server_url":"http://localhost/"}"#.to_vec(),
            br#"{"format":"rustsync-invite-v1","workspace_id":"../bad","server_url":"http://localhost/"}"#.to_vec(),
            br#"{"format":"rustsync-invite-v1","workspace_id":"workspace_test","server_url":"file:///tmp/server"}"#.to_vec(),
            br#"{"format":"rustsync-invite-v1","workspace_id":"workspace_test","server_url":"https://user:secret@example.com/"}"#.to_vec(),
            br#"{"format":"rustsync-invite-v1","workspace_id":"workspace_test"}"#.to_vec(),
        ] {
            fs::write(&file, bytes).unwrap();
            assert!(join(file.clone(), target.clone(), None, false, None).await.is_err());
            assert!(!target.exists());
        }
    }
}
