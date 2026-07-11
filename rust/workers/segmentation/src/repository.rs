//! Port of `data/repositories/implementation/segmentation_repository.py`.
//!
//! Two different backends are involved, exactly as in the Python service:
//!  * `get_profiles_by_id` / `get_events` call the **profile-service** /
//!    **event-service** directly over plain HTTPS
//!    (`Config['PROFILE_SERVICE_CLUSTER_IP']` / `Config['EVENT_SERVICE_CLUSTER_IP']`),
//!    using the same retrying session as the Python `requests_retry_session()`.
//!  * `get_segment_definitions` / `update_segment_profiles_ids` read/write the
//!    `tbl_segments` Mongo collection through the `octy-data-gateway` sidecar
//!    (`ctx.gateway`), since raw Mongo drivers are unavailable in WASM.
//!
//! `get_profiles` (the unfiltered/all-profiles variant) exists in the Python
//! interface but is never called by `segmentation_engine.py` — it is ported
//! here for completeness but is currently dead code, matching the original.

use octy_shared::errors::OctyError;
use octy_spin::ctx::Ctx;
use octy_spin::gateway::http_post_json_with_retry;
use serde_json::{json, Value};

const SEGMENTS_COLLECTION: &str = "tbl_segments";
const PROFILE_CHUNK_SIZE: usize = 2000;

fn chunks<T: Clone>(items: &[T], n: usize) -> Vec<Vec<T>> {
    items.chunks(n.max(1)).map(|c| c.to_vec()).collect()
}

