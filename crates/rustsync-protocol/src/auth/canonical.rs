use sha2::{Digest, Sha256};

use crate::{DeviceId, UnixTimestamp, auth::RequestNonce};

pub const AUTH_DOMAIN: &str = "rustsync-http-auth";

pub fn canonical_request_payload(
    method: &str,
    path_and_query: &str,
    body_sha256_hex: &str,
    timestamp: UnixTimestamp,
    device_id: &DeviceId,
    nonce: &RequestNonce,
    content_length: u64,
) -> Vec<u8> {
    format!(
        "{AUTH_DOMAIN}\n{method}\n{path_and_query}\n{body_sha256_hex}\n{}\n{}\n{}\n{content_length}\n", 
        timestamp.as_secs(),
        device_id.as_str(),
        nonce.as_str()
    )
    .into_bytes()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);

    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
    }

    out
}
