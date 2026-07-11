//! Port of `data/repositories/implementation/messaging_repository.py`
//! (`_MessagingContentRepository`) — item recommendations and items come from
//! the recommendation/items services over plain HTTP (with the
//! `requests_retry_session` retry behaviour); currency rates live in
//! `tbl_currency_rates` behind the data gateway.

use octy_shared::ejson::date_millis;
use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::http::Method;

use octy_spin::ctx::Ctx;
use octy_spin::gateway::{http_post_json_with_retry, http_send};

const CURRENCY_RATES_COLLECTION: &str = "tbl_currency_rates";

/// GET with the `requests_retry_session` semantics (4 retries on
/// 500/502/504; immediate — no timer host call in the component).
async fn http_get_with_retry(
    url: &str,
    headers: &[(&str, &str)],
) -> Result<(u16, Vec<u8>), OctyError> {
    let mut last_err = OctyError::internal(format!("request to {url} failed"));
    for _attempt in 0..=4 {
        match http_send(Method::Get, url, headers, None).await {
            Ok((status, body)) if !matches!(status, 500 | 502 | 504) => return Ok((status, body)),
            Ok((status, _)) => last_err = OctyError::internal(format!("{url} returned status {status}")),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// `get_item_recommendations` — POST /v1/internal/recommendations on the
/// recommendation service. A 400 yields an empty list (like the Python).
pub async fn get_item_recommendations(
    ctx: &Ctx,
    account_id: &str,
    profile_ids: &[String],
) -> Result<Vec<Value>, OctyError> {
    let url = format!(
        "{}/v1/internal/recommendations",
        ctx.config.get_str("REC_SERVICE_CLUSTER_IP")?
    );
    let payload = json!({ "account_id": account_id, "profile_ids": profile_ids });
    let (status, body) = http_post_json_with_retry(&url, &[], &payload).await?;
    eprintln!("[octy-messaging] POST Request: \"{url}\" returned response with valid status code: {status}");
    if status == 400 {
        return Ok(Vec::new());
    }
    let parsed: Value = serde_json::from_slice(&body)
        .map_err(|e| OctyError::internal(format!("recommendations response not JSON: {e}")))?;
    parsed
        .get("recommendations")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| OctyError::internal("recommendations response missing 'recommendations'"))
}

/// `get_items` — paginate GET /v1/internal/items on the items service using
/// the `cursor` header until a non-200 response.
pub async fn get_items(ctx: &Ctx, account_id: &str) -> Result<Vec<Value>, OctyError> {
    let url = format!(
        "{}/v1/internal/items?account_id={}&ids=false&status=active",
        ctx.config.get_str("ITEM_SERVICE_CLUSTER_IP")?,
        account_id
    );

    let mut items: Vec<Value> = Vec::new();
    let mut cursor: i64 = 0;
    loop {
        let cursor_str = cursor.to_string();
        let (status, body) = http_get_with_retry(&url, &[("cursor", &cursor_str)]).await?;
        eprintln!("[octy-messaging] GET Request: \"{url}\" returned response with valid status code: {status}");
        if status != 200 {
            break;
        }
        let parsed: Value = serde_json::from_slice(&body)
            .map_err(|e| OctyError::internal(format!("items response not JSON: {e}")))?;
        let page = parsed
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| OctyError::internal("items response missing 'items'"))?;
        items.extend(page);
        let count = parsed["request_meta"]["count"]
            .as_i64()
            .ok_or_else(|| OctyError::internal("items response missing 'request_meta.count'"))?;
        if count < 1 {
            // Guard against an infinite loop on a stuck cursor (the Python
            // relied on the items service eventually returning non-200).
            break;
        }
        cursor += count;
    }
    Ok(items)
}

/// `get_currency_rates` — the newest `tbl_currency_rates` document's `rates`.
///
/// mongoengine did `order_by('-created_at').first()`; the gateway `find` has
/// no sort option (capability gap), so fetch and pick the max client-side.
pub async fn get_currency_rates(ctx: &Ctx) -> Result<Value, OctyError> {
    let docs = ctx
        .gateway
        .find(CURRENCY_RATES_COLLECTION, json!({}), 0, 0)
        .await?;
    let latest = docs
        .into_iter()
        .max_by_key(|d| d.get("created_at").and_then(date_millis).unwrap_or(i64::MIN))
        .ok_or_else(|| OctyError::internal("no tbl_currency_rates documents found"))?;
    Ok(latest.get("rates").cloned().unwrap_or(Value::Null))
}
