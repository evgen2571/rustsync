use rustsync_protocol::{DeviceId, RequestNonce, UnixTimestamp, WorkspaceId};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::error::{ServerError, ServerResult};

#[derive(Debug, Clone, Default)]
pub struct ReplayCache {
    seen: Arc<Mutex<HashMap<ReplayKey, UnixTimestamp>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReplayKey {
    workspace_id: WorkspaceId,
    device_id: DeviceId,
    nonce: RequestNonce,
}

impl ReplayCache {
    pub fn check_and_record(
        &self,
        workspace_id: &WorkspaceId,
        device_id: &DeviceId,
        nonce: &RequestNonce,
        timestamp: UnixTimestamp,
        max_age_seconds: u64,
    ) -> ServerResult<()> {
        let min_timestamp = timestamp.as_secs().saturating_sub(max_age_seconds);
        let mut seen = self.seen.lock().expect("replay cache mutex poisoned");
        seen.retain(|_, timestamp| timestamp.as_secs() >= min_timestamp);

        let key = ReplayKey {
            workspace_id: workspace_id.clone(),
            device_id: device_id.clone(),
            nonce: nonce.clone(),
        };

        if seen.contains_key(&key) {
            return Err(ServerError::ReplayDetected);
        }

        seen.insert(key, timestamp);
        Ok(())
    }
}
