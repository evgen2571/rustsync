use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use crate::{DeviceId, ProtocolError, ProtocolResult};

pub const DEVICE_ID_HEADER: &str = "x-rustsync-device-id";
pub const TIMESTAMP_HEADER: &str = "x-rustsync-timestamp";
pub const REQUEST_ID_HEADER: &str = "x-rustsync-request-id";
pub const SIGNATURE_HEADER: &str = "x-rustsync-signature";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthHeaders {
    pub device_id: DeviceId,
    pub timestamp_unix_seconds: u64,
    pub request_id: String,
    pub signature: Vec<u8>,
}

impl AuthHeaders {
    pub fn from_header_values(
        device_id: &str,
        timestamp_unix_seconds: &str,
        request_id: &str,
        signature: &str,
    ) -> ProtocolResult<Self> {
        let device_id = DeviceId::parse(device_id)?;
        let timestamp_unix_seconds = timestamp_unix_seconds.parse::<u64>().map_err(|_| {
            ProtocolError::InvalidAuthHeader("timestamp must be an unsigned integer".to_string())
        })?;
        let request_id = parse_request_id(request_id)?;
        let signature = parse_signature(signature)?;

        Ok(Self {
            device_id,
            timestamp_unix_seconds,
            request_id,
            signature,
        })
    }
}

fn parse_request_id(value: &str) -> ProtocolResult<String> {
    if value.is_empty() {
        return Err(ProtocolError::InvalidAuthHeader(
            "request id must not be empty".to_string(),
        ));
    }
    Ok(value.to_owned())
}

fn parse_signature(value: &str) -> ProtocolResult<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| ProtocolError::InvalidRequestSignature)
}
