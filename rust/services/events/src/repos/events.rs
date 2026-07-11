//! Port of `data/repositories/implementation/events_repository.py`.
//!
//! MongoDB goes through the data gateway (documents as legacy extended
//! JSON). The profile / segmentation cluster-internal HTTP calls go straight
//! out of the component (like the Python `requests_retry_session`).

use chrono::{DateTime, Duration, Utc};
use octy_shared::ejson::{date_millis, legacy_date};
use octy_shared::utils::int_to_dt;
use serde_json::{json, Map, Value};
use spin_sdk::http::Method;

use octy_spin::ctx::Ctx;
use octy_spin::gateway::{http_post_json_with_retry, http_send};

use crate::gateway_ext::GatewayExt;
use crate::http_util::{ApiError, GMT_FMT};
use crate::models::UpdateEventsOwnerChild;

const COLLECTION: &str = "tbl_event_instances";

/// `int_to_dt(doc['created_at'], as_str=True)` — the Python actually crashed
/// here (it passed a datetime where the helper expects epoch millis); the
/// port implements the intent: legacy `{"$date": millis}` → GMT string.
fn format_created_at(doc: &mut Value) {
    let formatted = doc
        .get("created_at")
        .and_then(date_millis)
        .and_then(int_to_dt)
        .map(|d| d.format(GMT_FMT).to_string());
    doc["created_at"] = match formatted {
        Some(s) => json!(s),
        None => Value::Null,
    };
}

/// `create_event` — single insert with a server-side `created_at`.
pub async fn create_event(ctx: &Ctx, account_id: &str, event: &Value) -> Result<(), ApiError> {
    let document = json!({
        "_id": event["event_id"],
        "account_id": account_id,
        "profile_id": event["profile_id"],
        "event_type_id": event["event_type_id"],
        "event_type": event["event_type"],
        "event_properties": event["event_properties"],
        "created_at": legacy_date(Utc::now()),
    });
    // The Python wrapped this in try/except { print; raise } → any failure
    // (including duplicate keys) surfaced as the generic 500.
    ctx.gateway
        .insert_one(COLLECTION, document)
        .await
        .map_err(|e| ApiError::internal(format!("Error creating event: {e}")))?;
    Ok(())
}

/// `get_latest_checkout_info_submmited_event` (sic — name kept from Python).
pub async fn get_latest_checkout_info_submmited_event(
    account_id: &str,
    checkout_id: &str,
) -> Result<Option<Value>, ApiError> {
    let ext = GatewayExt::load();
    let docs = ext
        .find_sorted(
            COLLECTION,
            json!({
                "account_id": account_id,
                "event_type": "checkout_contact_info_submitted",
                "event_properties.checkout_id": checkout_id,
            }),
            json!({ "created_at": -1 }),
            0,
            1,
        )
        .await
        .map_err(ApiError::from)?;

    let Some(mut doc) = docs.into_iter().next() else {
        return Ok(None);
    };
    // Python quirk kept: the `_id` is exposed as `event_type_id` (not event_id).
    let id = doc["_id"].clone();
    doc["event_type_id"] = id;
    if let Some(obj) = doc.as_object_mut() {
        obj.remove("_id");
    }
    format_created_at(&mut doc);
    Ok(Some(doc))
}

/// `batch_create_events` — unordered insert_many; per-op write errors are
/// tolerated (the Python collected them but the caller discarded the result).
pub async fn batch_create_events(
    ctx: &Ctx,
    account_id: &str,
    events_batch: &[Value],
) -> Result<(), ApiError> {
    let _ = ctx; // Mongo access goes through the extension client below.
    let documents: Vec<Value> = events_batch
        .iter()
        .map(|event| {
            json!({
                "_id": event["event_id"],
                "account_id": account_id,
                "profile_id": event["profile_id"],
                "event_type_id": event["event_type_id"],
                "event_type": event["event_type"],
                "event_properties": event["event_properties"],
                "created_at": event["created_at"],
            })
        })
        .collect();

    let ext = GatewayExt::load();
    // BulkWriteError was caught in Python (duplicates → failed_to_create,
    // which the service ignored); any other failure propagated → 500.
    let _write_errors = ext
        .insert_many(COLLECTION, documents, false)
        .await
        .map_err(ApiError::from)?;
    Ok(())
}

