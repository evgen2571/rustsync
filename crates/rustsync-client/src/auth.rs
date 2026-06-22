use rustsync_protocol::DeviceId;

use crate::error::ClientResult;

pub trait RequestSigner: Send + Sync {
    fn device_id(&self) -> &DeviceId;

    fn sign(&self, canonical_request: &[u8]) -> ClientResult<Vec<u8>>;
}
