use rustsync_client::{ClientError, ClientResult, RequestSigner};
use rustsync_core::device::DeviceIdentity;
use rustsync_protocol::DeviceId;

pub(crate) struct LocalDeviceRequestSigner {
    identity: DeviceIdentity,
}

impl LocalDeviceRequestSigner {
    pub(crate) fn new(identity: DeviceIdentity) -> Self {
        Self { identity }
    }
}

impl RequestSigner for LocalDeviceRequestSigner {
    fn device_id(&self) -> &DeviceId {
        self.identity.device_id()
    }

    fn sign(&self, canonical_request: &[u8]) -> ClientResult<Vec<u8>> {
        self.identity
            .sign(canonical_request)
            .map_err(|error| ClientError::Signing(error.to_string()))
    }
}
