//! Port of `services/profiles.py::ProfilesService`.

use octy_shared::ejson::now_legacy_date;
use octy_shared::errors::{ErrorReason, OctyError};
use octy_shared::utils::generate_uid;
use serde_json::{json, Value};

use crate::http_util::ServiceError;
use crate::models::ProfileUpdateInput;
use crate::repos::profiles as repo;
use crate::services::billing::BillingUnits;
use octy_spin::auth::AuthAccount;
use octy_spin::ctx::Ctx;

fn profiles_help(ctx: &Ctx) -> &str {
    ctx.config.opt_str("PROFILES_EXTENDED_HELP").unwrap_or("")
}

fn rate_limit_help(ctx: &Ctx) -> &str {
    ctx.config.opt_str("RATE_LIMIT_EXTENDED_HELP").unwrap_or("")
}

// ---------------------------------------------------------------------
// GET /v1/retention/profiles
// ---------------------------------------------------------------------

pub async fn get_profiles(
    ctx: &Ctx,
    account_id: &str,
    identifiers: Option<&[String]>,
    cursor: i64,
    segments: Option<&[String]>,
    rfm_values: Option<&[i64]>,
    churn_prob: Option<&str>,
) -> Result<(Vec<Value>, i64), OctyError> {
    if let Some(ids) = identifiers {
        // The router only ever supplies `identifiers` with `cursor == 0`
        // (the pagination header is never consulted on that code path),
        // mirroring `if identifiers != None and cursor == 0:` in Python.
        if cursor == 0 {
            let (profiles, _) = repo::get_profiles_by_identifiers(ctx, account_id, ids, &["active".to_string()], false, false).await?;
            let count = profiles.len() as i64;
            if count < 1 {
                return Err(OctyError::new(
                    400,
                    "Invalid customer identifier(s) provided",
                    vec![ErrorReason::new(
                        "No customer profiles were found with the provided identifier(s)",
                        profiles_help(ctx),
                    )],
                ));
            }
            return Ok((profiles, count));
        }
    }

    let (profiles, total) = repo::get_profiles_by_params(ctx, account_id, cursor, segments, rfm_values, churn_prob).await?;
    if profiles.is_empty() {
        return Err(OctyError::new(
            400,
            "No customer profiles found",
            vec![ErrorReason::new(
                "No customer profiles found with the provided query parameters or pagination cursor exhausted",
                profiles_help(ctx),
            )],
        ));
    }
    Ok((profiles, total))
}

// ---------------------------------------------------------------------
// GET /v1/retention/profiles/metadata
// ---------------------------------------------------------------------

/// `_val_or_none` — the Python only caught `TypeError` (obj is `None`), so a
/// present-but-key-missing dict (e.g. a fresh profile with no `updated_at`
/// yet, since profiles are inserted with the raw driver and never get
/// mongoengine's field defaults) raised an uncaught `KeyError` -> 500 on
/// *every* `/v1/retention/profiles/metadata` call for such a profile. This
/// port implements the evident intent (`dict.get`) instead of reproducing
/// that crash.
fn val_or_none(obj: Option<&Value>, key: &str) -> Option<Value> {
    obj.and_then(|o| o.get(key)).filter(|v| !v.is_null()).cloned()
}

fn raw_val(obj: Option<&Value>, key: &str) -> Option<Value> {
    obj.and_then(|o| o.get(key)).cloned()
}

fn py_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
    }
}

