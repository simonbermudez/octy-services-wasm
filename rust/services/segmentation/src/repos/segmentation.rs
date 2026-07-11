//! Port of `data/repositories/implementation/segmentation_repository.py`.
//!
//! MongoDB access goes through the octy-data-gateway sidecar; segment
//! documents travel as `bson.json_util`-style legacy extended JSON. The
//! custom-event-type lookup is a direct HTTP call to the events service
//! (`EVENT_SERVICE_CLUSTER_IP`), like the Python `requests_retry_session`.

use chrono::{TimeZone, Utc};
use octy_shared::errors::OctyError;
use serde_json::{json, Map, Value};

use octy_spin::ctx::Ctx;
use octy_spin::gateway::http_post_json_with_retry;

use crate::models::CreateSegment;

const COLLECTION: &str = "tbl_segments";

// ---------------------------------------------------------------------------
// `_format_segment`
// ---------------------------------------------------------------------------

/// Port of `_format_segment` — every call site first copied `_id` into
/// `segment_id`, so this helper does both.
pub fn format_segment(mut segment: Value, internal: bool) -> Value {
    if let Some(id) = segment.get("_id").cloned() {
        segment["segment_id"] = id;
    }

    // Ensure an event_properties key is present on every event.
    if let Some(events) = segment
        .get_mut("event_sequence")
        .and_then(Value::as_array_mut)
    {
        for event in events {
            if let Some(obj) = event.as_object_mut() {
                obj.entry("event_properties").or_insert(Value::Null);
            }
        }
    }

    let Some(obj) = segment.as_object_mut() else {
        return segment;
    };

    if internal {
        obj.remove("_id");
        return segment;
    }

    obj.remove("account_id");
    obj.remove("_id");
    obj.remove("id");

    let sub_type = obj
        .get("segment_sub_type")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if sub_type < 3 {
        obj.remove("profile_property_name");
        obj.remove("profile_property_value");
    }

    // created_at: stored as epoch millis (plain int); older docs may carry a
    // legacy `{"$date": millis}`. Falsy / missing → null; anything the Python
    // `int_to_dt` would have TypeError'd on is left untouched (the handler
    // swallowed KeyError/TypeError).
    let created_at = obj.get("created_at").cloned();
    let millis = match &created_at {
        Some(v) => {
            if let Some(ms) = v.as_i64() {
                if ms == 0 {
                    obj.insert("created_at".into(), Value::Null);
                    None
                } else {
                    Some(ms)
                }
            } else if v.is_object() {
                octy_shared::ejson::date_millis(v)
            } else if v.is_null() {
                obj.insert("created_at".into(), Value::Null);
                None
            } else {
                None // non-numeric, non-dict: left as-is (Python `except TypeError: pass`)
            }
        }
        None => {
            obj.insert("created_at".into(), Value::Null);
            None
        }
    };
    if let Some(ms) = millis {
        if let Some(dt) = Utc.timestamp_millis_opt(ms).single() {
            obj.insert(
                "created_at".into(),
                json!(dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string()),
            );
        }
    }

    let profile_count = obj
        .get("profile_ids")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    obj.insert("profile_count".into(), json!(profile_count));
    obj.remove("profile_ids");

    segment
}

// ---------------------------------------------------------------------------
// Repository methods
// ---------------------------------------------------------------------------

pub async fn get_segment_count(ctx: &Ctx, account_id: &str) -> Result<i64, OctyError> {
    ctx.gateway
        .count(COLLECTION, json!({ "account_id": account_id }))
        .await
}

pub async fn get_segment_by_identifiers(
    ctx: &Ctx,
    identifiers: &[String],
    account_id: &str,
) -> Result<(Vec<Value>, i64), OctyError> {
    let query = json!({
        "$and": [
            { "account_id": account_id },
            { "status": "active" },
            { "$or": [
                { "_id": { "$in": identifiers } },
                { "segment_name": { "$in": identifiers } }
            ] }
        ]
    });

    let docs = ctx.gateway.find(COLLECTION, query.clone(), 0, 100).await?;
    let total = ctx.gateway.count(COLLECTION, query).await?;

    let segments = docs
        .into_iter()
        .map(|doc| format_segment(doc, false))
        .collect();
    Ok((segments, total))
}

