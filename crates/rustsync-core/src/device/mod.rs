mod challenge;
mod fingerprint;
mod identity;
mod registry;
mod store;

pub use challenge::DeviceJoinRequest;
pub use fingerprint::{fingerprint_from_public_keys, short_fingerprint};
pub use identity::{
    DEVICE_ID_PREFIX, DeviceIdentity, DeviceRecord, DeviceStatus, default_device_name,
};
pub use registry::DeviceRegistry;
pub use store::{
    DEVICE_IDENTITY_FILE_NAME, DEVICE_REGISTRY_FILE_NAME, load_device_registry,
    load_local_device_identity, save_device_registry, save_local_device_identity,
};

pub(crate) use crate::error::{DeviceError, DeviceResult};
