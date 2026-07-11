//! Port of `data/repositories/implementation/reward_cards_repository.py` —
//! the Rybbon rewards API, called directly from the component over HTTPS.

use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::http::Method;

use octy_spin::ctx::Ctx;
use octy_spin::gateway::http_send;

/// POST with `requests_retry_session` semantics (4 retries on 500/502/504).
async fn http_post_with_retry(
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<(u16, Vec<u8>), OctyError> {
    let mut last_err = OctyError::internal(format!("request to {url} failed"));
    for _attempt in 0..=4 {
        match http_send(Method::Post, url, headers, Some(body.to_vec())).await {
            Ok((status, response)) if !matches!(status, 500 | 502 | 504) => {
                return Ok((status, response))
            }
            Ok((status, _)) => last_err = OctyError::internal(format!("{url} returned status {status}")),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

async fn http_get_with_retry(
    url: &str,
    headers: &[(&str, &str)],
) -> Result<(u16, Vec<u8>), OctyError> {
    let mut last_err = OctyError::internal(format!("request to {url} failed"));
    for _attempt in 0..=4 {
        match http_send(Method::Get, url, headers, None).await {
            Ok((status, response)) if !matches!(status, 500 | 502 | 504) => {
                return Ok((status, response))
            }
            Ok((status, _)) => last_err = OctyError::internal(format!("{url} returned status {status}")),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// `auth` — OAuth client-credentials grant. Like the Python, only
/// `grant_type` and `client_id` are form-posted (no client secret).
pub async fn auth(ctx: &Ctx) -> Result<String, OctyError> {
    let url = ctx.config.get_str("RYBBON_AUTH_URL")?.to_string();
    let client_id = ctx.config.get_str("RYBBON_CLIENT_ID")?;
    let partner_id = ctx.config.get_str("RYBBON_PARTNER_ID")?.to_string();

    let body: String = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "client_credentials")
        .append_pair("client_id", client_id)
        .finish();

    let (status, response) = http_post_with_retry(
        &url,
        &[
            ("Partner-Id", partner_id.as_str()),
            ("Content-Type", "application/x-www-form-urlencoded"),
        ],
        body.as_bytes(),
    )
    .await?;
    eprintln!("[octy-messaging] POST Request: \"{url}\" returned response with valid status code: {status}");

    let parsed: Value = serde_json::from_slice(&response)
        .map_err(|e| OctyError::internal(format!("rybbon auth response not JSON: {e}")))?;
    parsed
        .get("access_token")
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| OctyError::internal("rybbon auth response missing 'access_token'"))
}

/// `get_campaigns` — paginated fetch of open campaigns.
///
/// The malformed query string (`?limit=…?start=…`) is preserved verbatim from
/// the Python. The Python loop condition (`200 > status_code < 500`) could
/// never terminate on 2xx responses; here pagination stops on a non-2xx
/// response or a short page (documented divergence — the intended behaviour).
pub async fn get_campaigns(ctx: &Ctx, auth_token: &str) -> Result<Vec<Value>, OctyError> {
    let base = ctx.config.get_str("RYBBON_CAMPAIGNS_URL")?.to_string();
    let partner_id = ctx.config.get_str("RYBBON_PARTNER_ID")?.to_string();
    let bearer = format!("Bearer {auth_token}");

    let mut campaigns: Vec<Value> = Vec::new();
    let mut cursor: i64 = 0;
    loop {
        let url = format!("{base}?limit=1000&filterByStatus=open?start={cursor}");
        let (status, response) = http_get_with_retry(
            &url,
            &[
                ("Partner-Id", partner_id.as_str()),
                ("Authorization", bearer.as_str()),
                ("Content-Type", "application/json"),
            ],
        )
        .await?;
        eprintln!("[octy-messaging] GET Request: \"{base}\" returned response with valid status code: {status}");
        if !(200..300).contains(&status) {
            break;
        }
        let parsed: Value = serde_json::from_slice(&response)
            .map_err(|e| OctyError::internal(format!("rybbon campaigns response not JSON: {e}")))?;
        let page = parsed["result"]["campaign"]
            .as_array()
            .cloned()
            .ok_or_else(|| OctyError::internal("rybbon campaigns response missing 'result.campaign'"))?;
        let n = page.len();
        campaigns.extend(page);
        if n < 1000 {
            break;
        }
        cursor += 1000;
    }
    Ok(campaigns)
}

/// `claim_rewards` — claims rewards per campaign group in chunks of 100.
pub async fn claim_rewards(
    ctx: &Ctx,
    auth_token: &str,
    claim_groups: &[Vec<Value>],
) -> Result<Vec<Value>, OctyError> {
    let url = ctx.config.get_str("RYBBON_REWARD_CLAIM_URL")?.to_string();
    let partner_id = ctx.config.get_str("RYBBON_PARTNER_ID")?.to_string();
    let bearer = format!("Bearer {auth_token}");

    let mut rewards: Vec<Value> = Vec::new();
    for claims in claim_groups {
        let Some(first) = claims.first() else { continue };
        let campaign_key = first["campaignKey"].clone();

        // Filter to active, non-exceeded claims and strip the bookkeeping keys.
        let valid_claims: Vec<Value> = claims
            .iter()
            .filter(|c| c["active"] == json!(true) && c["exceeded"] == json!(false))
            .map(|c| {
                let mut cleaned = c.clone();
                if let Some(obj) = cleaned.as_object_mut() {
                    obj.remove("active");
                    obj.remove("exceeded");
                    obj.remove("campaignKey");
                }
                cleaned
            })
            .collect();
        if valid_claims.is_empty() {
            continue;
        }

        for chunk in valid_claims.chunks(100) {
            let post_body = json!({
                "campaignKey": campaign_key,
                "rewardClaims": chunk,
            });
            let (status, response) = http_post_with_retry(
                &url,
                &[
                    ("Partner-Id", partner_id.as_str()),
                    ("Authorization", bearer.as_str()),
                    ("Content-Type", "application/json"),
                ],
                &serde_json::to_vec(&post_body).expect("serializable json"),
            )
            .await?;
            eprintln!("[octy-messaging] POST Request: \"{url}\" returned response with valid status code: {status}");

            let parsed: Value = serde_json::from_slice(&response)
                .map_err(|e| OctyError::internal(format!("rybbon claim response not JSON: {e}")))?;
            // KeyError → 500 in the Python when 'success'/'rewardAvailable' missing.
            let success = parsed
                .get("success")
                .ok_or_else(|| OctyError::internal("rybbon claim response missing 'success'"))?;
            let reward_available = parsed
                .get("rewardAvailable")
                .ok_or_else(|| OctyError::internal("rybbon claim response missing 'rewardAvailable'"))?;
            if success != &json!(true) || reward_available != &json!(true) {
                continue;
            }
            rewards.extend(
                parsed
                    .get("result")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    }
    Ok(rewards)
}
