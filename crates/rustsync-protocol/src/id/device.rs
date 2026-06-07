use super::define_text_id;

pub const DEVICE_ID_PREFIX: &str = "device_";

define_text_id!(DeviceId, "device", Some(DEVICE_ID_PREFIX), 96);
