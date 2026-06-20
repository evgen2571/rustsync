use axum::http::{HeaderMap, HeaderValue};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rustsync_protocol::DeviceId;

use crate::error::{ServerError, ServerResult};

pub const DEVICE_ID_HEADER: &str = "x-rustsync-device-id";
pub const TIMESTAMP_HEADER: &str = "x-rustsync-timestamp";
pub const REQUEST_ID_HEADER: &str = "x-rustsync-request-id";
pub const SIGNATURE_HEADER: &str = "x-rustsync-signature";

#[derive(Debug, Clone)]
pub struct AuthHeaders {
    pub device_id: DeviceId,
    pub timestamp_unix_seconds: u64,
    pub request_id: String,
    pub signature: Vec<u8>,
}

impl AuthHeaders {
    pub fn parse(headers: &HeaderMap) -> ServerResult<Self> {
        let device_id = parse_device_id(required_header(headers, DEVICE_ID_HEADER)?)?;
        let timestamp_unix_seconds = parse_timestamp(required_header(headers, TIMESTAMP_HEADER)?)?;
        let request_id = parse_request_id(required_header(headers, REQUEST_ID_HEADER)?)?;
        let signature = parse_signature(required_header(headers, SIGNATURE_HEADER)?)?;

        Ok(Self {
            device_id,
            timestamp_unix_seconds,
            request_id,
            signature,
        })
    }
}

fn required_header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
) -> ServerResult<&'a HeaderValue> {
    headers.get(name).ok_or(ServerError::AuthenticationRequired)
}

fn parse_device_id(value: &HeaderValue) -> ServerResult<DeviceId> {
    let value = value.to_str().map_err(|_| ServerError::InvalidAuthHeader)?;
    DeviceId::parse(value).map_err(|_| ServerError::InvalidDeviceId)
}

fn parse_timestamp(value: &HeaderValue) -> ServerResult<u64> {
    let value = value.to_str().map_err(|_| ServerError::InvalidAuthHeader)?;
    value
        .parse::<u64>()
        .map_err(|_| ServerError::InvalidAuthHeader)
}

fn parse_request_id(value: &HeaderValue) -> ServerResult<String> {
    let value = value.to_str().map_err(|_| ServerError::InvalidAuthHeader)?;
    if value.is_empty() {
        return Err(ServerError::InvalidAuthHeader);
    }
    Ok(value.to_owned())
}

fn parse_signature(value: &HeaderValue) -> ServerResult<Vec<u8>> {
    let value = value.to_str().map_err(|_| ServerError::InvalidAuthHeader)?;
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| ServerError::InvalidRequestSignature)
}
