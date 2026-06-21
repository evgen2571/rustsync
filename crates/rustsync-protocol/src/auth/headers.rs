use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use crate::{DeviceId, ProtocolError, ProtocolResult, UnixTimestamp, auth::RequestNonce};

pub const DEVICE_ID_HEADER: &str = "x-rustsync-device-id";
pub const TIMESTAMP_HEADER: &str = "x-rustsync-timestamp";
pub const NONCE_HEADER: &str = "x-rustsync-nonce";
pub const SIGNATURE_HEADER: &str = "x-rustsync-signature";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthHeaders {
    pub device_id: DeviceId,
    pub timestamp: UnixTimestamp,
    pub nonce: RequestNonce,
    pub signature: Vec<u8>,
}

impl AuthHeaders {
    pub fn from_header_values(
        device_id: &str,
        timestamp: &str,
        nonce: &str,
        signature: &str,
    ) -> ProtocolResult<Self> {
        let device_id = DeviceId::parse(device_id)?;
        let timestamp = timestamp.parse::<u64>().map_err(|_| {
            ProtocolError::InvalidAuthHeader("timestamp must be an unsigned integer".to_string())
        })?;
        let timestamp = UnixTimestamp::from_secs(timestamp);
        let nonce = RequestNonce::parse(nonce)?;
        let signature = parse_signature(signature)?;

        Ok(Self {
            device_id,
            timestamp,
            nonce,
            signature,
        })
    }
}

fn parse_signature(value: &str) -> ProtocolResult<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| ProtocolError::InvalidRequestSignature)
}