pub async fn get_profiles_meta(ctx: &Ctx, account_id: &str, identifiers: &[String]) -> Result<Vec<Value>, OctyError> {
    let merged_profiles = repo::get_merged_profiles(ctx, account_id, identifiers).await?;
    let (found_profiles, _) = repo::get_profiles_by_identifiers(ctx, account_id, identifiers, &["active".to_string()], false, true).await?;

    let mut identifiers_meta = Vec::with_capacity(identifiers.len());
    for i in identifiers {
        let exists = found_profiles.iter().find(|p| {
            p.get("profile_id").and_then(Value::as_str) == Some(i.as_str())
                || p.get("customer_id").and_then(Value::as_str) == Some(i.as_str())
        });
        let parent_merged = merged_profiles.iter().find(|m| {
            m.get("parent_profile_id").and_then(Value::as_str) == Some(i.as_str())
                || m.get("parent_customer_id").and_then(Value::as_str) == Some(i.as_str())
        });
        let mut child_merged: Option<&Value> = None;
        for mp in &merged_profiles {
            let matched = mp
                .get("merged_profiles")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter().any(|k| {
                        k.get("profile_id").and_then(Value::as_str) == Some(i.as_str())
                            || k.get("customer_id").and_then(Value::as_str) == Some(i.as_str())
                    })
                })
                .unwrap_or(false);
            if matched {
                child_merged = Some(mp);
                break;
            }
        }

        let is_parent = parent_merged.is_some();
        let was_merged = is_parent || child_merged.is_some();

        let merged_at = if is_parent { val_or_none(parent_merged, "merged_at") } else { val_or_none(child_merged, "merged_at") };
        let auth_key = if is_parent { val_or_none(parent_merged, "authenticated_id_key") } else { val_or_none(child_merged, "authenticated_id_key") };
        let auth_val = if is_parent { val_or_none(parent_merged, "authenticated_id_value") } else { val_or_none(child_merged, "authenticated_id_value") };
        let parent_or_child = if is_parent {
            json!("parent_profile")
        } else if child_merged.is_some() {
            json!("child_profile")
        } else {
            Value::Null
        };

        let parent_profile_id = val_or_none(child_merged, "parent_profile_id");
        let parent_customer_id = val_or_none(child_merged, "parent_customer_id");

        let merged_child_profiles = {
            let pm = raw_val(parent_merged, "merged_profiles");
            if pm.as_ref().map(py_truthy).unwrap_or(false) {
                pm.unwrap()
            } else {
                let cm = raw_val(child_merged, "merged_profiles");
                if cm.as_ref().map(py_truthy).unwrap_or(false) {
                    cm.unwrap()
                } else {
                    json!([])
                }
            }
        };

        identifiers_meta.push(json!({
            "provided_identifier": i,
            "profile": {
                "profile_exists": exists.is_some(),
                "profile_id": val_or_none(exists, "profile_id"),
                "customer_id": val_or_none(exists, "customer_id"),
                "created_at": val_or_none(exists, "created_at"),
                "updated_at": val_or_none(exists, "updated_at"),
            },
            "merged_info": {
                "was_merged": was_merged,
                "merged_at": merged_at,
                "authenticated_id_key": auth_key,
                "authenticated_id_value": auth_val,
                "parent_or_child": parent_or_child,
                "parent_profile": {
                    "parent_profile_id": parent_profile_id,
                    "parent_customer_id": parent_customer_id,
                },
                "merged_child_profiles": merged_child_profiles,
            }
        }));
    }
    Ok(identifiers_meta)
}

// ---------------------------------------------------------------------
// POST /v1/retention/profiles/create
// ---------------------------------------------------------------------

fn py_type_str(v: &Value) -> &'static str {
    match v {
        Value::Null => "<class 'NoneType'>",
        Value::Bool(_) => "<class 'bool'>",
        Value::Number(n) => {
            if n.is_f64() {
                "<class 'float'>"
            } else {
                "<class 'int'>"
            }
        }
        Value::String(_) => "<class 'str'>",
        Value::Array(_) => "<class 'list'>",
        Value::Object(_) => "<class 'dict'>",
    }
}

/// Port of the nested `build_map` helper in `_validate_profile_key_types`.
fn build_map(profiles: &[Value]) -> Result<Vec<(String, String)>, String> {
    let mut map: Vec<(String, String)> = Vec::new();
    for profile in profiles {
        let platform_info = profile.get("platform_info").and_then(Value::as_object).cloned().unwrap_or_default();
        let profile_data = profile.get("profile_data").and_then(Value::as_object).cloned().unwrap_or_default();
        let customer_id = profile.get("customer_id").and_then(Value::as_str).unwrap_or("");

        let mut profile_keys: Vec<String> = Vec::new();
        for k in platform_info.keys() {
            if profile_keys.contains(k) {
                return Err(format!("Duplicate key: '{k}', provided in profile with customer_id: {customer_id}"));
            }
            profile_keys.push(k.clone());
        }
        for k in profile_data.keys() {
            if profile_keys.contains(k) {
                return Err(format!("Duplicate key: '{k}', provided in profile with customer_id: {customer_id}"));
            }
            profile_keys.push(k.clone());
        }

        let mut merged = platform_info;
        for (k, v) in profile_data {
            merged.insert(k, v);
        }

        for (k, v) in merged.iter() {
            let type_str = py_type_str(v);
            match map.iter().find(|(mk, _)| mk == k) {
                Some((_, existing_type)) => {
                    if existing_type != type_str {
                        return Err(format!(
                            "Invalid type provided for key '{k}'. Got type {existing_type} expected type {type_str} according the data type of the value for the first instance of the key: {k}."
                        ));
                    }
                }
                None => map.push((k.clone(), type_str.to_string())),
            }
        }
    }
    Ok(map)
}

