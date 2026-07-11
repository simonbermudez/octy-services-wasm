//! Ports of `data/repositories/implementation/{account,algorithm}_config_repository.py`.
//!
//! Neither repository touches MongoDB — both publish update commands to
//! RabbitMQ (via the data gateway's `/v1/amqp/publish`), and the algorithm
//! repository additionally pages the internal items endpoint of the items
//! service over plain HTTP.

use octy_shared::errors::OctyError;
use octy_spin::ctx::Ctx;
use octy_spin::gateway::http_send;
use serde_json::{json, Value};
use spin_sdk::http::Method;

use crate::models::SetAccountConfigs;

/// `_AccountConfigRepository.set_account_configs` — publish
/// `account.configs.cmd.update`.
pub async fn set_account_configs(ctx: &Ctx, account: &SetAccountConfigs) -> Result<(), OctyError> {
    ctx.gateway
        .amqp_publish(
            "account.configs.cmd.update",
            &json!({
                "account_id": account.account_id,
                "contact_email_address": account.contact_email_address,
                "contact_name": account.contact_name,
                "contact_surname": account.contact_surname,
                "webhook_url": account.webhook_url,
                "authenticated_id_key": account.authenticated_id_key,
            }),
        )
        .await
}

/// `_AlgorithmConfigRepository.set_algorithm_configs` — publish
/// `algo.configs.cmd.update`. `config_json` is the full pydantic
/// `configurations.dict()` (including `event_type` and the item-identifier
/// key, and — for `rec` — the stop list already mutated into
/// `{"item_id": …}` objects).
pub async fn set_algorithm_configs(
    ctx: &Ctx,
    account_id: &str,
    algorithm_name: &str,
    config_json: &Value,
) -> Result<(), OctyError> {
    ctx.gateway
        .amqp_publish(
            "algo.configs.cmd.update",
            &json!({
                "account_id": account_id,
                "algorithm_configurations": {
                    "algorithm_name": algorithm_name,
                    "config_json": config_json,
                }
            }),
        )
        .await
}

/// `requests_retry_session().get(...)` — retry 500/502/504 up to 4 times.
/// WASI exposes no timer host call in this component, so retries are
/// immediate instead of backing off (same divergence as the account port).
/// Exhausted retries raise, like urllib3's `MaxRetryError` did.
async fn get_with_retry(url: &str, cursor: i64) -> Result<(u16, Vec<u8>), OctyError> {
    let cursor_header = cursor.to_string();
    let mut last_status = 0u16;
    for _attempt in 0..=4 {
        let (status, body) =
            http_send(Method::Get, url, &[("cursor", &cursor_header)], None).await?;
        if !matches!(status, 500 | 502 | 504) {
            return Ok((status, body));
        }
        last_status = status;
    }
    Err(OctyError::internal(format!(
        "item service {url} returned status {last_status} after retries"
    )))
}

/// `_AlgorithmConfigRepository.get_items` — page the items service's
/// internal endpoint (`?ids=true&status=all`) via the `cursor` header until
/// it answers non-200. Item ids are kept as raw JSON values, matching the
/// Python's untyped `item['item_id']` accesses.
pub async fn get_items(ctx: &Ctx, account_id: &str) -> Result<Vec<Value>, OctyError> {
    eprintln!("Getting items for account: {account_id}");
    let base = ctx.config.get_str("ITEM_SERVICE_CLUSTER_IP")?;
    // ?ids=true only return item ids
    let url = format!("{base}/v1/internal/items?account_id={account_id}&ids=true&status=all");

    let mut item_ids: Vec<Value> = Vec::new();
    let mut cursor: i64 = 0;

    loop {
        let (status, body) = get_with_retry(&url, cursor).await?;
        eprintln!("GET Request: \"{url}\" returned response with valid status code: {status}");

        if status != 200 {
            break;
        }

        let parsed: Value = serde_json::from_slice(&body)
            .map_err(|e| OctyError::internal(format!("item service returned invalid JSON: {e}")))?;
        let items = parsed
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| OctyError::internal("item service response missing 'items' list"))?;
        for item in items {
            item_ids.push(item.get("item_id").cloned().ok_or_else(|| {
                OctyError::internal("KeyError: 'item_id' missing from item service response")
            })?);
        }

        let count = parsed["request_meta"]["count"].as_i64().ok_or_else(|| {
            OctyError::internal("item service response missing integer request_meta.count")
        })?;
        // Divergence: a 200 with count == 0 would loop forever in the Python;
        // stop instead of spinning.
        if count <= 0 {
            break;
        }
        cursor += count;
    }

    Ok(item_ids)
}
