use super::define_text_id;

pub const JOIN_REQUEST_ID_PREFIX: &str = "join_";

define_text_id!(
    JoinRequestId,
    "join request",
    Some(JOIN_REQUEST_ID_PREFIX),
    96
);
