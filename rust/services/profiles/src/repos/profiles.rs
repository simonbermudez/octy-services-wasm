//! Port of `data/repositories/implementation/profiles_repository.py`.
//!
//! MongoDB access goes through the `octy-data-gateway` sidecar; the Redis
//! `{account_id}_profile_key_types` set (db index **1**, matching
//! `data/context/db_context.py::db_redis_connect`) is written directly via
//! Spin's outbound-Redis host capability.

use std::collections::{HashMap, HashSet};

use octy_shared::ejson::now_legacy_date;
use octy_shared::errors::OctyError;
use serde_json::{json, Value};

use crate::gateway_ext;
use crate::utils::int_to_dt_str;
use octy_spin::ctx::Ctx;

pub const COLLECTION: &str = "tbl_profiles";
pub const MERGED_COLLECTION: &str = "tbl_merged_profiles";

// ---------------------------------------------------------------------
// Formatting (port of `_format_profile` / `_format_segment_tags`)
// ---------------------------------------------------------------------

/// Port of `_format_profile`. Mutates `doc` in place: `_id` -> `profile_id`,
/// drops `account_id`, drops `ltv_prediction`/`current_ltv` unless
/// `internal`, filters `segment_tags` by `tag_statuses`, and renders
/// `{"$date": millis}` values as the Python `int_to_dt(..., as_str=True)`
/// formatted string.
pub fn format_profile(doc: &mut Value, tag_statuses: &[String], internal: bool) {
    let Some(obj) = doc.as_object_mut() else { return };

    if let Some(id) = obj.remove("_id") {
        obj.insert("profile_id".to_string(), id);
    }
    obj.remove("account_id");

    if !internal {
        obj.remove("ltv_prediction");
        obj.remove("current_ltv");
    }

    if let Some(Value::Array(tags)) = obj.get("segment_tags").cloned() {
        let mut valid = Vec::new();
        for mut tag in tags {
            let status_ok = tag
                .get("status")
                .and_then(Value::as_str)
                .map(|s| tag_statuses.iter().any(|t| t == s))
                .unwrap_or(false);
            if !status_ok {
                continue;
            }
            if !internal {
                if let Some(tobj) = tag.as_object_mut() {
                    tobj.remove("segment_id");
                    tobj.remove("status");
                    tobj.remove("updated_at");
                    if let Some(millis) = tobj.get("created_at").and_then(|c| c.get("$date")).and_then(Value::as_i64) {
                        if let Some(s) = int_to_dt_str(millis) {
                            tobj.insert("created_at".to_string(), Value::String(s));
                        }
                    }
                }
            }
            valid.push(tag);
        }
        obj.insert("segment_tags".to_string(), Value::Array(valid));
    }

    for key in ["created_at", "updated_at"] {
        if let Some(millis) = obj.get(key).and_then(|v| v.get("$date")).and_then(Value::as_i64) {
            if let Some(s) = int_to_dt_str(millis) {
                obj.insert(key.to_string(), Value::String(s));
            }
        }
    }
}

/// Port of `_format_segment_tags` — merge new segment tags into the existing
/// list, updating matches in place and appending new ones.
///
/// NOTE: the Python implementation stamped timestamps with
/// `dt.now(tz.now)` — `timezone` has no `now` attribute, so this raised
/// `AttributeError` on every call in production (an unreachable/broken code
/// path, not a preservable quirk). This port implements the evident intent:
/// `dt.now(tz.utc)`, i.e. the current UTC time.
pub fn format_segment_tags(new_tags: &[Value], existing_tags: &[Value]) -> Vec<Value> {
    let mut formatted: Vec<Value> = existing_tags.to_vec();
    for tag in new_tags {
        let seg_id = tag.get("segment_id").and_then(Value::as_str).map(str::to_string);
        let found = formatted
            .iter()
            .position(|t| t.get("segment_id").and_then(Value::as_str).map(str::to_string) == seg_id);
        match found {
            Some(pos) => {
                if let (Some(existing_obj), Some(new_obj)) = (formatted[pos].as_object().cloned(), tag.as_object()) {
                    let mut merged = existing_obj;
                    for (k, v) in new_obj {
                        merged.insert(k.clone(), v.clone());
                    }
                    merged.insert("updated_at".to_string(), now_legacy_date());
                    formatted[pos] = Value::Object(merged);
                }
            }
            None => {
                let mut t = tag.clone();
                if let Some(obj) = t.as_object_mut() {
                    obj.insert("created_at".to_string(), now_legacy_date());
                }
                formatted.push(t);
            }
        }
    }
    formatted
}

// ---------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------

