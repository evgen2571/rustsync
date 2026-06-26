use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rustsync_protocol::{
    ApiErrorResponse, RequestNonce, UnixTimestamp,
    auth::{
        DEVICE_ID_HEADER, NONCE_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER,
        canonical_request_payload, sha256_hex,
    },
};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::atomic::{AtomicU64, Ordering};
use url::Url;

use crate::{ClientError, ClientResult, RequestSigner};

static NEXT_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy)]
pub(crate) enum Method {
    Get,
    Post,
    Put,
}

impl Method {
    const fn as_reqwest(self) -> reqwest::Method {
        match self {
            Self::Get => reqwest::Method::GET,
            Self::Post => reqwest::Method::POST,
            Self::Put => reqwest::Method::PUT,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
        }
    }
}

pub(crate) async fn request_json_signed<T, S>(
    http: &reqwest::Client,
    base_url: &Url,
    method: Method,
    path: &str,
    body: Vec<u8>,
    signer: &S,
) -> ClientResult<T>
where
    T: DeserializeOwned,
    S: RequestSigner,
{
    let response = send_signed(http, base_url, method, path, body, signer, None).await?;
    decode_json_response(response).await
}

pub(crate) async fn request_json_body_signed<T, B, S>(
    http: &reqwest::Client,
    base_url: &Url,
    method: Method,
    path: &str,
    body: &B,
    signer: &S,
) -> ClientResult<T>
where
    T: DeserializeOwned,
    B: Serialize,
    S: RequestSigner,
{
    let body = serde_json::to_vec(body).map_err(|error| {
        ClientError::InvalidConfig(format!("failed to encode JSON body: {error}"))
    })?;
    let response = send_signed(
        http,
        base_url,
        method,
        path,
        body,
        signer,
        Some("application/json"),
    )
    .await?;
    decode_json_response(response).await
}

pub(crate) async fn request_bytes_signed<S>(
    http: &reqwest::Client,
    base_url: &Url,
    method: Method,
    path: &str,
    body: Vec<u8>,
    signer: &S,
) -> ClientResult<Vec<u8>>
where
    S: RequestSigner,
{
    let response = send_signed(http, base_url, method, path, body, signer, None).await?;
    decode_bytes_response(response).await
}

async fn send_signed<S>(
    http: &reqwest::Client,
    base_url: &Url,
    method: Method,
    path: &str,
    body: Vec<u8>,
    signer: &S,
    content_type: Option<&str>,
) -> ClientResult<reqwest::Response>
where
    S: RequestSigner,
{
    let url = base_url
        .join(path)
        .map_err(|error| ClientError::InvalidConfig(error.to_string()))?;
    let path_and_query = path_and_query(&url);
    let timestamp = UnixTimestamp::now();
    let nonce = generate_nonce(timestamp)?;
    let body_hash = sha256_hex(&body);
    let canonical_request = canonical_request_payload(
        method.as_str(),
        &path_and_query,
        &body_hash,
        timestamp,
        signer.device_id(),
        &nonce,
        body.len() as u64,
    );
    let signature = signer.sign(&canonical_request)?;

    let mut request = http
        .request(method.as_reqwest(), url)
        .header(DEVICE_ID_HEADER, signer.device_id().as_str())
        .header(TIMESTAMP_HEADER, timestamp.as_secs().to_string())
        .header(NONCE_HEADER, nonce.as_str())
        .header(SIGNATURE_HEADER, URL_SAFE_NO_PAD.encode(signature));

    if let Some(content_type) = content_type {
        request = request.header(reqwest::header::CONTENT_TYPE, content_type);
    }

    request
        .body(body)
        .send()
        .await
        .map_err(ClientError::from_reqwest)
}

async fn decode_json_response<T>(response: reqwest::Response) -> ClientResult<T>
where
    T: DeserializeOwned,
{
    let status = response.status();
    if !status.is_success() {
        return Err(decode_error_response(response, status).await);
    }

    response
        .json::<T>()
        .await
        .map_err(ClientError::from_reqwest)
}

async fn decode_bytes_response(response: reqwest::Response) -> ClientResult<Vec<u8>> {
    let status = response.status();
    if !status.is_success() {
        return Err(decode_error_response(response, status).await);
    }

    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(ClientError::from_reqwest)
}

async fn decode_error_response(
    response: reqwest::Response,
    status: reqwest::StatusCode,
) -> ClientError {
    response
        .json::<ApiErrorResponse>()
        .await
        .map(ClientError::Server)
        .unwrap_or_else(|error| {
            ClientError::InvalidResponse(format!(
                "failed to decode JSON error response for HTTP {status}: {error}"
            ))
        })
}

fn generate_nonce(timestamp: UnixTimestamp) -> ClientResult<RequestNonce> {
    let counter = NEXT_NONCE.fetch_add(1, Ordering::Relaxed);
    RequestNonce::parse(format!("nonce_{}_{}", timestamp.as_secs(), counter))
        .map_err(|error| ClientError::Signing(error.to_string()))
}

fn path_and_query(url: &Url) -> String {
    let mut value = url.path().to_owned();
    if let Some(query) = url.query() {
        value.push('?');
        value.push_str(query);
    }
    value
}
