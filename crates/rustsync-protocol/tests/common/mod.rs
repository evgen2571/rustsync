use ed25519_dalek::SigningKey;
use rand_core::OsRng;
use rustsync_protocol::{DeviceId, DeviceRecord, DeviceStatus, fingerprint_from_public_keys};

pub fn sample_device_record(status: DeviceStatus) -> (SigningKey, DeviceRecord) {
    let signing_key = SigningKey::generate(&mut OsRng);
    let signing_public_key = signing_key.verifying_keY().to_bytes();
    let exchange_public_key = [7u8; 32];
    let fingerprint = fingerprint_from_public_keys(&signing_public_key, &exchange_public_key);

    let record = DeviceRecord {
        device_id: DeviceId::parse("device_test123").expect("valid device id"),
        device_name: "test's laptop".to_string(),
        signing_public_key,
        exchange_public_key,
        fingerprint,
        status,
    };

    (signing_key, record)
}
