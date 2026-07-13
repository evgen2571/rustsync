use std::{
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::{
    fs,
    io::{AsyncWriteExt, ErrorKind},
};

use crate::error::{ServerError, ServerResult};

#[cfg(test)]
type BeforePublishHook = Box<dyn FnOnce(&Path) + Send>;

#[cfg(test)]
static BEFORE_PUBLISH_HOOK: std::sync::Mutex<Option<BeforePublishHook>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
pub(crate) fn set_before_publish_hook(hook: impl FnOnce(&Path) + Send + 'static) {
    *BEFORE_PUBLISH_HOOK
        .lock()
        .expect("atomic write test hook mutex must not be poisoned") = Some(Box::new(hook));
}

#[cfg(test)]
fn run_before_publish_hook(path: &Path) {
    if let Some(hook) = BEFORE_PUBLISH_HOOK
        .lock()
        .expect("atomic write test hook mutex must not be poisoned")
        .take()
    {
        hook(path);
    }
}

pub(crate) async fn write_new(path: &Path, bytes: &[u8]) -> ServerResult<bool> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let temp_path = temp_path_for(path);

    let mut temp_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .await?;

    temp_file.write_all(bytes).await?;
    temp_file.sync_all().await?;
    drop(temp_file);

    #[cfg(test)]
    run_before_publish_hook(path);

    match fs::hard_link(&temp_path, path).await {
        Ok(()) => {
            remove_temp_file(&temp_path).await?;
            Ok(true)
        }
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
            remove_temp_file(&temp_path).await?;
            Ok(false)
        }
        Err(err) => {
            let _ = remove_temp_file(&temp_path).await;
            Err(ServerError::Storage(err))
        }
    }
}

pub(crate) async fn write_replace(path: &Path, bytes: &[u8]) -> ServerResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let temp_path = temp_path_for(path);

    let mut temp_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .await?;

    temp_file.write_all(bytes).await?;
    temp_file.sync_all().await?;
    drop(temp_file);

    if let Err(err) = fs::rename(&temp_path, path).await {
        let _ = remove_temp_file(&temp_path).await;
        return Err(ServerError::Storage(err));
    }

    Ok(())
}

async fn remove_temp_file(path: &Path) -> ServerResult<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(ServerError::Storage(err)),
    }
}

fn temp_path_for(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("object");

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    parent.join(format!(".{file_name}.{}.{}.tmp", process::id(), timestamp))
}
