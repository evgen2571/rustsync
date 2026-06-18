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
