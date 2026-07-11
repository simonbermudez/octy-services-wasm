//! Port of `data/repositories/implementation/items_repository.py`.
//!
//! MongoDB goes through the data gateway. The Python repository used the
//! legacy pymongo *unordered bulk* API; the gateway only exposes single-doc
//! operations, so bulk inserts/updates/removes are issued per-document with
//! the same success/failure bookkeeping (the gateway surfaces duplicate-key
//! violations as 409 → `OctyError` "Duplicate entry").

use chrono::{TimeZone, Utc};
use octy_shared::ejson;
use octy_shared::errors::{ErrorReason, OctyError};
use octy_spin::auth::AuthAccount;
use octy_spin::ctx::Ctx;
use serde_json::{json, Value};
use spin_sdk::http::Method;

use crate::errors::ApiError;

const COLLECTION: &str = "tbl_items";

/// `utils.int_to_dt(ms, as_str=True)` — `'%a, %d %b %Y %H:%M:%S GMT'`.
/// (The Python used `dt.fromtimestamp`, i.e. server-local time; the services
/// run in UTC so this renders UTC.)
pub fn fmt_gmt(ms: i64) -> String {
    match Utc.timestamp_millis_opt(ms).single() {
        Some(dt) => dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
        None => String::new(),
    }
}

/// Port of `_format_item`, including its bare-`except` early returns:
/// a missing `created_at`/`updated_at` key or a non-`{"$date": …}` value
/// leaves the item as-is (minus the popped keys).
pub fn format_item(item: &mut Value) {
    let Some(obj) = item.as_object_mut() else { return };
    obj.remove("_id");
    obj.remove("account_id");
    obj.remove("event_type");

    // created_at (KeyError → return item)
    let Some(created) = obj.get("created_at").cloned() else { return };
    if created.is_null() {
        // stays null
    } else if let Some(ms) = ejson::date_millis(&created) {
        obj.insert("created_at".to_string(), Value::String(fmt_gmt(ms)));
    } else {
        return; // TypeError → bare except → return item
    }

    // updated_at (KeyError → return item; TypeError branch handled formatted
    // strings from the update flow)
    let Some(updated) = obj.get("updated_at").cloned() else { return };
    if updated.is_null() || updated.is_string() {
        // stays null / already formatted (Python's strftime fallback)
    } else if let Some(ms) = ejson::date_millis(&updated) {
        obj.insert("updated_at".to_string(), Value::String(fmt_gmt(ms)));
    }
}

pub async fn get_item_count(ctx: &Ctx, account_id: &Value) -> Result<i64, OctyError> {
    ctx.gateway
        .count(COLLECTION, json!({ "account_id": account_id }))
        .await
}

pub async fn get_item_by_ids(
    ctx: &Ctx,
    item_ids: &[String],
    account_id: &Value,
) -> Result<Vec<Value>, OctyError> {
    let mut items = ctx
        .gateway
        .find(
            COLLECTION,
            json!({ "item_id": { "$in": item_ids }, "account_id": account_id }),
            0,
            0, // no limit (mongoengine query was unbounded)
        )
        .await?;
    for item in &mut items {
        format_item(item);
    }
    Ok(items)
}

pub async fn get_items(
    ctx: &Ctx,
    account_id: &Value,
    cursor: i64,
    ids: bool,
    status: &str,
) -> Result<(Vec<Value>, i64), OctyError> {
    let filter = if status == "all" {
        json!({ "account_id": account_id })
    } else {
        json!({ "account_id": account_id, "status": status })
    };
    let limit = if ids { 200 } else { 100 };

    let mut items = ctx
        .gateway
        .find(COLLECTION, filter.clone(), cursor, limit)
        .await?;

    let mut total = 0;
    if !items.is_empty() {
        total += ctx.gateway.count(COLLECTION, filter).await?;
    }

    if ids {
        // `.only('item_id')` + `_format_item`'s KeyError early-return left
        // exactly `{"item_id": …}` per document.
        items = items
            .iter()
            .map(|d| json!({ "item_id": d.get("item_id").cloned().unwrap_or(Value::Null) }))
            .collect();
    } else {
        for item in &mut items {
            format_item(item);
        }
    }
    Ok((items, total))
}

