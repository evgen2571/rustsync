use rustsync_protocol::{DeviceId, WorkspaceId};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::error::{ServerError, ServerResult};

#[derive(Debug, Clone, Default)]
pub struct ReplayCache {
    seen: Arc<Mutex<HashMap<ReplayKey, u64>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReplayKey {
    workspace_id: WorkspaceId,
    device_id: DeviceId,
    request_id: String,
}

impl ReplayCache {
    pub fn check_and_record(
        &self,
        workspace_id: &WorkspaceId,
        device_id: &DeviceId,
        request_id: &str,
        timestamp_unix_seconds: u64,
        max_age_seconds: u64,
    ) -> ServerResult<()> {
        let min_timestamp = timestamp_unix_seconds.saturating_sub(max_age_seconds);
        let mut seen = self.seen.lock().expect("replay cache mutex poisoned");
        seen.retain(|_, timestamp| *timestamp >= min_timestamp);

        let key = ReplayKey {
            workspace_id: workspace_id.clone(),
            device_id: device_id.clone(),
            request_id: request_id.to_owned(),
        };

        if seen.contains_key(&key) {
            return Err(ServerError::ReplayDetected);
        }

        seen.insert(key, timestamp_unix_seconds);
        Ok(())
    }
}