pub async fn get_profile_count(ctx: &Ctx, account_id: &str) -> Result<i64, OctyError> {
    ctx.gateway.count(COLLECTION, json!({ "account_id": account_id })).await
}

/// Port of `get_profiles_by_identifiers`.
///
/// NOTE (preserved Python bug): `not_found` is computed by diffing the
/// requested identifiers against the set of *found profile_ids only* — a
/// `customer_id` identifier that *did* match a profile is still reported as
/// "not found" because `found_ids` never contains customer_id values. This
/// port reproduces that behaviour byte-for-byte.
pub async fn get_profiles_by_identifiers(
    ctx: &Ctx,
    account_id: &str,
    identifiers: &[String],
    tag_statuses: &[String],
    ids: bool,
    internal: bool,
) -> Result<(Vec<Value>, Vec<String>), OctyError> {
    let filter = json!({
        "$and": [
            { "$or": [ { "_id": { "$in": identifiers } }, { "customer_id": { "$in": identifiers } } ] },
            { "account_id": account_id }
        ]
    });
    let docs = ctx.gateway.find(COLLECTION, filter, 0, 0).await?;

    let mut found = Vec::with_capacity(docs.len());
    let mut found_ids: HashSet<String> = HashSet::new();
    for mut doc in docs {
        let profile_id = doc.get("_id").and_then(Value::as_str).unwrap_or_default().to_string();
        found_ids.insert(profile_id.clone());
        if ids {
            found.push(json!({ "profile_id": profile_id }));
        } else {
            format_profile(&mut doc, tag_statuses, internal);
            found.push(doc);
        }
    }

    let not_found = identifiers
        .iter()
        .filter(|id| !found_ids.contains(*id))
        .cloned()
        .collect();

    Ok((found, not_found))
}

/// Port of `get_profiles_by_params`.
pub async fn get_profiles_by_params(
    ctx: &Ctx,
    account_id: &str,
    cursor: i64,
    segments: Option<&[String]>,
    rfm_values: Option<&[i64]>,
    churn_prob: Option<&str>,
) -> Result<(Vec<Value>, i64), OctyError> {
    let mut query = json!({ "account_id": account_id });

    // NOTE: the Python service indexes `rfm_values[0]`/`rfm_values[1]`
    // unconditionally once the list is non-empty; a single-value range (e.g.
    // `?rfm=-5` or `?rfm=5-`, both of which pass its format validator) would
    // raise an unhandled `IndexError` -> generic 500. This port implements
    // the evident intent instead: the range filter only applies once both
    // bounds are present.
    if let Some(rfm) = rfm_values {
        if rfm.len() >= 2 {
            query["rfm_score"] = json!({ "$gt": rfm[0], "$lt": rfm[1] });
        }
    }

    if let Some(cp) = churn_prob {
        query["churn_probability"] = json!(cp);
    }

    if let Some(segs) = segments {
        if segs.len() == 1 {
            query["segment_tags.segment_tag"] = json!(segs[0]);
            query["segment_tags.status"] = json!("active");
        } else if segs.len() > 1 {
            query["$or"] = json!(segs
                .iter()
                .map(|s| json!({ "$and": [ { "segment_tags.segment_tag": s }, { "segment_tags.status": "active" } ] }))
                .collect::<Vec<_>>());
        }
    }

    let total = ctx.gateway.count(COLLECTION, query.clone()).await?;
    let docs = ctx.gateway.find(COLLECTION, query, cursor, 100).await?;

    let mut profiles = Vec::with_capacity(docs.len());
    for mut doc in docs {
        format_profile(&mut doc, &["active".to_string()], false);
        profiles.push(doc);
    }

    Ok((profiles, total))
}

/// Port of `get_all_profiles`.
pub async fn get_all_profiles(
    ctx: &Ctx,
    account_id: &str,
    tag_statuses: &[String],
    cursor: i64,
    ids: bool,
    status: &str,
    limit: i64,
    internal: bool,
) -> Result<(Vec<Value>, i64), OctyError> {
    let query = json!({ "account_id": account_id, "status": status });
    let total = ctx.gateway.count(COLLECTION, query.clone()).await?;
    let docs = ctx.gateway.find(COLLECTION, query, cursor, limit).await?;

    let mut profiles = Vec::with_capacity(docs.len());
    for mut doc in docs {
        if ids {
            let profile_id = doc.get("_id").and_then(Value::as_str).unwrap_or_default().to_string();
            profiles.push(json!({ "profile_id": profile_id }));
        } else {
            format_profile(&mut doc, tag_statuses, internal);
            profiles.push(doc);
        }
    }

    Ok((profiles, total))
}