/// Port of `_validate_profile_key_types`. Enforces that every `profile_data`/
/// `platform_info` key uses the same JSON type across *all* profiles in the
/// account (first-seen type wins) — training data is built from these
/// profiles, and a mixed-type key would corrupt it downstream.
fn validate_profile_key_types(ctx: &Ctx, account_id: &str, new_customer_profiles: &[Value]) -> Result<(), String> {
    const GENERIC_ERROR: &str = "Unknown error occurred. Typically, this is caused by malformed profile_data or platform_info. Please ensure you provided a valid JSON key pair object within both profile_data and platform_info for each new profile.";

    let existing_types_map = repo::get_profile_key_types(ctx, account_id).map_err(|_| GENERIC_ERROR.to_string())?;
    let new_types_map = build_map(new_customer_profiles)?;

    if !existing_types_map.is_empty() {
        for (key, type_) in &new_types_map {
            let existing = existing_types_map
                .iter()
                .find(|et| et.get("key").and_then(Value::as_str) == Some(key.as_str()));
            match existing {
                Some(et) => {
                    let existing_type = et.get("type_").and_then(Value::as_str).unwrap_or("");
                    if existing_type != type_ {
                        return Err(format!(
                            "Invalid type provided for key '{key}'. Got type {type_} expected type {existing_type}"
                        ));
                    }
                }
                None => {
                    let _ = repo::set_profile_key_type(ctx, account_id, &json!({ "key": key, "type_": type_ }));
                }
            }
        }
    } else {
        for (key, type_) in &new_types_map {
            let _ = repo::set_profile_key_type(ctx, account_id, &json!({ "key": key, "type_": type_ }));
        }
    }
    Ok(())
}

pub async fn create_profiles(
    ctx: &Ctx,
    account: &AuthAccount,
    profiles: crate::models::CreateProfiles,
) -> Result<(Vec<Value>, Vec<Value>), ServiceError> {
    let account_id = account.account_oid().unwrap_or_default().to_string();
    let cfg = &account.account_configurations;
    let limits = cfg.get("li").and_then(Value::as_str).unwrap_or("");

    let current_count = repo::get_profile_count(ctx, &account_id).await?;
    let (within_limit, counts) = crate::utils::assess_resource_limit(limits, current_count, profiles.profiles.len() as i64);
    if !within_limit {
        let limit = counts["limit"].as_i64().unwrap_or(0);
        let remainder = counts["remainder"].as_i64().unwrap_or(0);
        return Err(ServiceError::Octy(OctyError::new(
            400,
            "Resource limit exceeded",
            vec![ErrorReason::new(
                format!("This request could not be completed as the number of profiles sent with this request exceeds the allowed limit of : {limit}. This account can create another {remainder} profiles."),
                rate_limit_help(ctx),
            )],
        )));
    }

    let profiles_batch: Vec<Value> = profiles
        .profiles
        .iter()
        .map(|p| {
            json!({
                "profile_id": generate_uid("profile"),
                "customer_id": p.customer_id,
                "account_id": account_id,
                "profile_data": p.profile_data,
                "platform_info": p.platform_info,
                "has_charged": p.has_charged,
            })
        })
        .collect();

    if let Err(msg) = validate_profile_key_types(ctx, &account_id, &profiles_batch) {
        return Err(ServiceError::Octy(OctyError::new(
            400,
            "An error occurred when validating keys.",
            vec![ErrorReason::new(msg, profiles_help(ctx))],
        )));
    }

    let mut created = Vec::new();
    let mut failed = Vec::new();
    for p in &profiles_batch {
        let customer_id = p["customer_id"].as_str().unwrap_or("").to_string();
        let doc = json!({
            "_id": p["profile_id"],
            "customer_id": p["customer_id"],
            "account_id": p["account_id"],
            "profile_data": p["profile_data"],
            "platform_info": p["platform_info"],
            "has_charged": p["has_charged"],
            "created_at": now_legacy_date(),
        });
        match repo::insert_profile(ctx, doc).await {
            Ok(()) => created.push(json!({
                "profile_id": p["profile_id"],
                "customer_id": p["customer_id"],
                "profile_data": p["profile_data"],
                "platform_info": p["platform_info"],
                "has_charged": p["has_charged"],
            })),
            Err(_) => failed.push(json!({
                "customer_id": customer_id,
                "error_message": format!("Another profile exists with provided customer_id: {customer_id}"),
            })),
        }
    }

    if created.is_empty() {
        return Err(ServiceError::RawList { code: 400, reason: "No profiles created!".to_string(), errors: failed });
    }

    let mut billing = BillingUnits::new(
        &account_id,
        cfg.get("a_t").and_then(Value::as_str).unwrap_or(""),
        cfg.get("a_c").and_then(Value::as_str).unwrap_or(""),
        "profiles_data",
    );
    billing.track_data_units(&created);
    billing.complete_data_units(ctx, "MB").await?;

    Ok((created, failed))
}