/// Duplicate lookup used by `SegmentValidatation._v_segment_duplicates`.
///
/// NB: the Python called this without `await` and with a pydantic model where
/// a dict was expected, so the whole duplicate check crashed (500) whenever it
/// ran; this ports the *intended* behavior.
pub async fn get_segment_by_attr(
    ctx: &Ctx,
    account_id: &str,
    segment: &CreateSegment,
) -> Result<Option<Value>, OctyError> {
    // First try to find by segment name.
    if let Some(found) = ctx
        .gateway
        .find_one(
            COLLECTION,
            json!({ "segment_name": segment.segment_name, "account_id": account_id }),
        )
        .await?
    {
        return Ok(Some(format_segment(found, false)));
    }

    // Then by type / sub type (+ profile properties when both provided).
    let mut query = json!({
        "account_id": account_id,
        "segment_type": segment.segment_type,
        "segment_sub_type": segment.segment_sub_type,
        "status": "active"
    });
    if segment.profile_property_name.is_some()
        && segment
            .profile_property_value
            .as_ref()
            .map(|v| !v.is_null())
            .unwrap_or(false)
    {
        query["profile_property_name"] = json!(segment.profile_property_name);
        query["profile_property_value"] = segment.profile_property_value_or_null();
    }

    let docs = ctx.gateway.find(COLLECTION, query, 0, 0).await?;

    let es_json_list: Vec<Value> = segment
        .event_sequence
        .iter()
        .map(|es| es.to_dict())
        .collect();

    for found in docs {
        let stored_es = found
            .get("event_sequence")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if stored_es == es_json_list
            && found.get("segment_type").and_then(Value::as_str) == Some(segment.segment_type.as_str())
            && found.get("segment_sub_type").and_then(Value::as_i64) == Some(segment.segment_sub_type)
            && found.get("segment_timeframe").and_then(Value::as_i64) == Some(segment.segment_timeframe)
            && found.get("profile_property_name").cloned().unwrap_or(Value::Null)
                == segment
                    .profile_property_name
                    .as_ref()
                    .map(|s| json!(s))
                    .unwrap_or(Value::Null)
            && found.get("profile_property_value").cloned().unwrap_or(Value::Null)
                == segment.profile_property_value_or_null()
        {
            return Ok(Some(format_segment(found, false)));
        }
    }

    Ok(None)
}

/// Raw (unformatted) past segments whose `profile_ids` intersect the given ids.
pub async fn get_past_segments_by_profile_ids(
    ctx: &Ctx,
    account_id: &str,
    profile_ids: &[String],
) -> Result<Vec<Value>, OctyError> {
    let query = json!({
        "$and": [
            { "account_id": account_id },
            { "segment_type": "past" },
            { "status": "active" },
            { "profile_ids": { "$in": profile_ids } }
        ]
    });
    // length=None in the Python → no limit (the gateway treats limit<=0 as unlimited).
    ctx.gateway.find(COLLECTION, query, 0, 0).await
}

pub async fn get_segments(
    ctx: &Ctx,
    account_id: &str,
    segment_type: &str,
    status: &str,
    cursor: i64,
    internal: bool,
) -> Result<(Vec<Value>, i64), OctyError> {
    let mut query = json!({ "account_id": account_id });
    if status != "all" {
        query["status"] = json!(status);
    }
    if segment_type != "all" {
        query["segment_type"] = json!(segment_type);
    }

    let docs = ctx.gateway.find(COLLECTION, query.clone(), cursor, 100).await?;
    let total = ctx.gateway.count(COLLECTION, query).await?;

    let segments = docs
        .into_iter()
        .map(|doc| format_segment(doc, internal))
        .collect();
    Ok((segments, total))
}