/// `get_profiles` — fetches *all* profiles for an account (paginated via the
/// `cursor` header). Ported for parity; unused by the current engine.
#[allow(dead_code)]
pub async fn get_profiles(ctx: &Ctx, account_id: &str) -> Result<Vec<Value>, OctyError> {
    let url = format!(
        "{}/v1/internal/profiles?ids=false",
        ctx.config.get_str("PROFILE_SERVICE_CLUSTER_IP")?
    );
    let mut profiles = Vec::new();
    let mut cursor: i64 = 0;
    loop {
        let payload = json!({
            "account_id": account_id,
            "profiles": Vec::<String>::new(),
            "tag_statuses": ["active", "pending", "inactive"],
            "get_all": true,
        });
        let (status, body) = http_post_json_with_retry(
            &url,
            &[("cursor", &cursor.to_string())],
            &payload,
        )
        .await?;
        if status != 200 {
            break;
        }
        let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let batch = parsed
            .get("profiles")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let count = parsed
            .get("request_meta")
            .and_then(|m| m.get("count"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        profiles.extend(batch);
        cursor += count;
        if count == 0 {
            break;
        }
    }
    Ok(profiles)
}

/// `get_profiles_by_id` — chunks `profile_ids` by 2000, one non-paginated
/// request per chunk (the Python always sends `cursor: 0` here).
pub async fn get_profiles_by_id(
    ctx: &Ctx,
    account_id: &str,
    profile_ids: &[String],
) -> Result<Vec<Value>, OctyError> {
    let url = format!(
        "{}/v1/internal/profiles?ids=false",
        ctx.config.get_str("PROFILE_SERVICE_CLUSTER_IP")?
    );
    let mut profiles = Vec::new();
    for chunk in chunks(profile_ids, PROFILE_CHUNK_SIZE) {
        let payload = json!({
            "account_id": account_id,
            "profiles": chunk,
            "tag_statuses": ["active", "pending", "inactive"],
            "get_all": false,
        });
        let (status, body) = http_post_json_with_retry(&url, &[("cursor", "0")], &payload).await?;
        if status != 200 {
            // Python: `return profiles` (partial result) on non-200.
            return Ok(profiles);
        }
        let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let batch = parsed
            .get("profiles")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        profiles.extend(batch);
    }
    Ok(profiles)
}

/// `get_events` — paginates via the `cursor` header, incrementing by
/// `request_meta.count` until a non-200 response.
///
/// PYTHON BUG (preserved bug-for-bug): the `profile_ids` filter is only
/// included in the request payload when `profile_ids` is *falsy* (`None`/
/// empty) — i.e. exactly when there is nothing to filter by. When callers
/// actually pass profile IDs (`PendingLiveSegmentation._get_past_inaction_events`),
/// the `profile_ids` key is omitted entirely and the event-service receives an
/// unfiltered query. This almost certainly is not what was intended, but the
/// task calls for byte-for-byte orchestration parity, so it is reproduced here.
pub async fn get_events(
    ctx: &Ctx,
    account_id: &str,
    timeframe: i64,
    event_sequence_event: &Value,
    profile_ids: Option<&[String]>,
) -> Result<Vec<Value>, OctyError> {
    let url = format!(
        "{}/v1/internal/events",
        ctx.config.get_str("EVENT_SERVICE_CLUSTER_IP")?
    );
    let has_profile_ids = profile_ids.map(|p| !p.is_empty()).unwrap_or(false);
    let mut payload = json!({
        "event_sequence_event": event_sequence_event,
        "timeframe": timeframe,
        "account_id": account_id,
    });
    if !has_profile_ids {
        // Mirrors the Python `else` branch, which sends `profile_ids` (even
        // when it is `None`) only in the "no ids supplied" case.
        payload["profile_ids"] = profile_ids
            .map(|p| json!(p))
            .unwrap_or(Value::Null);
    }

    let mut events = Vec::new();
    let mut cursor: i64 = 0;
    loop {
        let (status, body) =
            http_post_json_with_retry(&url, &[("cursor", &cursor.to_string())], &payload).await?;
        if status != 200 {
            break;
        }
        let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let batch = parsed
            .get("events")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let count = parsed
            .get("request_meta")
            .and_then(|m| m.get("count"))
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

/// `get_segment_definitions` — reads `tbl_segments` via the gateway.
/// `segment_id` takes priority over `type_`, matching the Python signature
/// `get_segment_definitions(account_id, type_=None, segment_id=None)`.
pub async fn get_segment_definitions(
    ctx: &Ctx,
    account_id: &str,
    type_: Option<&str>,
    segment_id: Option<&str>,
) -> Result<Vec<Value>, OctyError> {
    let filter = if let Some(sid) = segment_id {
        // `segment_id` is the mongoengine primary key -> stored as Mongo `_id`.
        json!({ "_id": sid, "account_id": account_id, "status": "active" })
    } else {
        json!({ "segment_type": type_, "account_id": account_id, "status": "active" })
    };

    let docs = ctx.gateway.find(SEGMENTS_COLLECTION, filter, 0, 0).await?;
    let mut found_segments = Vec::with_capacity(docs.len());
    for mut doc in docs {
        if let Some(id) = doc.get("_id").cloned() {
            doc["segment_id"] = id;
        }
        if let Some(seq) = doc.get_mut("event_sequence").and_then(Value::as_array_mut) {
            for event in seq.iter_mut() {
                if event.get("event_properties").is_none() {
                    event["event_properties"] = Value::Null;
                }
            }
        }
        found_segments.push(doc);
    }
    Ok(found_segments)
}

/// `update_segment_profiles_ids` — Python drives this through a single-op
/// "bulk" write (`initialize_unordered_bulk_op` with one `find().update()`);
/// functionally equivalent to a single `update_one`, which is what the
/// gateway exposes.
pub async fn update_segment_profiles_ids(
    ctx: &Ctx,
    account_id: &str,
    segment_id: &str,
    matching_profile_ids: &[String],
) -> Result<(), OctyError> {
    ctx.gateway
        .update_one(
            SEGMENTS_COLLECTION,
            json!({ "account_id": account_id, "_id": segment_id }),
            json!({ "$set": { "profile_ids": matching_profile_ids } }),
        )
        .await
}