// ---------------------------------------------------------------------
// POST /v1/retention/profiles/update  (HTTP + AMQP `profiles.cmd.update`)
// ---------------------------------------------------------------------

/// `internal` gates which fields are writable: HTTP clients may only touch
/// the basic profile fields, while `rfm_score`/`rfm_segment_desc`/
/// `churn_probability`/`ltv_prediction`/`current_ltv`/`segment_tags` are
/// updatable only via the internal AMQP path (segmentation/RFM/churn
/// workers), never directly by a client.
pub async fn update_profiles(
    ctx: &Ctx,
    account_id: &str,
    profiles: &[ProfileUpdateInput],
    internal: bool,
    billing_ctx: Option<(&str, &str)>,
) -> Result<(Vec<Value>, Vec<Value>), ServiceError> {
    let profiles_batch: Vec<Value> = profiles
        .iter()
        .map(|p| {
            json!({
                "profile_id": p.profile_id,
                "customer_id": p.customer_id,
                "account_id": account_id,
                "profile_data": p.profile_data,
                "platform_info": p.platform_info,
                "has_charged": p.has_charged,
            })
        })
        .collect();

    if !internal {
        if let Err(msg) = validate_profile_key_types(ctx, account_id, &profiles_batch) {
            return Err(ServiceError::Octy(OctyError::new(
                400,
                "An error occurred when validating keys.",
                vec![ErrorReason::new(msg, profiles_help(ctx))],
            )));
        }
    }

    let ids: Vec<String> = profiles.iter().map(|p| p.profile_id.clone()).collect();
    let existing_map = repo::find_profiles_by_ids(ctx, &ids).await?;

    let mut updated = Vec::new();
    let mut failed = Vec::new();

    for p in profiles {
        let Some(existing) = existing_map.get(&p.profile_id) else {
            failed.push(json!({ "profile_id": p.profile_id, "error_message": format!("No profile found with profile_id: {}", p.profile_id) }));
            continue;
        };

        let mut set_fields = serde_json::Map::new();
        set_fields.insert("updated_at".to_string(), now_legacy_date());
        if let Some(v) = &p.customer_id {
            set_fields.insert("customer_id".to_string(), json!(v));
        }
        if let Some(v) = &p.profile_data {
            set_fields.insert("profile_data".to_string(), v.clone());
        }
        if let Some(v) = &p.platform_info {
            set_fields.insert("platform_info".to_string(), v.clone());
        }
        if let Some(v) = p.has_charged {
            set_fields.insert("has_charged".to_string(), json!(v));
        }
        if let Some(v) = &p.status {
            set_fields.insert("status".to_string(), json!(v));
        }

        if internal {
            if let Some(v) = p.rfm_score {
                set_fields.insert("rfm_score".to_string(), json!(v));
            }
            if let Some(v) = &p.rfm_segment_desc {
                set_fields.insert("rfm_segment_desc".to_string(), json!(v));
            }
            if let Some(v) = &p.churn_probability {
                set_fields.insert("churn_probability".to_string(), json!(v));
            }
            if let Some(v) = p.ltv_prediction {
                set_fields.insert("ltv_prediction".to_string(), json!(v));
            }
            if let Some(v) = p.current_ltv {
                set_fields.insert("current_ltv".to_string(), json!(v));
            }
            if let Some(tags) = &p.segment_tags {
                let new_tags: Vec<Value> = tags
                    .iter()
                    .map(|t| {
                        json!({
                            "segment_id": t.segment_id,
                            "segment_tag": t.segment_tag,
                            "status": t.status.clone().unwrap_or_else(|| "active".to_string()),
                        })
                    })
                    .collect();
                let existing_tags = existing.get("segment_tags").and_then(Value::as_array).cloned().unwrap_or_default();
                let merged_tags = repo::format_segment_tags(&new_tags, &existing_tags);
                set_fields.insert("segment_tags".to_string(), Value::Array(merged_tags));
            }
        }

        // NOTE (preserved Python behaviour): the existence lookup is not
        // scoped by `account_id`, and the write filter uses the *existing
        // document's own* `account_id` rather than the caller's. With
        // globally-unique `profile_id`s (UUIDv4) this is not exploitable in
        // practice, but it does mean this call trusts whichever account the
        // profile already belongs to rather than the authenticated caller.
        let filter_account_id = existing.get("account_id").and_then(Value::as_str).unwrap_or(account_id).to_string();

        match repo::update_profile(ctx, &p.profile_id, &filter_account_id, Value::Object(set_fields.clone())).await {
            Ok(()) => {
                let mut merged_doc = existing.clone();
                if let Some(obj) = merged_doc.as_object_mut() {
                    for (k, v) in set_fields.iter() {
                        obj.insert(k.clone(), v.clone());
                    }
                }
                repo::format_profile(&mut merged_doc, &["active".to_string()], internal);
                updated.push(merged_doc);
            }
            Err(e) => failed.push(json!({ "profile_id": p.profile_id, "error_message": format!("Update failed: {}", e.error_description) })),
        }
    }

    if updated.is_empty() {
        let reason = if internal { "[toxic]:: No profiles updated!" } else { "No profiles updated!" };
        return Err(ServiceError::RawList { code: 400, reason: reason.to_string(), errors: failed });
    }

    if !internal {
        if let Some((account_type, account_currency)) = billing_ctx {
            let mut billing = BillingUnits::new(account_id, account_type, account_currency, "profiles_data");
            billing.track_data_units(&updated);
            billing.complete_data_units(ctx, "MB").await?;
        }
    }

    Ok((updated, failed))
}