/// Port of `create_items` — unordered bulk insert emulated per-document.
/// Returns `(created_items, failed_to_create)`.
pub async fn create_items(
    ctx: &Ctx,
    items_batch: &[Value],
) -> Result<(Vec<Value>, Vec<Value>), OctyError> {
    let mut item_ids: Vec<String> = Vec::new();
    let mut invalid: Vec<String> = Vec::new();

    for item in items_batch {
        let id = item["item_id"].as_str().unwrap_or_default().to_string();
        item_ids.push(id.clone());

        // tbl_items schema defaults: status='active', created_at=now,
        // updated_at null (DateTimeField(null=True)).
        let document = json!({
            "item_id": item["item_id"],
            "account_id": item["account_id"],
            "item_category": item["item_category"],
            "item_name": item["item_name"],
            "item_description": item["item_description"],
            "item_price": item["item_price"],
            "event_type": item["event_type"],
            "status": "active",
            "created_at": ejson::now_legacy_date(),
            "updated_at": Value::Null,
        });

        match ctx.gateway.insert_one(COLLECTION, document).await {
            Ok(_) => {}
            // duplicate key (unique item_id+account_id) → collected like a
            // pymongo BulkWriteError writeError
            Err(err) if err.error_description == "Duplicate entry" => invalid.push(id),
            Err(err) => return Err(err),
        }
    }

    let failed_to_create: Vec<Value> = invalid
        .iter()
        .map(|id| {
            json!({
                "item_id": id,
                "error_message": format!("Another item exists with provided item_id : {id}"),
            })
        })
        .collect();

    // Python: `valid = list(set(item_ids) - set(invalid))` — set order is
    // arbitrary there; we keep deterministic first-occurrence order.
    let mut created_items: Vec<Value> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for id in &item_ids {
        if invalid.iter().any(|inv| inv == id) || !seen.insert(id.as_str()) {
            continue;
        }
        if let Some(item) = items_batch
            .iter()
            .find(|d| d["item_id"].as_str() == Some(id.as_str()))
        {
            let mut out = item.clone();
            if let Some(obj) = out.as_object_mut() {
                obj.remove("account_id");
                obj.remove("event_type");
            }
            created_items.push(out);
        }
    }

    Ok((created_items, failed_to_create))
}