/// Port of `get_merged_profiles`.
///
/// NOTE (preserved Python bug): a single aggregation pipeline is built by
/// concatenating a `$match`/`$sort`/`$limit` triple *per identifier*. Mongo
/// executes those stages sequentially against the *already-filtered* working
/// set, so for two or more identifiers the second `$match` narrows what the
/// first one already matched instead of running an independent lookup. This
/// port reproduces that exact pipeline shape.
pub async fn get_merged_profiles(ctx: &Ctx, account_id: &str, identifiers: &[String]) -> Result<Vec<Value>, OctyError> {
    let _ = ctx;
    if identifiers.is_empty() {
        return Ok(vec![]);
    }
    let mut pipeline = Vec::with_capacity(identifiers.len() * 3);
    for identifier in identifiers {
        pipeline.push(json!({
            "$match": {
                "$and": [
                    { "account_id": account_id },
                    { "$or": [
                        { "merged_profiles.profile_id": identifier },
                        { "merged_profiles.customer_id": identifier },
                        { "parent_profile_id": identifier },
                        { "parent_customer_id": identifier }
                    ] }
                ]
            }
        }));
        pipeline.push(json!({ "$sort": { "created_at": -1 } }));
        pipeline.push(json!({ "$limit": 1 }));
    }

    let results = gateway_ext::aggregate(MERGED_COLLECTION, pipeline).await?;
    let mut merged_profiles = Vec::with_capacity(results.len());
    for res in results {
        let merged_at = res
            .get("created_at")
            .and_then(|c| c.get("$date"))
            .and_then(Value::as_i64)
            .and_then(int_to_dt_str);
        merged_profiles.push(json!({
            "merged_profiles": res.get("merged_profiles").cloned().unwrap_or_else(|| json!([])),
            "parent_profile_id": res.get("parent_profile_id").cloned().unwrap_or(Value::Null),
            "parent_customer_id": res.get("parent_customer_id").cloned().unwrap_or(Value::Null),
            "authenticated_id_key": res.get("authenticated_id_key").cloned().unwrap_or(Value::Null),
            "authenticated_id_value": res.get("authenticated_id_value").cloned().unwrap_or(Value::Null),
            "merged_at": merged_at,
        }));
    }
    Ok(merged_profiles)
}

// ---------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------

/// Port of one iteration of `create_profiles`'s `InsertOne` bulk op.
///
/// NOTE (divergence): the Python service used a single `bulk_write(ordered=
/// false)` and inspected `BulkWriteError.details['writeErrors']` to report
/// exactly which `customer_id`s collided. The gateway's generic
/// `insert-many` endpoint (used by other services) does not surface
/// per-document results on partial failure, so this port issues one
/// `insert_one` per profile instead — more gateway round trips (bounded by
/// `MAX_CREATE_PROFILES`, default 100), but it reproduces the same
/// created/failed split the Python API contract promises.
pub async fn insert_profile(ctx: &Ctx, doc: Value) -> Result<(), OctyError> {
    ctx.gateway.insert_one(COLLECTION, doc).await.map(|_| ())
}

pub async fn find_profiles_by_ids(ctx: &Ctx, ids: &[String]) -> Result<HashMap<String, Value>, OctyError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let docs = ctx.gateway.find(COLLECTION, json!({ "_id": { "$in": ids } }), 0, 0).await?;
    let mut map = HashMap::new();
    for doc in docs {
        if let Some(id) = doc.get("_id").and_then(Value::as_str) {
            map.insert(id.to_string(), doc);
        }
    }
    Ok(map)
}

pub async fn update_profile(ctx: &Ctx, profile_id: &str, account_id: &str, set_fields: Value) -> Result<(), OctyError> {
    ctx.gateway
        .update_one(
            COLLECTION,
            json!({ "_id": profile_id, "account_id": account_id }),
            json!({ "$set": set_fields }),
        )
        .await
}

pub async fn delete_profile(ctx: &Ctx, profile_id: &str, account_id: &str) -> Result<(), OctyError> {
    ctx.gateway
        .delete_one(COLLECTION, json!({ "_id": profile_id, "account_id": account_id }))
        .await
}

