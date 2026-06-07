mod device;
mod join_request;
mod key;
mod workspace;

pub use device::{DEVICE_ID_PREFIX, DeviceId};
pub use join_request::{JOIN_REQUEST_ID_PREFIX, JoinRequestId};
pub use key::{KeyId, SYSTEM_KEY_ID};
pub use workspace::WorkspaceId;

use crate::{ProtocolError, ProtocolResult};

pub(crate) fn validate_text_id(
    kind: &str,
    value: &str,
    required_prefix: Option<&str>,
    max_len: usize,
) -> ProtocolResult<()> {
    if value.is_empty() {
        return Err(invalid_id(kind, value, "identifier is empty"));
    }

    if value.len() > max_len {
        return Err(invalid_id(
            kind,
            value,
            format!("identifier exceeds {max_len} bytes"),
        ));
    }

    if let Some(prefix) = required_prefix
        && (!value.starts_with(prefix) || value.len() == prefix.len())
    {
        return Err(invalid_id(
            kind,
            value,
            format!("identifier must start with `{prefix}` and contain a suffix"),
        ));
    }

    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(invalid_id(
            kind,
            value,
            "only ascii letters, digits, `_`, `-`, and `.` are allowed",
        ));
    }

    Ok(())
}

pub(crate) fn invalid_id(
    kind: &str,
    value: impl Into<String>,
    reason: impl Into<String>,
) -> ProtocolError {
    ProtocolError::InvalidIdentifier {
        kind: kind.to_string(),
        value: value.into(),
        reason: reason.into(),
    }
}

macro_rules! define_text_id {
    ($name:ident, $kind:literal, $prefix:expr, $max_len:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> crate::ProtocolResult<Self> {
                let value = value.into();
                super::validate_text_id($kind, &value, $prefix, $max_len)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = crate::ProtocolError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = crate::ProtocolError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = crate::ProtocolError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = <String as serde::Deserialize>::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

pub(crate) use define_text_id;
