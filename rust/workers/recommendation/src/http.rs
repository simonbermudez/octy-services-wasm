//! Outbound-HTTP helpers: the `requests_retry_session` port (retry on
//! 500/502/504, 4 retries — immediate, WASI has no timer host call here) and
//! a JSON POST helper for the data-gateway S3 endpoints not wrapped by
//! `octy_spin::gateway::GatewayClient`.

use anyhow::{anyhow, Result};
use serde_json::Value;
use spin_sdk::http::Method;

use octy_spin::gateway::http_send;

const RETRY_STATUSES: [u16; 3] = [500, 502, 504];
const RETRIES: usize = 4;

pub async fn request_with_retry(
    method: Method,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<Vec<u8>>,
) -> Result<(u16, Vec<u8>)> {
    let mut last_err = anyhow!("request to {url} failed");
    for _attempt in 0..=RETRIES {
        match http_send(method.clone(), url, headers, body.clone()).await {
            Ok((status, response_body)) if !RETRY_STATUSES.contains(&status) => {
                return Ok((status, response_body));
            }
            Ok((status, _)) => last_err = anyhow!("{url} returned status {status}"),
            Err(e) => last_err = anyhow!("{url}: {e}"),
        }
    }
    Err(last_err)
}

pub async fn post_json_with_retry(
    url: &str,
    extra_headers: &[(&str, &str)],
    payload: &Value,
) -> Result<(u16, Vec<u8>)> {
    let body = serde_json::to_vec(payload)?;
    let mut headers = vec![("content-type", "application/json")];
    headers.extend_from_slice(extra_headers);
    request_with_retry(Method::Post, url, &headers, Some(body)).await
}

pub async fn get_with_retry(url: &str, extra_headers: &[(&str, &str)]) -> Result<(u16, Vec<u8>)> {
    request_with_retry(Method::Get, url, extra_headers, None).await
}

/// The data-gateway base URL (same resolution as `octy_spin::ctx::Ctx`).
pub fn gateway_base() -> String {
    octy_spin::ctx::variable("gateway_url", "GATEWAY_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// POST a JSON body to a gateway endpoint and parse the JSON response.
/// (GatewayClient keeps its generic `post` private; the S3 object endpoints
/// are wrapped in this crate instead.)
pub async fn gateway_post(path: &str, payload: &Value) -> Result<Value> {
    let url = format!("{}{}", gateway_base(), path);
    let (status, body) = http_send(
        Method::Post,
        &url,
        &[("content-type", "application/json")],
        Some(serde_json::to_vec(payload)?),
    )
    .await
    .map_err(|e| anyhow!("{e}"))?;
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        let detail = parsed
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("gateway error");
        return Err(anyhow!("gateway {path}: {status} {detail}"));
    }
    Ok(parsed)
}
