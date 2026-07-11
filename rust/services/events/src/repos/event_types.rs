//! Port of `data/repositories/implementation/event_types_repository.py`.

use chrono::Utc;
use octy_shared::ejson::date_millis;
use octy_shared::utils::int_to_dt;
use serde_json::{json, Value};

use octy_spin::ctx::Ctx;

use crate::gateway_ext::GatewayExt;
use crate::http_util::{ApiError, GMT_FMT};

const COLLECTION: &str = "tbl_custom_event_types";

/// `_format_event_type` — datetimes (legacy `{"$date": millis}` over the
/// gateway) become GMT strings; anything else (including the plain-int
/// `created_at` that `create_event_types` stores) becomes `null`, exactly
/// like the Python `else: … = None` branch.
fn format_event_type(doc: &mut Value) {
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

pub async fn get_event_types_count(ctx: &Ctx, account_id: &str) -> Result<i64, ApiError> {
    ctx.gateway
        .count(COLLECTION, json!({ "account_id": account_id }))
        .await
        .map_err(ApiError::from)
}

/// Python quirk kept: the filter queries an `event_type_id` *field*, but the
/// documents store that identifier as `_id` (mongoengine primary key), so the
/// lookup matches nothing on documents written by this codebase.
pub async fn get_event_type_by_ids(
    ctx: &Ctx,
    account_id: &str,
    event_type_ids: &[String],
) -> Result<Vec<Value>, ApiError> {
    let query = json!({
        "$and": [
            { "event_type_id": { "$in": event_type_ids } },
            { "account_id": account_id },
        ]
    });
    let mut docs = ctx
        .gateway
        .find(COLLECTION, query, 0, 0)
        .await
        .map_err(ApiError::from)?;
    for doc in &mut docs {
        let id = doc["_id"].clone();
        doc["event_type_id"] = id;
        format_event_type(doc);
    }
    Ok(docs)
}

pub async fn get_event_type_by_name(
    ctx: &Ctx,
    account_id: &str,
    event_type: &str,
) -> Result<Option<Value>, ApiError> {
    let doc = ctx
        .gateway
        .find_one(COLLECTION, json!({ "account_id": account_id, "event_type": event_type }))
        .await
        .map_err(ApiError::from)?;
    Ok(doc.map(|mut doc| {
        let id = doc["_id"].clone();
        doc["event_type_id"] = id;
        format_event_type(&mut doc);
        doc
    }))
}

/// Python quirk kept: the `_id` lands on a `profile_id` key (copy/paste bug
/// in the source repository).
pub async fn get_event_types_by_name(
    ctx: &Ctx,
    account_id: &str,
    event_type_names: &[String],
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let mut docs = ctx
        .gateway
        .find(
            COLLECTION,
            json!({ "account_id": account_id, "event_type": { "$in": event_type_names } }),
            0,
            0,
        )
        .await
        .map_err(ApiError::from)?;

    for doc in &mut docs {
        let id = doc["_id"].clone();
        doc["profile_id"] = id;
        format_event_type(doc);
    }

    let not_found: Vec<Value> = event_type_names
        .iter()
        .filter(|name| {
            !docs
                .iter()
                .any(|et| et.get("event_type").and_then(Value::as_str) == Some(name.as_str()))
        })
        .map(|name| json!(name))
        .collect();

    Ok((docs, not_found))
}

pub async fn get_all_event_types(
    ctx: &Ctx,
    account_id: &str,
    cursor: i64,
) -> Result<(Vec<Value>, i64), ApiError> {
    let mut docs = ctx
        .gateway
        .find(COLLECTION, json!({ "account_id": account_id }), cursor, 100)
        .await
        .map_err(ApiError::from)?;
    let total = ctx
        .gateway
        .count(COLLECTION, json!({ "account_id": account_id }))
        .await
        .map_err(ApiError::from)?;
    for doc in &mut docs {
        let id = doc["_id"].clone();
        doc["event_type_id"] = id;
        format_event_type(doc);
    }
    Ok((docs, total))
}

/// `create_event_types(event_type_batch)` → `(created, failed_to_create)`.
///
/// Faithful quirks: `created_at` is stored as a plain epoch-millis int (not a
/// BSON datetime), and *any* insert failure other than per-op write errors is
/// swallowed (the Python `except Exception` only inspected BulkWriteError
/// details and re-raised nothing).
pub async fn create_event_types(
    ctx: &Ctx,
    event_types: &[Value],
    system_event_types: &[String],
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let _ = ctx;
    let mut failed_to_create: Vec<Value> = Vec::new();
    let mut valid_docs: Vec<Value> = Vec::new();
    let mut valid_names: Vec<String> = Vec::new();

    for et in event_types {
        let name = et["event_type"].as_str().unwrap_or_default().to_string();
        if system_event_types.iter().any(|s| s == &name) {
            failed_to_create.push(json!({
                "event_type": name,
                "error_message": format!("System event type exists: {name}"),
            }));
            continue;
        }
        valid_docs.push(json!({
            "_id": et["event_type_id"],
            "account_id": et["account_id"],
            "event_type": et["event_type"],
            "event_properties": et["event_properties"],
            "created_at": Utc::now().timestamp_millis(),
        }));
        valid_names.push(name);
    }

    if !valid_docs.is_empty() {
        match GatewayExt::load().insert_many(COLLECTION, valid_docs, false).await {
            Ok(write_errors) => {
                for err in write_errors {
                    // `err['op']['event_type']` in the Python BulkWriteError
                    // details — resolved via the op index here.
                    let Some(idx) = err.get("index").and_then(Value::as_u64) else { continue };
                    let Some(conflict_type) = valid_names.get(idx as usize) else { continue };
                    failed_to_create.push(json!({
                        "event_type": conflict_type,
                        "error_message": format!("Conflict: {conflict_type}"),
                    }));
                }
            }
            Err(e) => {
                // Swallowed in Python (bare `except Exception` with no re-raise).
                eprintln!("[events-service] insert_many failed (swallowed like the Python): {e}");
            }
        }
    }

    let created: Vec<Value> = event_types
        .iter()
        .filter(|et| {
            !failed_to_create
                .iter()
                .any(|f| f["event_type"] == et["event_type"])
        })
        .map(|et| {
            let mut copy = et.clone();
            if let Some(obj) = copy.as_object_mut() {
                obj.remove("account_id");
            }
            copy
        })
        .collect();

    Ok((created, failed_to_create))
}

/// `delete_event_types(event_types_batch)` → `(deleted, failed_to_delete)`.
pub async fn delete_event_types(
    ctx: &Ctx,
    event_types_batch: &[Value],
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let _ = ctx;
    let ext = GatewayExt::load();
    let mut deleted_event_types = Vec::new();
    let mut failed_to_delete = Vec::new();

    for et in event_types_batch {
        let deleted = ext
            .delete_one_counted(
                COLLECTION,
                json!({ "_id": et["event_type_id"], "account_id": et["account_id"] }),
            )
            .await
            .map_err(ApiError::from)?;
        if deleted > 0 {
            deleted_event_types.push(json!({ "event_type_id": et["event_type_id"] }));
        } else {
            failed_to_delete.push(json!({
                "event_type_id": et["event_type_id"],
                "error_message": "No match found for deletion",
            }));
        }
    }
    Ok((deleted_event_types, failed_to_delete))
}

/// `delete_all_event_types_by_account` → `(deleted, failed)`.
pub async fn delete_all_event_types_by_account(
    ctx: &Ctx,
    account_id: &str,
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let docs = ctx
        .gateway
        .find(COLLECTION, json!({ "account_id": account_id }), 0, 0)
        .await
        .map_err(ApiError::from)?;

    if docs.is_empty() {
        return Ok((
            vec![],
            vec![json!({
                "account_id": account_id,
                "error_message": format!("No event types found for account_id: {account_id}"),
            })],
        ));
    }

    let deleted: Vec<Value> = docs
        .iter()
        .map(|doc| {
            json!({
                "event_type_id": doc["_id"],
                "event_type": doc.get("event_type").and_then(Value::as_str).unwrap_or("unknown"),
            })
        })
        .collect();

    match GatewayExt::load()
        .delete_many(COLLECTION, json!({ "account_id": account_id }))
        .await
    {
        Ok(_) => Ok((deleted, vec![])),
        Err(e) => Ok((
            vec![],
            vec![json!({
                "account_id": account_id,
                "error_message": format!("Failed to delete: {e}"),
            })],
        )),
    }
}

/// `delete_account_event_types`.
pub async fn delete_account_event_types(account_id: &str) -> Result<bool, ApiError> {
    let deleted = GatewayExt::load()
        .delete_many(COLLECTION, json!({ "account_id": account_id }))
        .await
        .map_err(ApiError::from)?;
    Ok(deleted > 0)
}