/// `get_events_meta` — latest event instance per provided event type plus the
/// account's total event count. The Python used a single `$facet` aggregation;
/// per-type sorted finds are semantically identical and avoid needing an
/// aggregate endpoint on the gateway.
///
/// Used by `verify_event` to infer each event property's expected data type
/// from the most recent instance, and by the resource-limit check to confirm
/// the account hasn't exceeded its event creation limit.
pub async fn get_events_meta(
    ctx: &Ctx,
    account_id: &str,
    event_type_list: &[String],
) -> Result<(Vec<Value>, i64), ApiError> {
    if event_type_list.is_empty() {
        // `$facet: {}` raised in Mongo → generic 500 in the Python service.
        return Err(ApiError::internal(
            "$facet specification must have at least one field (empty event_type_list)",
        ));
    }

    let event_count = ctx
        .gateway
        .count(COLLECTION, json!({ "account_id": account_id }))
        .await
        .map_err(ApiError::from)?;

    let ext = GatewayExt::load();
    let mut result = Vec::new();
    for event_type in event_type_list {
        let docs = ext
            .find_sorted(
                COLLECTION,
                json!({ "account_id": account_id, "event_type": event_type }),
                json!({ "created_at": -1 }),
                0,
                1,
            )
            .await
            .map_err(ApiError::from)?;
        if let Some(doc) = docs.first() {
            result.push(json!({
                "event_type": doc["event_type"],
                "event_properties": doc["event_properties"],
            }));
        }
    }
    Ok((result, event_count))
}

/// `get_events` — paginated timeframe query (used by /v1/internal/events).
pub async fn get_events(
    ctx: &Ctx,
    account_id: &str,
    timeframe: i64,
    cursor: i64,
    event_sequence_event: Option<&Value>,
    profile_ids: Option<&Vec<String>>,
    event_type: Option<&str>,
) -> Result<(Vec<Value>, i64), ApiError> {
    let from_dt: DateTime<Utc> = Utc::now() - Duration::minutes(timeframe + 1);
    let mut query = Map::new();
    query.insert("account_id".to_string(), json!(account_id));
    query.insert("created_at".to_string(), json!({ "$gt": legacy_date(from_dt) }));

    // `if event_sequence_event:` — empty dicts are falsy in Python.
    let ese = event_sequence_event.filter(|v| v.as_object().is_some_and(|o| !o.is_empty()));
    let pids = profile_ids.filter(|p| !p.is_empty());
    let et = event_type.filter(|t| !t.is_empty());

    if let Some(ese) = ese {
        let ese_event_type = ese
            .get("event_type")
            .ok_or_else(|| ApiError::internal("KeyError: event_sequence_event['event_type']"))?;
        query.insert("event_type".to_string(), ese_event_type.clone());
        if let Some(props) = ese.get("event_properties") {
            let props = props
                .as_object()
                .ok_or_else(|| ApiError::internal("event_sequence_event['event_properties'] is not a dict"))?;
            for (k, v) in props {
                query.insert(format!("event_properties.{k}"), v.clone());
            }
        }
    } else if let (Some(pids), Some(et)) = (pids, et) {
        query.insert("profile_id".to_string(), json!({ "$in": pids }));
        query.insert("event_type".to_string(), json!(et));
    } else if let Some(pids) = pids {
        query.insert("profile_id".to_string(), json!({ "$in": pids }));
    }

    let query = Value::Object(query);
    let mut raw = ctx
        .gateway
        .find(COLLECTION, query.clone(), cursor, 3000)
        .await
        .map_err(ApiError::from)?;
    let total = ctx
        .gateway
        .count(COLLECTION, query)
        .await
        .map_err(ApiError::from)?;

    for doc in &mut raw {
        let id = doc["_id"].clone();
        doc["event_id"] = id;
        if let Some(obj) = doc.as_object_mut() {
            obj.remove("_id");
        }
        format_created_at(doc);
    }
    Ok((raw, total))
}

