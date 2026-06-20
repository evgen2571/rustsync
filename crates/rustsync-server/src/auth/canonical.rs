use axum::http::{Method, Uri};
use sha2::{Digest, Sha256};

pub const AUTH_DOMAIN: &str = "rustsync-http-auth";

pub fn canonical_request_payload(
    method: &Method,
    uri: &Uri,
    body_sha256_hex: &str,
    timestamp_unix_seconds: u64,
    device_id: &str,
    request_id: &str,
    content_length: u64,
) -> Vec<u8> {
    format!(
        "{AUTH_DOMAIN}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        method.as_str(),
        uri.path_and_query()
            .map_or_else(|| uri.path(), |path_and_query| path_and_query.as_str()),
        body_sha256_hex,
        timestamp_unix_seconds,
        device_id,
        request_id,
        content_length,
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