/// Insert the new segment document. `created_at` is a plain epoch-millis int
/// (`int(time.time() * 1000)` in the Python — *not* a BSON date).
pub async fn create_segment(ctx: &Ctx, segment: &Value) -> Result<(), OctyError> {
    let event_sequence: Vec<Value> = segment["event_sequence"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let document = json!({
        "_id": segment["segment_id"],
        "account_id": segment["account_id"],
        "segment_name": segment["segment_name"],
        "segment_type": segment["segment_type"],
        "segment_sub_type": segment["segment_sub_type"],
        "segment_timeframe": segment["segment_timeframe"],
        "event_sequence": event_sequence,
        "profile_property_name": segment["profile_property_name"],
        "profile_property_value": segment["profile_property_value"],
        "status": "active",
        "created_at": Utc::now().timestamp_millis(),
        "profile_ids": []
    });

    ctx.gateway.insert_one(COLLECTION, document).await?;
    Ok(())
}

/// Port of `update_past_segment_profile_ids` — including the Python quirk of
/// filtering on a `segment_id` *field* (the documents store the id in `_id`,
/// so this matches nothing and the update is a silent no-op, exactly as the
/// Python behaves in production).
pub async fn update_past_segment_profile_ids(
    ctx: &Ctx,
    account_id: &str,
    segment_id: &Value,
    profile_ids: &[Value],
) -> Result<(), OctyError> {
    ctx.gateway
        .update_one(
            COLLECTION,
            json!({ "account_id": account_id, "segment_id": segment_id }),
            json!({ "$set": { "profile_ids": profile_ids } }),
        )
        .await
}

/// Port of the bulk `delete_one` operations (the gateway has no bulk-write
/// endpoint; per-op calls have the same effect as `ordered=False`).
pub async fn delete_segments(
    ctx: &Ctx,
    account_id: &str,
    segment_ids: &[Value],
) -> Result<(), OctyError> {
    for segment in segment_ids {
        ctx.gateway
            .delete_one(
                COLLECTION,
                json!({ "$and": [
                    { "_id": segment["segment_id"] },
                    { "account_id": account_id }
                ] }),
            )
            .await?;
    }
    Ok(())
}

/// Custom event-type lookup against the events service
/// (`POST {EVENT_SERVICE_CLUSTER_IP}/v1/internal/events/types`).
/// Returns `(found_event_types, not_found)`.
pub async fn get_event_types_by_name(
    ctx: &Ctx,
    account_id: &str,
    event_type_names: &[String],
) -> Result<(Vec<Value>, Vec<String>), OctyError> {
    if event_type_names.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let url = format!(
        "{}/v1/internal/events/types",
        ctx.config.get_str("EVENT_SERVICE_CLUSTER_IP")?
    );
    let payload = json!({
        "account_id": account_id,
        "event_type_names": event_type_names,
    });

    let (status, body) = http_post_json_with_retry(&url, &[], &payload).await?;

    if status == 400 {
        return Ok((Vec::new(), event_type_names.to_vec()));
    }

    let parsed: Value = serde_json::from_slice(&body)
        .map_err(|e| OctyError::internal(format!("events service returned invalid JSON: {e}")))?;

    let found = parsed
        .get("event_types")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| OctyError::internal("events service response missing 'event_types'"))?;
    let not_found = parsed
        .get("not_found")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_string())
                .collect()
        })
        .ok_or_else(|| OctyError::internal("events service response missing 'not_found'"))?;

    Ok((found, not_found))
}

/// Port of `delete_account_segments` (`delete_many({"account_id": …})`).
///
/// GATEWAY GAP: the data gateway exposes no `delete-many` endpoint yet; this
/// posts to the hypothetical `POST /v1/mongo/{coll}/delete-many` expecting
/// `{"deleted_count": <n>}` and must be added to the gateway.
pub async fn delete_account_segments(_ctx: &Ctx, account_id: &str) -> Result<bool, OctyError> {
    let base = octy_spin::ctx::variable("gateway_url", "GATEWAY_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string());
    let url = format!(
        "{}/v1/mongo/{COLLECTION}/delete-many",
        base.trim_end_matches('/')
    );
    let body = json!({ "filter": { "account_id": account_id } });

    let (status, response) = octy_spin::gateway::http_send(
        spin_sdk::http::Method::Post,
        &url,
        &[("content-type", "application/json")],
        Some(serde_json::to_vec(&body).expect("serializable json")),
    )
    .await?;

    if !(200..300).contains(&status) {
        return Err(OctyError::internal(format!(
            "gateway delete-many returned status {status}"
        )));
    }

    let parsed: Map<String, Value> = serde_json::from_slice(&response).unwrap_or_default();
    Ok(parsed
        .get("deleted_count")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        > 0)
}
