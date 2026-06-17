use std::time::{Duration, UNIX_EPOCH};

use rustsync_protocol::UnixTimestamp;

#[test]
fn unix_timestamp_wraps_epoch_seconds() {
    let timestamp = UnixTimestamp::from_secs(42);

    assert_eq!(timestamp.as_secs(), 42);
    assert_eq!(u64::from(timestamp), 42);
}

#[test]
fn unix_timestamp_converts_system_time() {
    let time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);

    let timestamp = UnixTimestamp::from_system_time(time).expect("valid unix timestamp");

    assert_eq!(timestamp.as_secs(), 1_700_000_000);
}

#[test]
fn unix_timestamp_serializes_as_number() {
    let timestamp = UnixTimestamp::from_secs(123);

    let json = serde_json::to_string(&timestamp).expect("serialize timestamp");
    let decoded: UnixTimestamp = serde_json::from_str(&json).expect("deserialize timestamp");

    assert_eq!(json, "123");
    assert_eq!(decoded, timestamp);
}
