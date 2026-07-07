mod middleware;
mod replay;

pub use middleware::{AuthenticatedDevice, workspace_auth_middleware};
pub(crate) use middleware::{
    MAX_AUTH_BODY_BYTES, MAX_TIMESTAMP_SKEW_SECONDS, authenticate,
    signed_http_request_from_headers, validate_timestamp,
};
pub use replay::ReplayCache;
