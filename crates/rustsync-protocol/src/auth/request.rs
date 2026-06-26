use crate::{
    DeviceId, DeviceRecord, ProtocolError, ProtocolResult, UnixTimestamp,
    auth::{AuthHeaders, RequestNonce, canonical_request_payload, sha256_hex},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedHttpRequestParts<'a> {
    pub method: &'a str,
    pub path_and_query: &'a str,
    pub body: &'a [u8],
}

impl<'a> SignedHttpRequestParts<'a> {
    pub fn new(method: &'a str, path_and_query: &'a str, body: &'a [u8]) -> Self {
        Self {
            method,
            path_and_query,
            body,
        }
    }

    pub fn signature_input(
        self,
        device_id: DeviceId,
        timestamp: UnixTimestamp,
        nonce: RequestNonce,
    ) -> HttpRequestSignatureInput {
        HttpRequestSignatureInput {
            method: self.method.to_string(),
            path_and_query: self.path_and_query.to_string(),
            body_sha256_hex: sha256_hex(self.body),
            timestamp,
            device_id,
            nonce,
            content_length: self.body.len() as u64,
        }
    }

    pub fn from_auth_headers(self, headers: AuthHeaders) -> SignedHttpRequest {
        self.signature_input(headers.device_id, headers.timestamp, headers.nonce)
            .with_signature(headers.signature)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequestSignatureInput {
    pub method: String,
    pub path_and_query: String,
    pub body_sha256_hex: String,
    pub timestamp: UnixTimestamp,
    pub device_id: DeviceId,
    pub nonce: RequestNonce,
    pub content_length: u64,
}

impl HttpRequestSignatureInput {
    pub fn canonical_payload(&self) -> Vec<u8> {
        canonical_request_payload(
            &self.method,
            &self.path_and_query,
            &self.body_sha256_hex,
            self.timestamp,
            &self.device_id,
            &self.nonce,
            self.content_length,
        )
    }

    pub fn with_signature(self, signature: Vec<u8>) -> SignedHttpRequest {
        SignedHttpRequest {
            input: self,
            signature,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedHttpRequest {
    pub input: HttpRequestSignatureInput,
    pub signature: Vec<u8>,
}

impl SignedHttpRequest {
    pub fn canonical_payload(&self) -> Vec<u8> {
        self.input.canonical_payload()
    }

    pub fn verify_with_device(&self, device: &DeviceRecord) -> ProtocolResult<()> {
        if self.input.device_id != device.device_id {
            return Err(ProtocolError::InvalidRequestSignature);
        }

        device
            .verify_signature(&self.input.canonical_payload(), &self.signature)
            .map_err(|_| ProtocolError::InvalidRequestSignature)
    }

    pub fn auth_headers(&self) -> AuthHeaders {
        AuthHeaders {
            device_id: self.input.device_id.clone(),
            timestamp: self.input.timestamp,
            nonce: self.input.nonce.clone(),
            signature: self.signature.clone(),
        }
    }
}