/// Port of `update_items`. Returns `(updated_items, failed_to_update)`.
pub async fn update_items(
    ctx: &Ctx,
    items_batch: &[Value],
    account_id: &Value,
    items_help: &str,
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let mut updated_items: Vec<Value> = Vec::new();
    let mut failed_to_update: Vec<Value> = Vec::new();
    let mut item_ids: Vec<String> = Vec::new();

    // determine valid items (reject duplicate identifiers)
    for item in items_batch {
        let id = item["item_id"].as_str().unwrap_or_default().to_string();
        if item_ids.contains(&id) {
            return Err(OctyError::new(
                400,
                "An error occurred when validating request.",
                vec![ErrorReason::new(
                    format!("Identical item identifers supplied. Found duplicate item_id : {id}"),
                    items_help,
                )],
            )
            .into());
        }
        item_ids.push(id);
    }

    let items = ctx
        .gateway
        .find(
            COLLECTION,
            json!({ "item_id": { "$in": item_ids }, "account_id": account_id }),
            0,
            0,
        )
        .await
        .map_err(ApiError::from)?;

    if items.is_empty() {
        for item in items_batch {
            let id = item["item_id"].as_str().unwrap_or_default();
            failed_to_update.push(json!({
                "item_id": item["item_id"],
                "error_message": format!("No item found with item_id : {id}"),
            }));
        }
        return Ok((updated_items, failed_to_update));
    }

    for itd in &item_ids {
        let exists = items
            .iter()
            .any(|key| key["item_id"].as_str() == Some(itd.as_str()));
        if !exists {
            failed_to_update.push(json!({
                "item_id": itd,
                "error_message": format!("No item exists with provided item_id : {itd}"),
            }));
        }
    }

    let now = Utc::now();
    let now_str = fmt_gmt(now.timestamp_millis());

    for i in &items {
        let item_batch_obj = items_batch
            .iter()
            .find(|key| key["item_id"] == i["item_id"])
            .ok_or_else(|| {
                ApiError::from(OctyError::internal("StopIteration: item not found in batch"))
            })?;

        // Python assigned `i['created_at']` before formatting; a missing key
        // was a KeyError → 500.
        let created_at_raw = i
            .get("created_at")
            .cloned()
            .ok_or_else(|| ApiError::from(OctyError::internal("KeyError: 'created_at'")))?;

        let set_dict = json!({
            "item_id": i["item_id"],
            "item_category": item_batch_obj["item_category"],
            "item_name": item_batch_obj["item_name"],
            "item_description": item_batch_obj["item_description"],
            "item_price": item_batch_obj["item_price"],
            "event_type": item_batch_obj["event_type"],
            "status": item_batch_obj["status"],
            "updated_at": ejson::legacy_date(now),
        });

        let filter = json!({
            "$and": [
                { "item_id": { "$eq": i["item_id"] } },
                { "account_id": { "$eq": i["account_id"] } },
            ]
        });

        match ctx
            .gateway
            .update_one(COLLECTION, filter, json!({ "$set": set_dict }))
            .await
        {
            Ok(()) => {
                // append updated item to return array (batch obj + created_at
                // from the stored document + updated_at, then `_format_item`)
                let mut out = item_batch_obj.clone();
                if let Some(obj) = out.as_object_mut() {
                    obj.insert("created_at".to_string(), created_at_raw);
                    obj.insert("updated_at".to_string(), Value::String(now_str.clone()));
                }
                format_item(&mut out);
                updated_items.push(out);
            }
            // pymongo writeError code 11000 branch
            Err(err) if err.error_description == "Duplicate entry" => {
                let id = i["item_id"].as_str().unwrap_or_default();
                failed_to_update.push(json!({
                    "item_id": i["item_id"],
                    "error_message": format!("Another item exists with provided item_id : {id}"),
                }));
            }
            Err(err) => return Err(err.into()),
        }
    }

    Ok((updated_items, failed_to_update))
}

