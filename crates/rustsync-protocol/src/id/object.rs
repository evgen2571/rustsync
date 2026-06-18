use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, str::FromStr};

use crate::{ProtocolError, ProtocolResult};

const SHA256_HEX_LENGTH: usize = 64;

macro_rules! define_object_id {
    ($name:ident, $kind:literal, $prefix:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> ProtocolResult<Self> {
                let value = value.into();
                let expected_len = $prefix.len() + SHA256_HEX_LENGTH;

                if value.len() != expected_len || !value.starts_with($prefix) {
                    return Err(ProtocolError::InvalidIdentifier {
                        kind: $kind.to_string(),
                        value,
                        reason: format!(
                            "expected `{}` followed by 64 lowercase hexadecimal characters",
                            $prefix
                        ),
                    });
                }

                let hash = &value[$prefix.len()..];

                if !hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
                {
                    return Err(ProtocolError::InvalidIdentifier {
                        kind: $kind.to_string(),
                        value,
                        reason: "object hash must be lowercase hexadecimal".to_string(),
                    });
                }

                Ok(Self(value))
            }

            pub fn from_content(bytes: &[u8]) -> Self {
                let digest = Sha256::digest(bytes);
                let mut value = String::with_capacity($prefix.len() + SHA256_HEX_LENGTH);
                value.push_str($prefix);

                for byte in digest {
                    use std::fmt::Write as _;
                    write!(&mut value, "{byte:02x}").expect("writing to String cannot fail");
                }

                Self(value)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = ProtocolError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = ProtocolError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

pub const BLOB_ID_PREFIX: &str = "blob_";
pub const MANIFEST_ID_PREFIX: &str = "manifest_";

define_object_id!(BlobId, "blob", BLOB_ID_PREFIX);
define_object_id!(ManifestId, "manifest", MANIFEST_ID_PREFIX);
