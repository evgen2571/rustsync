use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{ProtocolError, ProtocolResult};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnixTimestamp(u64);

impl UnixTimestamp {
    pub fn now() -> Self {
        Self::from_system_time(SystemTime::now()).expect("system time should be after unix epoch")
    }

    pub const fn from_secs(seconds: u64) -> Self {
        Self(seconds)
    }

    pub fn from_system_time(time: SystemTime) -> ProtocolResult<Self> {
        let seconds = time
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ProtocolError::TimeBeforeUnixEpoch)?
            .as_secs();

        Ok(Self(seconds))
    }

    pub const fn as_secs(self) -> u64 {
        self.0
    }
}

impl From<UnixTimestamp> for u64 {
    fn from(timestamp: UnixTimestamp) -> Self {
        timestamp.as_secs()
    }
}
