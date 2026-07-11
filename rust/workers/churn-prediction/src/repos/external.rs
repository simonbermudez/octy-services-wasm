//! HTTP side of `data/repositories/implementation/churn_repository.py`:
//! `get_events`, `get_profiles`, `get_items` — internal calls to the events /
//! profiles / items services, paginated with a `cursor` request header and a
//! `request_meta.count` response field, exactly like the Python.
//!
//! `get_segments` is ported nowhere: it exists on the Python repository but
//! is never called by `ChurnPredictionTraining` / `ChurnPredictionCompleteTrainingJob`
//! (dead code in the original service).
//!
//! DIVERGENCE: the Python pagination loop has no `count == 0` guard and
//! relies entirely on the upstream service eventually answering non-200 to
//! terminate — if a service ever kept returning 200 with `count: 0` the
//! Python would spin forever. A whole job here runs synchronously inside one
//! `/internal/amqp/consume` HTTP request, so an infinite loop would hang the
//! gateway's forwarded request rather than a background thread; each loop
//! below also stops once `count == 0`.

use octy_shared::errors::OctyError;
use octy_spin::ctx::Ctx;
use serde_json::{json, Value};

use crate::util::http_post_json_with_retry;

/// `get_events(account_id, profile_ids, timeframe, event_type)`.
pub async fn get_events(
    ctx: &Ctx,
    account_id: &str,
    profile_ids: &[String],
    timeframe: i64,
    event_type: &str,
) -> Result<Vec<Value>, OctyError> {
    let base = ctx.config.get_str("EVENT_SERVICE_CLUSTER_IP")?;
    let url = format!("{base}/v1/internal/events");
    let payload = json!({
        "timeframe": timeframe,
        "account_id": account_id,
        "profile_ids": profile_ids,
        "event_type": event_type,
    });

    let mut events = Vec::new();
    let mut cursor: i64 = 0;
    loop {
        let cursor_header = cursor.to_string();
        let (status, body) =
            http_post_json_with_retry(&url, &[("cursor", cursor_header.as_str())], &payload).await?;
        if status != 200 {
            break;
        }
        let parsed: Value = serde_json::from_slice(&body)
            .map_err(|e| OctyError::internal(format!("get_events: invalid JSON body: {e}")))?;
        let batch = parsed
            .get("events")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let count = parsed
            .pointer("/request_meta/count")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        events.extend(batch);
        cursor += count;
        if count == 0 {
            break;
        }
    }
    Ok(events)
}

/// `get_profiles(account_id, status, ids)`.
pub async fn get_profiles(
    ctx: &Ctx,
    account_id: &str,
    status: &str,
    ids: &str,
) -> Result<Vec<Value>, OctyError> {
    let base = ctx.config.get_str("PROFILE_SERVICE_CLUSTER_IP")?;
    let url = format!("{base}/v1/internal/profiles?ids={ids}&status={status}");
    let payload = json!({
        "account_id": account_id,
        "profiles": [],
        "get_all": true,
    });

    let mut profiles = Vec::new();
    let mut cursor: i64 = 0;
    loop {
        let cursor_header = cursor.to_string();
        let (status_code, body) =
            http_post_json_with_retry(&url, &[("cursor", cursor_header.as_str())], &payload).await?;
        if status_code != 200 {
            break;
        }
        let parsed: Value = serde_json::from_slice(&body)
            .map_err(|e| OctyError::internal(format!("get_profiles: invalid JSON body: {e}")))?;
        let batch = parsed
            .get("profiles")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let count = parsed
            .pointer("/request_meta/count")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if ids == "true" {
            profiles.extend(
                batch
                    .iter()
                    .filter_map(|p| p.get("profile_id").cloned()),
            );
        } else {
            profiles.extend(batch);
        }
        cursor += count;
        if count == 0 {
            break;
        }
    }
    Ok(profiles)
}

/// `get_items(account_id, ids)` — a GET request (unlike events/profiles).
pub async fn get_items(ctx: &Ctx, account_id: &str, ids: &str) -> Result<Vec<Value>, OctyError> {
    let base = ctx.config.get_str("ITEM_SERVICE_CLUSTER_IP")?;
    let url = format!("{base}/v1/internal/items?account_id={account_id}&ids={ids}&status=all");

    let mut items = Vec::new();
    let mut cursor: i64 = 0;
    loop {
        let cursor_header = cursor.to_string();
        let (status_code, body) =
            crate::util::http_get_with_retry(&url, &[("cursor", cursor_header.as_str())]).await?;
        if status_code != 200 {
            break;
        }
        let parsed: Value = serde_json::from_slice(&body)
            .map_err(|e| OctyError::internal(format!("get_items: invalid JSON body: {e}")))?;
        let batch = parsed
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let count = parsed
            .pointer("/request_meta/count")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if ids == "true" {
            items.extend(batch.iter().filter_map(|i| i.get("item_id").cloned()));
        } else {
            items.extend(batch);
        }
        cursor += count;
        if count == 0 {
            break;
        }
    }
    Ok(items)
}
