use rand_core::{OsRng, RngCore};
use rustsync_protocol::{
    ApiErrorResponse, RequestNonce, UnixTimestamp,
    auth::{
        DEVICE_ID_HEADER, NONCE_HEADER, SIGNATURE_HEADER, SignedHttpRequestParts, TIMESTAMP_HEADER,
    },
};
use serde::{Serialize, de::DeserializeOwned};
use url::Url;

use crate::{ClientError, ClientResult, RequestSigner};

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
    maximum_response_bytes: usize,
) -> ClientResult<Vec<u8>>
where
    S: RequestSigner,
{
    let response = send_signed(http, base_url, method, path, body, signer, None).await?;
    decode_bytes_response(response, maximum_response_bytes).await
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
    let nonce = generate_nonce()?;
    let input = SignedHttpRequestParts::new(method.as_str(), &path_and_query, &body)
        .signature_input(signer.device_id().clone(), timestamp, nonce);
    let signature = signer.sign(&input.canonical_payload())?;
    let auth_header_values = input
        .with_signature(signature)
        .auth_headers()
        .to_header_values();

    let mut request = http
        .request(method.as_reqwest(), url)
        .header(DEVICE_ID_HEADER, auth_header_values.device_id)
        .header(TIMESTAMP_HEADER, auth_header_values.timestamp)
        .header(NONCE_HEADER, auth_header_values.nonce)
        .header(SIGNATURE_HEADER, auth_header_values.signature);

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

async fn decode_bytes_response(
    mut response: reqwest::Response,
    maximum_response_bytes: usize,
) -> ClientResult<Vec<u8>> {
    let status = response.status();
    if !status.is_success() {
        return Err(decode_error_response(response, status).await);
    }

    if let Some(content_length) = response.content_length()
        && content_length > maximum_response_bytes as u64
    {
        return Err(ClientError::InvalidResponse(format!(
            "response body exceeds the {maximum_response_bytes}-byte limit"
        )));
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(ClientError::from_reqwest)? {
        let remaining = maximum_response_bytes.saturating_sub(bytes.len());
        if chunk.len() > remaining {
            return Err(ClientError::InvalidResponse(format!(
                "response body exceeds the {maximum_response_bytes}-byte limit"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(bytes)
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

fn generate_nonce() -> ClientResult<RequestNonce> {
    let mut bytes = [0u8; 16];
    OsRng.try_fill_bytes(&mut bytes).map_err(|error| {
        ClientError::Signing(format!("failed to generate request nonce: {error}"))
    })?;
    RequestNonce::parse(format!("nonce_{:032x}", u128::from_le_bytes(bytes)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, process::Command};

    #[test]
    fn nonce_child_process() {
        if std::env::var_os("RUSTSYNC_TEST_NONCE_CHILD").is_none() {
            return;
        }
        for _ in 0..128 {
            println!("TEST_NONCE={}", generate_nonce().unwrap());
        }
    }

    #[test]
    fn request_nonces_are_unique_across_processes() {
        let children: Vec<_> = (0..8)
            .map(|_| {
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "transport::tests::nonce_child_process",
                        "--nocapture",
                    ])
                    .env("RUSTSYNC_TEST_NONCE_CHILD", "1")
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        let outputs: Vec<_> = children
            .into_iter()
            .map(|child| child.wait_with_output().unwrap())
            .collect();
        let mut nonces = BTreeSet::new();
        for output in outputs {
            assert!(output.status.success());
            let stdout = String::from_utf8(output.stdout).unwrap();
            let values: Vec<_> = stdout
                .lines()
                .filter_map(|line| line.strip_prefix("TEST_NONCE="))
                .collect();
            assert_eq!(values.len(), 128);
            for value in values {
                let nonce = RequestNonce::parse(value).unwrap();
                assert!(
                    nonces.insert(nonce),
                    "nonce reused across processes: {value}"
                );
            }
        }
        assert_eq!(nonces.len(), 1024);
    }
}
