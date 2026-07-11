//! Port of `data/repositories/implementation/templates_repository.py`.
//!
//! `tbl_templates` (mongoengine alias `template_db`) is reached through the
//! data gateway; documents travel as legacy extended JSON. The Python bulk
//! write operations become per-document gateway calls — the unordered-bulk
//! semantics (continue past per-op duplicate-key errors) are preserved by
//! catching the gateway's 409→"Duplicate entry" per operation.

use chrono::{DateTime, TimeZone, Utc};
use octy_shared::ejson::{date_millis, legacy_date};
use octy_shared::errors::{ErrorReason, OctyError};
use serde_json::{json, Map, Value};

use octy_spin::ctx::Ctx;

use crate::http_util::MsgError;

pub const COLLECTION: &str = "tbl_templates";

/// A prepared template document (the dicts `MessagingService` builds).
#[derive(Debug, Clone)]
pub struct TemplateBatchEntry {
    pub template_id: String,
    pub account_id: String,
    pub friendly_name: String,
    pub template_type: String,
    pub title: String,
    pub content: String,
    pub default_values: Map<String, Value>,
    pub metadata: Option<Value>,
}

fn is_duplicate_err(err: &OctyError) -> bool {
    err.code == 400 && err.error_description == "Duplicate entry"
}