/// `update_events_owner` — the Python issued one `UpdateOne` per child
/// profile (a single matching event each, quirk preserved).
pub async fn update_events_owner(
    ctx: &Ctx,
    account_id: &str,
    profiles: &[UpdateEventsOwnerChild],
) -> Result<(), ApiError> {
    let all_child_profile_ids: Vec<&String> =
        profiles.iter().flat_map(|p| p.child_profiles.iter()).collect();

    for cpi in all_child_profile_ids {
        let parent = profiles
            .iter()
            .find(|p| p.child_profiles.contains(cpi))
            .map(|p| p.parent_profile.as_str());
        if let Some(parent) = parent {
            ctx.gateway
                .update_one(
                    COLLECTION,
                    json!({ "account_id": account_id, "profile_id": cpi }),
                    json!({ "$set": { "profile_id": parent } }),
                )
                .await
                .map_err(ApiError::from)?;
        }
    }
    Ok(())
}

/// `delete_profile_events`.
pub async fn delete_profile_events(account_id: &str, profile_id: &str) -> Result<(), ApiError> {
    GatewayExt::load()
        .delete_many(COLLECTION, json!({ "account_id": account_id, "profile_id": profile_id }))
        .await
        .map_err(ApiError::from)?;
    Ok(())
}

/// `delete_account_events`.
pub async fn delete_account_events(account_id: &str) -> Result<(), ApiError> {
    GatewayExt::load()
        .delete_many(COLLECTION, json!({ "account_id": account_id }))
        .await
        .map_err(ApiError::from)?;
    Ok(())
}

/// `get_profile_ids` — POST to the profiles service internal endpoint with
/// retry-on-500/502/504 (port of `requests_retry_session`).
pub async fn get_profile_ids(
    ctx: &Ctx,
    account_id: &str,
    profile_ids: &[String],
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let base = ctx
        .config
        .get_str("PROFILE_SERVICE_CLUSTER_IP")
        .map_err(ApiError::from)?
        .to_string();
    let url = format!("{base}/v1/internal/profiles?ids=true");
    let payload = json!({
        "account_id": account_id,
        "profiles": profile_ids,
        "get_all": false,
    });

    let (status, body) = http_post_json_with_retry(&url, &[], &payload)
        .await
        .map_err(|e| ApiError::internal(format!("Profile API error: {e}")))?;
    // response.raise_for_status()
    if status >= 400 {
        return Err(ApiError::internal(format!("Profile API error: status {status}")));
    }
    let data: Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::internal(format!("Profile API error: invalid JSON response: {e}")))?;

    let valid_profiles = data
        .get("profiles")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let invalid_profiles = data
        .get("not_found")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok((valid_profiles, invalid_profiles))
}

/// GET with the same retry policy as `requests_retry_session` (immediate
/// retries — no timer host call in the component).
async fn http_get_with_retry(url: &str) -> Result<(u16, Vec<u8>), ApiError> {
    let mut last_err = ApiError::internal(format!("request to {url} failed"));
    for _attempt in 0..=4 {
        match http_send(Method::Get, url, &[], None).await {
            Ok((status, body)) if !matches!(status, 500 | 502 | 504) => return Ok((status, body)),
            Ok((status, _)) => last_err = ApiError::internal(format!("{url} returned status {status}")),
            Err(e) => last_err = ApiError::from(e),
        }
    }
    Err(last_err)
}

/// `get_live_segment_definitions` — GET to the segmentation service.
pub async fn get_live_segment_definitions(ctx: &Ctx, account_id: &str) -> Result<Vec<Value>, ApiError> {
    let base = ctx
        .config
        .get_str("SEGMENTATION_SERVICE_CLUSTER_IP")
        .map_err(ApiError::from)?
        .to_string();
    let url = format!(
        "{base}/v1/internal/segments?account_id={account_id}&status=active&segment_type=live"
    );

    let (status, body) = http_get_with_retry(&url).await
        .map_err(|e| ApiError::internal(format!("Segment API error: {e}")))?;
    if status >= 400 {
        return Err(ApiError::internal(format!("Segment API error: status {status}")));
    }
    let data: Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::internal(format!("Segment API error: invalid JSON response: {e}")))?;
    Ok(data
        .get("segments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}