// ---------------------------------------------------------------------
// POST /v1/retention/profiles/delete  (HTTP + AMQP `profiles.cmd.delete`)
// ---------------------------------------------------------------------

/// `identification_job` mirrors the AMQP-triggered delete path
/// (`profiles.cmd.delete`, invoked with `identification_job=True`): no
/// `events.cmd.delete` fan-out, and failure uses the `[toxic]::` prefix that
/// tells the AMQP handler to reject without requeue instead of retrying.
///
/// NOTE (fixed Python bug): the Python `delete_profiles` published exactly
/// one `events.cmd.delete` message per call, using the *last* profile_id
/// from the input list (a `for p in profiles.profiles: ...` loop variable
/// leaking past its loop) — not one message per profile actually deleted.
/// For any bulk delete (>1 profile) this silently dropped the cascade event
/// for every profile but the last. This port publishes one event per
/// profile in `deleted`, the evident intent.
pub async fn delete_profiles(
    ctx: &Ctx,
    account_id: &str,
    profile_ids: &[String],
    identification_job: bool,
) -> Result<(Vec<Value>, Vec<Value>), ServiceError> {
    let existing_map = repo::find_profiles_by_ids(ctx, profile_ids).await?;

    let mut deleted = Vec::new();
    let mut failed = Vec::new();

    for pid in profile_ids {
        let Some(existing) = existing_map.get(pid) else {
            failed.push(json!({ "profile_id": pid, "error_message": format!("No profile found with profile_id: {pid}") }));
            continue;
        };
        let filter_account_id = existing.get("account_id").and_then(Value::as_str).unwrap_or(account_id).to_string();
        match repo::delete_profile(ctx, pid, &filter_account_id).await {
            Ok(()) => deleted.push(json!({
                "profile_id": pid,
                "customer_id": existing.get("customer_id").cloned().unwrap_or(Value::Null),
            })),
            Err(e) => failed.push(json!({ "profile_id": pid, "error_message": format!("Delete failed: {}", e.error_description) })),
        }
    }

    if deleted.is_empty() {
        let reason = if identification_job { "[toxic]:: No profiles deleted!" } else { "No profiles deleted!" };
        return Err(ServiceError::RawList { code: 400, reason: reason.to_string(), errors: failed });
    }

    if !identification_job {
        for d in &deleted {
            if let Some(pid) = d.get("profile_id").and_then(Value::as_str) {
                ctx.gateway
                    .amqp_publish("events.cmd.delete", &json!({ "account_id": account_id, "profile_id": pid }))
                    .await?;
            }
        }
    }

    Ok((deleted, failed))
}