/// `'%a, %d %b %Y %H:%M:%S GMT'` — the strftime the Python `int_to_dt(...,
/// as_str=True)` used.
fn gmt_str(dt: DateTime<Utc>) -> String {
    dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

fn format_date_value(v: &Value) -> Value {
    if v.is_null() {
        return Value::Null;
    }
    match date_millis(v) {
        Some(ms) => match Utc.timestamp_millis_opt(ms).single() {
            Some(dt) => Value::String(gmt_str(dt)),
            None => v.clone(),
        },
        None => v.clone(),
    }
}

/// `_format_template` — pops `_id`/`account_id`, formats the timestamps.
fn format_template_doc(mut doc: Value) -> Value {
    if let Some(obj) = doc.as_object_mut() {
        let id = obj.remove("_id");
        obj.remove("account_id");
        if let Some(id) = id {
            obj.insert("template_id".to_string(), id);
        }
        // Python: KeyError on 'created_at' skips date formatting entirely.
        if obj.contains_key("created_at") {
            let created = obj["created_at"].clone();
            obj.insert("created_at".to_string(), format_date_value(&created));
            if let Some(updated) = obj.get("updated_at").cloned() {
                obj.insert("updated_at".to_string(), format_date_value(&updated));
            }
        }
    }
    doc
}

/// `get_all_templates` — every active template for the account.
///
/// The Python used the mongoengine `tbl_templates.objects(...)` QuerySet here
/// (unlike `get_templates`, which drops to raw pymongo), so `t['template_id']`
/// / `t.id` transparently resolve through the primary-key field mapping. The
/// gateway returns raw documents (`_id`), so mirror that mapping by copying
/// `_id` into `template_id` on every doc.
pub async fn get_all_templates(ctx: &Ctx, account_id: &str) -> Result<Vec<Value>, OctyError> {
    let mut docs = ctx
        .gateway
        .find(
            COLLECTION,
            json!({ "account_id": account_id, "status": "active" }),
            0,
            0,
        )
        .await?;
    for doc in &mut docs {
        if let Some(obj) = doc.as_object_mut() {
            if let Some(id) = obj.get("_id").cloned() {
                obj.insert("template_id".to_string(), id);
            }
        }
    }
    Ok(docs)
}

/// `get_template_count`
pub async fn get_template_count(ctx: &Ctx, account_id: &str) -> Result<i64, OctyError> {
    ctx.gateway
        .count(
            COLLECTION,
            json!({ "account_id": account_id, "status": "active" }),
        )
        .await
}

/// `get_templates` — filtered/paginated fetch, formatted for the DTO.
pub async fn get_templates(
    ctx: &Ctx,
    account_id: &str,
    identifiers: Option<&[String]>,
    cursor: i64,
) -> Result<(Vec<Value>, i64), OctyError> {
    let mut query = vec![
        json!({ "account_id": { "$eq": account_id } }),
        json!({ "status": { "$eq": "active" } }),
    ];
    let mut skip = cursor;
    if let Some(ids) = identifiers {
        skip = 0;
        query.push(json!({
            "$or": [
                { "_id": { "$in": ids } },
                { "friendly_name": { "$in": ids } },
                { "template_type": { "$in": ids } }
            ]
        }));
    }
    let filter = json!({ "$and": query });

    let docs = ctx.gateway.find(COLLECTION, filter.clone(), skip, 100).await?;
    let total = ctx.gateway.count(COLLECTION, filter).await?;

    let templates: Vec<Value> = docs.into_iter().map(format_template_doc).collect();
    Ok((templates, total))
}

/// `create_templates` — returns `(created_templates, failed_to_create)`.
pub async fn create_templates(
    ctx: &Ctx,
    batch: &[TemplateBatchEntry],
) -> Result<(Vec<Value>, Vec<Value>), OctyError> {
    let mut invalid: Vec<String> = Vec::new();

    for t in batch {
        let mut doc = json!({
            "_id": t.template_id,
            "account_id": t.account_id,
            "friendly_name": t.friendly_name,
            "template_type": t.template_type,
            "title": t.title,
            "content": t.content,
            "default_values": t.default_values,
            "status": "active",
            "created_at": legacy_date(Utc::now()),
            "updated_at": Value::Null,
        });
        if let Some(m) = &t.metadata {
            doc["metadata"] = m.clone();
        }
        match ctx.gateway.insert_one(COLLECTION, doc).await {
            Ok(_) => {}
            Err(e) if is_duplicate_err(&e) => invalid.push(t.friendly_name.clone()),
            Err(e) => return Err(e),
        }
    }

    let failed_to_create: Vec<Value> = invalid
        .iter()
        .map(|name| {
            json!({
                "friendly_name": name,
                "error_message": format!(
                    "Failed to created new message template. Template(s) with provided friendly_name(s) already exist. : {name}"
                ),
            })
        })
        .collect();

    // `set(names) - set(invalid)`, first batch entry per surviving name.
    let mut seen: Vec<&str> = Vec::new();
    let mut created_templates: Vec<Value> = Vec::new();
    for t in batch {
        if invalid.iter().any(|n| n == &t.friendly_name) || seen.contains(&t.friendly_name.as_str())
        {
            continue;
        }
        seen.push(&t.friendly_name);
        created_templates.push(json!({
            "template_id": t.template_id,
            "friendly_name": t.friendly_name,
            "template_type": t.template_type,
            "title": t.title,
            "content": t.content,
            "default_values": t.default_values,
            "metadata": t.metadata.clone().unwrap_or(Value::Null),
        }));
    }

    Ok((created_templates, failed_to_create))
}

/// `update_templates` — returns `(updated_templates, failed_to_update)`.
///
/// NB (parity): like the Python, existing templates are fetched by
/// `template_id` only and the update filter reuses the *fetched* document's
/// `account_id` — see the security note in the service report.
pub async fn update_templates(
    ctx: &Ctx,
    batch: &[TemplateBatchEntry],
) -> Result<(Vec<Value>, Vec<Value>), MsgError> {
    let mut updated_templates: Vec<Value> = Vec::new();
    let mut failed_to_update: Vec<Value> = Vec::new();

    // duplicate template_id check
    let mut template_ids: Vec<&str> = Vec::new();
    for t in batch {
        let tid = t.template_id.as_str();
        if template_ids.contains(&tid) {
            return Err(MsgError::Octy(OctyError::new(
                400,
                "An error occurred when validating request.",
                vec![ErrorReason::new(
                    format!("Identical template identifers supplied. Found duplicate template_id : {tid}"),
                    ctx.config.opt_str("MESSAGING_EXTENDED_HELP").unwrap_or(""),
                )],
            )));
        }
        template_ids.push(tid);
    }

    let templates = ctx
        .gateway
        .find(COLLECTION, json!({ "_id": { "$in": template_ids } }), 0, 0)
        .await
        .map_err(MsgError::Octy)?;

    if templates.is_empty() {
        for tid in &template_ids {
            failed_to_update.push(json!({
                "template_id": tid,
                "error_message": format!("No template found with template_id : {tid}"),
            }));
        }
        return Ok((updated_templates, failed_to_update));
    }

    for tid in &template_ids {
        let exists = templates.iter().any(|t| t["_id"].as_str() == Some(tid));
        if !exists {
            failed_to_update.push(json!({
                "template_id": tid,
                "error_message": format!("No template exists with provided template_id : {tid}"),
            }));
        }
    }

    let now = Utc::now();
    for t in &templates {
        let Some(b) = batch
            .iter()
            .find(|b| Some(b.template_id.as_str()) == t["_id"].as_str())
        else {
            continue;
        };
        let tid = b.template_id.clone();

        let mut set_doc = json!({
            "_id": tid,
            "friendly_name": b.friendly_name,
            "template_type": b.template_type,
            "title": b.title,
            "content": b.content,
            "default_values": b.default_values,
            "updated_at": legacy_date(now),
        });
        // DictConditional(lambda x: x != None): metadata skipped when None.
        if let Some(m) = &b.metadata {
            set_doc["metadata"] = m.clone();
        }

        let filter = json!({
            "$and": [
                { "_id": { "$eq": t["_id"] } },
                { "account_id": { "$eq": t["account_id"] } }
            ]
        });

        match ctx
            .gateway
            .update_one(COLLECTION, filter, json!({ "$set": set_doc }))
            .await
        {
            Ok(()) => {
                updated_templates.push(json!({
                    "template_id": tid,
                    "friendly_name": b.friendly_name,
                    "template_type": b.template_type,
                    "title": b.title,
                    "content": b.content,
                    "default_values": b.default_values,
                    "metadata": b.metadata.clone().unwrap_or(Value::Null),
                    "created_at": format_date_value(&t["created_at"]),
                    "updated_at": gmt_str(now),
                }));
            }
            Err(e) if is_duplicate_err(&e) => {
                failed_to_update.push(json!({
                    "template_id": tid,
                    "friendly_name": b.friendly_name,
                    "error_message": format!(
                        "Another template exists with provided friendly_name : {}",
                        b.friendly_name
                    ),
                }));
            }
            Err(_) => {
                failed_to_update.push(json!({
                    "template_id": tid,
                    "friendly_name": b.friendly_name,
                    "error_message": format!(
                        "Unknown error occurred when updating template with friendly_name : {}",
                        b.friendly_name
                    ),
                }));
            }
        }
    }

    Ok((updated_templates, failed_to_update))
}

/// `delete_templates` — returns `(deleted_templates, failed_to_delete)`.
pub async fn delete_templates(
    ctx: &Ctx,
    batch: &[(String, String)], // (template_id, account_id)
) -> Result<(Vec<Value>, Vec<Value>), OctyError> {
    let mut deleted_templates: Vec<Value> = Vec::new();
    let mut failed_to_delete: Vec<Value> = Vec::new();

    let template_ids: Vec<&str> = batch.iter().map(|(tid, _)| tid.as_str()).collect();

    let templates = ctx
        .gateway
        .find(COLLECTION, json!({ "_id": { "$in": template_ids } }), 0, 0)
        .await?;

    if templates.is_empty() {
        for (tid, _) in batch {
            failed_to_delete.push(json!({
                "template_id": tid,
                "error_message": format!("No template found with template_id : {tid}"),
            }));
        }
        return Ok((deleted_templates, failed_to_delete));
    }

    for (tid, account_id) in batch {
        let owned = templates.iter().any(|t| {
            t["_id"].as_str() == Some(tid) && t["account_id"].as_str() == Some(account_id)
        });
        if owned {
            deleted_templates.push(json!({ "template_id": tid }));
        } else {
            failed_to_delete.push(json!({
                "template_id": tid,
                "error_message": format!("No template found with template_id : {tid}"),
            }));
        }
        // The Python queued the remove op for every batch entry regardless.
        ctx.gateway
            .delete_one(
                COLLECTION,
                json!({
                    "$and": [
                        { "_id": { "$eq": tid } },
                        { "account_id": { "$eq": account_id } }
                    ]
                }),
            )
            .await?;
    }

    Ok((deleted_templates, failed_to_delete))
}

/// `delete_account_templates` — `tbl_templates.objects(account_id=…).delete()`.
/// The gateway has no delete-many endpoint, so page through and delete one by
/// one (see the capability-gap note in the port report).
pub async fn delete_account_templates(ctx: &Ctx, account_id: &str) -> Result<bool, OctyError> {
    loop {
        let docs = ctx
            .gateway
            .find(COLLECTION, json!({ "account_id": account_id }), 0, 100)
            .await?;
        if docs.is_empty() {
            return Ok(true);
        }
        for doc in docs {
            ctx.gateway
                .delete_one(COLLECTION, json!({ "_id": doc["_id"] }))
                .await?;
        }
    }
}