/// Port of `update_delete_segment_tags`.
pub async fn update_delete_segment_tags(ctx: &Ctx, account_id: &str, segment_ids: &[String], action: &str) -> Result<(), OctyError> {
    let _ = ctx;
    match action {
        "update" => {
            for seg in segment_ids {
                gateway_ext::update_many(
                    COLLECTION,
                    json!({ "account_id": account_id, "segment_tags.segment_id": seg }),
                    json!({ "$set": { "segment_tags.$.status": "pending_deletion", "segment_tags.$.updated_at": now_legacy_date() } }),
                )
                .await?;
            }
        }
        "delete" => {
            for seg in segment_ids {
                gateway_ext::update_many(
                    COLLECTION,
                    json!({ "account_id": account_id }),
                    json!({ "$pull": { "segment_tags": { "segment_id": seg } } }),
                )
                .await?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Port of `delete_all_profiles`.
pub async fn delete_all_profiles(ctx: &Ctx, account_id: &str) -> Result<bool, OctyError> {
    gateway_ext::delete_many(COLLECTION, json!({ "account_id": account_id })).await?;
    gateway_ext::delete_many(MERGED_COLLECTION, json!({ "account_id": account_id })).await?;
    delete_profile_key_types(ctx, account_id)?;
    Ok(true)
}

/// Port of `create_segment_tags`. `segment_tags` are raw `{segment_id,
/// segment_tag, status}` objects (the grouped-segmentation AMQP payload is
/// untyped, same as the Python `List[dict]`); a missing key mirrors the
/// Python `KeyError` by returning an error the caller treats as "skip this
/// operation" (see `services::profiles::grouped_segmentation_database_operations`).
pub async fn create_segment_tags(ctx: &Ctx, account_id: &str, profile_id: &str, segment_tags: &[Value]) -> Result<(), OctyError> {
    for seg in segment_tags {
        let (Some(segment_id), Some(segment_tag), Some(status)) = (
            seg.get("segment_id").and_then(Value::as_str),
            seg.get("segment_tag").and_then(Value::as_str),
            seg.get("status").and_then(Value::as_str),
        ) else {
            return Err(OctyError::internal("missing segment tag key"));
        };
        let tag_doc = json!({
            "segment_id": segment_id,
            "segment_tag": segment_tag,
            "status": status,
            "created_at": now_legacy_date(),
        });
        ctx.gateway
            .update_one(
                COLLECTION,
                json!({ "_id": profile_id, "account_id": account_id }),
                json!({ "$push": { "segment_tags": tag_doc } }),
            )
            .await?;
    }
    Ok(())
}

/// Port of `update_segment_tags`.
pub async fn update_segment_tags(ctx: &Ctx, account_id: &str, profile_id: &str, segment_tags: &[Value]) -> Result<(), OctyError> {
    ctx.gateway
        .update_one(
            COLLECTION,
            json!({ "_id": profile_id, "account_id": account_id }),
            json!({ "$set": { "segment_tags": segment_tags, "updated_at": now_legacy_date() } }),
        )
        .await
}

/// Port of `delete_segment_tags`.
pub async fn delete_segment_tags(ctx: &Ctx, account_id: &str, profile_id: &str, segment_tags: &[Value]) -> Result<(), OctyError> {
    let mut segment_ids = Vec::with_capacity(segment_tags.len());
    for seg in segment_tags {
        match seg.get("segment_id").and_then(Value::as_str) {
            Some(id) => segment_ids.push(id.to_string()),
            None => return Err(OctyError::internal("missing segment tag key")),
        }
    }
    ctx.gateway
        .update_one(
            COLLECTION,
            json!({ "_id": profile_id, "account_id": account_id }),
            json!({ "$pull": { "segment_tags": { "segment_id": { "$in": segment_ids } } } }),
        )
        .await
}

// ---------------------------------------------------------------------
// Redis: `{account_id}_profile_key_types` set (db index 1)
// ---------------------------------------------------------------------

fn redis_conn(ctx: &Ctx) -> Result<spin_sdk::redis::Connection, OctyError> {
    spin_sdk::redis::Connection::open(&ctx.redis_address(1)?).map_err(|e| OctyError::internal(format!("redis connect failed: {e:?}")))
}

pub fn set_profile_key_type(ctx: &Ctx, account_id: &str, profile_key_type: &Value) -> Result<(), OctyError> {
    let payload = serde_json::to_string(profile_key_type).expect("serializable json");
    redis_conn(ctx)?
        .sadd(&format!("{account_id}_profile_key_types"), &[payload])
        .map_err(|e| OctyError::internal(format!("redis sadd failed: {e:?}")))?;
    Ok(())
}

pub fn get_profile_key_types(ctx: &Ctx, account_id: &str) -> Result<Vec<Value>, OctyError> {
    let members = redis_conn(ctx)?
        .smembers(&format!("{account_id}_profile_key_types"))
        .map_err(|e| OctyError::internal(format!("redis smembers failed: {e:?}")))?;
    Ok(members.into_iter().filter_map(|m| serde_json::from_str(&m).ok()).collect())
}

pub fn delete_profile_key_types(ctx: &Ctx, account_id: &str) -> Result<(), OctyError> {
    redis_conn(ctx)?
        .del(&[format!("{account_id}_profile_key_types")])
        .map_err(|e| OctyError::internal(format!("redis del failed: {e:?}")))?;
    Ok(())
}
