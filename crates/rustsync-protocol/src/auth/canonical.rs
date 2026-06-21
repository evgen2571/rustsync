use sha2::{Digest, Sha256};

pub const AUTH_DOMAIN: &str = "rustsync-http-auth";

pub fn canonical_request_payload(
    method: &str,
    path_and_query: &str,
    body_sha256_hex: &str,
    timestamp_unix_seconds: u64,
    device_id: &str,
    request_id: &str,
    content_length: u64,
) -> Vec<u8> {
    format!("{AUTH_DOMAIN}\n{method}\n{path_and_query}\n{body_sha256_hex}\n{timestamp_unix_seconds}\n{device_id}\n{request_id}\n{content_length}\n",).into_bytes()
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