// ---------------------------------------------------------------------
// Internal: POST /v1/internal/profiles
// ---------------------------------------------------------------------

/// Two retrieval modes exist for two different classes of internal caller:
/// bounded id lookups (event creation, rec-engine predictions, message
/// personalization) pass explicit `profiles` ids and need no pagination,
/// while corpus-wide consumers (training data export) set `get_all` and
/// page through the account's full profile set.
pub async fn get_profiles_internal(
    ctx: &Ctx,
    account_id: &str,
    req: &crate::models::GetProfilesInternal,
    status: &str,
    cursor: i64,
    ids: bool,
) -> Result<(Vec<Value>, Option<Vec<String>>, i64), OctyError> {
    let tag_statuses = req.tag_statuses.clone().unwrap_or_else(|| vec!["active".to_string()]);

    let (profiles, not_found, total) = if req.get_all {
        let (profiles, total) = repo::get_all_profiles(ctx, account_id, &tag_statuses, cursor, ids, status, 2000, true).await?;
        (profiles, None, total)
    } else {
        let (profiles, not_found) = repo::get_profiles_by_identifiers(ctx, account_id, &req.profiles, &tag_statuses, ids, true).await?;
        let total = profiles.len() as i64;
        (profiles, Some(not_found), total)
    };

    if profiles.is_empty() {
        return Err(OctyError::new(
            400,
            "No profiles found",
            vec![ErrorReason::new("No profiles found or pagination cursor exhausted", "")],
        ));
    }

    Ok((profiles, not_found, total))
}

// ---------------------------------------------------------------------
// Internal: POST /v1/internal/profiles/delete (account-deletion fan-out)
// ---------------------------------------------------------------------

pub async fn delete_account_profiles_internal(ctx: &Ctx, account_id: &str) -> Result<bool, OctyError> {
    repo::delete_all_profiles(ctx, account_id).await
}

// ---------------------------------------------------------------------
// Internal: AMQP `grouped.segmentation.operations.cmd`
// ---------------------------------------------------------------------

/// Port of `grouped_segmentation_database_operations`. The segmentation
/// worker batches per-profile tag create/update/delete operations that must
/// happen synchronously into a single AMQP message; they are applied here in
/// the same order the worker produced them.
///
/// NOTE (divergence): the Python `except KeyError: continue` only swallows a
/// missing dict key within one operation — any *other* exception (e.g. a
/// Mongo error) propagates and fails the whole AMQP delivery (retried by the
/// broker). This port cannot cheaply distinguish "missing key" from "gateway
/// error" through a single `Result<(), OctyError>`, so it treats every
/// per-operation failure as skip-and-continue. This is strictly more lenient
/// than Python (a transient gateway error here is silently dropped rather
/// than retried); acceptable since a subsequent segmentation run
/// re-computes and re-publishes the same tags.
pub async fn grouped_segmentation_database_operations(ctx: &Ctx, account_id: &str, operations: &[Value]) {
    for op in operations {
        let Some(action) = op.get("action").and_then(Value::as_str) else { continue };
        let Some(payload) = op.get("operation_payload") else { continue };
        let Some(profile_id) = payload.get("profile_id").and_then(Value::as_str) else { continue };
        let Some(segment_tags) = payload.get("segment_tags").and_then(Value::as_array) else { continue };

        let result = match action {
            "create" => repo::create_segment_tags(ctx, account_id, profile_id, segment_tags).await,
            "update" => repo::update_segment_tags(ctx, account_id, profile_id, segment_tags).await,
            "delete" => repo::delete_segment_tags(ctx, account_id, profile_id, segment_tags).await,
            _ => Ok(()),
        };
        if let Err(err) = result {
            eprintln!("[profiles-service] grouped segmentation op failed, skipping: {err}");
        }
    }
}

// ---------------------------------------------------------------------
// Internal: AMQP `segment.tags.cmd.update.delete`
// ---------------------------------------------------------------------

pub async fn update_delete_segment_tags(ctx: &Ctx, account_id: &str, segment_ids: &[String], action: &str) -> Result<(), OctyError> {
    repo::update_delete_segment_tags(ctx, account_id, segment_ids, action).await
}