/// Port of `delete_items`. Returns `(deleted_items, failed_to_delete)`.
pub async fn delete_items(
    ctx: &Ctx,
    items_batch: &[Value],
    account: &AuthAccount,
) -> Result<(Vec<Value>, Vec<Value>), OctyError> {
    let mut deleted_items: Vec<Value> = Vec::new();
    let mut failed_to_delete: Vec<Value> = Vec::new();

    let item_ids: Vec<Value> = items_batch.iter().map(|i| i["item_id"].clone()).collect();

    let items = ctx
        .gateway
        .find(
            COLLECTION,
            json!({ "item_id": { "$in": item_ids }, "account_id": account.account_id }),
            0,
            0,
        )
        .await?;

    if items.is_empty() {
        for item in items_batch {
            let id = item["item_id"].as_str().unwrap_or_default();
            failed_to_delete.push(json!({
                "item_id": item["item_id"],
                "error_message": format!("No item found with item_id : {id}"),
            }));
        }
        return Ok((deleted_items, failed_to_delete));
    }

    for item in items_batch {
        let item_batch_object = items.iter().find(|key| {
            key["item_id"] == item["item_id"] && key["account_id"] == item["account_id"]
        });
        match item_batch_object {
            Some(found) => deleted_items.push(json!({ "item_id": found["item_id"] })),
            None => {
                let id = item["item_id"].as_str().unwrap_or_default();
                failed_to_delete.push(json!({
                    "item_id": item["item_id"],
                    "error_message": format!("No item found with item_id : {id}"),
                }));
            }
        }

        // The Python bulk op queued a remove for every batch entry (a no-op
        // when nothing matches).
        ctx.gateway
            .delete_one(
                COLLECTION,
                json!({
                    "$and": [
                        { "item_id": { "$eq": item["item_id"] } },
                        { "account_id": { "$eq": item["account_id"] } },
                    ]
                }),
            )
            .await?;
    }

    // update item_id_stop_list in account configurations
    let rec_configs = account
        .algorithm_configurations
        .as_array()
        .and_then(|arr| arr.iter().find(|key| key["algorithm_name"] == "rec"))
        .cloned();

    if let Some(mut rec_configs) = rec_configs {
        // The Python unconditionally assigned into rec_configs['config_json']
        // at the end — missing/non-dict config_json was a KeyError/TypeError
        // (→ 500).
        if !rec_configs
            .get("config_json")
            .map(Value::is_object)
            .unwrap_or(false)
        {
            return Err(OctyError::internal(
                "rec algorithm 'config_json' missing or not an object (KeyError in Python)",
            ));
        }

        let item_id_stop_list: Vec<Value> = match rec_configs["config_json"].get("item_id_stop_list")
        {
            None => Vec::new(), // KeyError → []
            Some(Value::Array(list)) => list.clone(),
            Some(_) => {
                return Err(OctyError::internal(
                    "item_id_stop_list is not a list (TypeError in Python)",
                ))
            }
        };

        let mut augmented_stop_list = item_id_stop_list.clone();
        for item in &deleted_items {
            for sl_item in &item_id_stop_list {
                if sl_item["item_id"] == item["item_id"] {
                    let index = augmented_stop_list
                        .iter()
                        .position(|d| d["item_id"] == sl_item["item_id"]);
                    match index {
                        Some(index) => {
                            augmented_stop_list.remove(index);
                        }
                        // Python: `del augmented_stop_list[None]` → TypeError
                        None => {
                            return Err(OctyError::internal(
                                "stop-list entry already removed (TypeError: del list[None] in Python)",
                            ))
                        }
                    }
                }
            }
        }

        rec_configs["config_json"]["item_id_stop_list"] = Value::Array(augmented_stop_list);
        eprintln!("{}", rec_configs["config_json"]); // Python: print(rec_configs['config_json'])

        ctx.gateway
            .amqp_publish(
                "algo.configs.cmd.update",
                &json!({
                    "account_id": account.account_id,
                    "algorithm_configurations": {
                        "algorithm_name": "rec",
                        "config_json": rec_configs["config_json"],
                    }
                }),
            )
            .await?;
    }

    Ok((deleted_items, failed_to_delete))
}

/// Port of `delete_account_items_internal` —
/// `tbl_items.objects(account_id__exact=account_id).delete()` is a
/// *delete-many*, which the gateway does not expose yet. This calls the
/// hypothetical `POST /v1/mongo/{collection}/delete-many` endpoint
/// (`{"filter": …}` → `{"deleted": n}`); see the capability-gap note in the
/// service README/report.
pub async fn delete_account_items_internal(account_id: &str) -> Result<bool, OctyError> {
    let base = octy_spin::ctx::variable("gateway_url", "GATEWAY_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string());
    let url = format!(
        "{}/v1/mongo/{COLLECTION}/delete-many",
        base.trim_end_matches('/')
    );
    let body = serde_json::to_vec(&json!({ "filter": { "account_id": account_id } }))
        .expect("serializable json");

    let (status, response) = octy_spin::gateway::http_send(
        Method::Post,
        &url,
        &[("content-type", "application/json")],
        Some(body),
    )
    .await?;

    if !(200..300).contains(&status) {
        return Err(OctyError::internal(format!(
            "gateway delete-many returned {status}: {}",
            String::from_utf8_lossy(&response)
        )));
    }
    Ok(true) // the Python returned True unconditionally
}
