//! SigV4-signed outbound requests to AWS REST APIs (SageMaker runtime, etc.)
//! straight from the WASM component — no native SDK required. Prefer the
//! data gateway for S3/SageMaker control-plane calls; use this for
//! lightweight data-plane calls (e.g. `invoke-endpoint`).

use base64::Engine;
use chrono::Utc;
use octy_shared::errors::OctyError;
use octy_shared::sigv4::{authorization_header, sha256_hex, SigningParams};
use spin_sdk::http::Method;

use crate::gateway::http_send;

pub struct AwsCreds {
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
}

impl AwsCreds {
    /// Reads `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` from a secrets
    /// blob plus `AWS_REGION` from config (Python `Secrets[...]`/`Config[...]`).
    pub fn from_ctx(ctx: &crate::ctx::Ctx) -> Result<Self, OctyError> {
        Ok(Self {
            access_key: ctx.secrets.get_str("AWS_ACCESS_KEY_ID")?.to_string(),
            secret_key: ctx.secrets.get_str("AWS_SECRET_ACCESS_KEY")?.to_string(),
            region: ctx.config.get_str("AWS_REGION")?.to_string(),
        })
    }
}

/// Send a signed request. `service` is the SigV4 service name (`"sagemaker"`,
/// `"runtime.sagemaker"`, `"s3"`, …); extra headers are included in signing.
pub async fn send_signed(
    creds: &AwsCreds,
    service: &str,
    method: Method,
    url: &str,
    extra_headers: &[(&str, &str)],
    body: Vec<u8>,
) -> Result<(u16, Vec<u8>), OctyError> {
    let parsed = url::Url::parse(url).map_err(|e| OctyError::internal(format!("bad AWS url: {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| OctyError::internal("AWS url missing host"))?
        .to_string();
    let path = parsed.path().to_string();
    let query: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let amz_date = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let payload_hash = sha256_hex(&body);

    let mut signed_headers: Vec<(String, String)> = vec![
        ("host".to_string(), host.clone()),
        ("x-amz-date".to_string(), amz_date.clone()),
        ("x-amz-content-sha256".to_string(), payload_hash.clone()),
    ];
    for (name, value) in extra_headers {
        signed_headers.push((name.to_lowercase(), value.to_string()));
    }

    let authorization = authorization_header(
        &SigningParams {
            access_key: &creds.access_key,
            secret_key: &creds.secret_key,
            region: &creds.region,
            service,
            amz_date: &amz_date,
        },
        method_name(&method),
        &path,
        &query,
        &signed_headers,
        &payload_hash,
    );

    let mut headers: Vec<(String, String)> = signed_headers
        .iter()
        .filter(|(name, _)| name != "host") // set by the HTTP stack
        .cloned()
        .collect();
    headers.push(("authorization".to_string(), authorization));

    let header_refs: Vec<(&str, &str)> = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    http_send(method, url, &header_refs, Some(body)).await
}

fn method_name(method: &Method) -> &'static str {
    match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
        Method::Patch => "PATCH",
        Method::Head => "HEAD",
        Method::Options => "OPTIONS",
        _ => "GET",
    }
}

/// Helper for APIs that want base64 bodies embedded in JSON.
pub fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}
