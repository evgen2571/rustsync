use serde::{Deserialize, Serialize};

use crate::{ProtocolError, ProtocolResult};

const MAX_REQUEST_NONCE_LEN: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RequestNonce(String);

impl RequestNonce {
    pub fn parse(value: impl Into<String>) -> ProtocolResult<Self> {
        let value = value.into();

        if value.is_empty() {
            return Err(ProtocolError::InvalidAuthHeader(
                "request nonce must not be empty".to_string(),
            ));
        }

        if value.len() > MAX_REQUEST_NONCE_LEN {
            return Err(ProtocolError::InvalidAuthHeader(format!(
                "request nonce must not excee {MAX_REQUEST_NONCE_LEN} bytes"
            )));
        }

        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(ProtocolError::InvalidAuthHeader(
                "request nonce may contain only ascii letters, digits, `_`, `-`, and `.`"
                    .to_string(),
            ));
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Display for RequestNonce {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::str::FromStr for RequestNonce {
    type Err = ProtocolError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<String> for RequestNonce {
    type Error = ProtocolError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl AsRef<str> for RequestNonce {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
