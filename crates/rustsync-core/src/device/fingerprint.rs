use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

pub fn fingerprint_from_public_keys(
    signing_public_key: &[u8; 32],
    exchange_public_key: &[u8; 32],
) -> String {
    let mut hasher = Sha256::new();

    hasher.update(signing_public_key);
    hasher.update(exchange_public_key);

    let hash = hasher.finalize();

    short_fingerprint(&hash)
}

pub fn short_fingerprint(bytes: &[u8]) -> String {
    let encoded = URL_SAFE_NO_PAD.encode(&bytes[..10]).to_uppercase();

    encoded
        .as_bytes()
        .chunks(4)
        .map(|chunk| std::str::from_utf8(chunk).expect("base64 is valid utf8"))
        .collect::<Vec<_>>()
        .join("-")
}
