mod canonical;
mod headers;
mod middleware;
mod replay;

pub use canonical::{AUTH_DOMAIN, canonical_request_payload, sha256_hex};
pub use headers::{DEVICE_ID_HEADER, REQUEST_ID_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER};
pub use middleware::{AuthenticatedDevice, workspace_auth_middleware};
pub use replay::ReplayCache;
