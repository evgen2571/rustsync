use super::define_text_id;

pub const ACCESS_EVENT_ID_PREFIX: &str = "event_";

define_text_id!(
    AccessEventId,
    "access event",
    Some(ACCESS_EVENT_ID_PREFIX),
    96
);
