//! Port of `services/event_types.py` (`EventTypesService`).
//!
//! Divergence (documented): the Python service methods were synchronous but
//! called the (async) Motor repository without awaiting — every call returned
//! a coroutine, so `len(...)` / arithmetic on the result raised TypeError and
//! all event-type endpoints answered the generic 500. The port implements the
//! obviously intended behaviour (awaiting the repository).

use octy_shared::utils::generate_uid;
use serde_json::{json, Value};

use octy_spin::auth::AuthAccount;
use octy_spin::ctx::Ctx;

use crate::http_util::ApiError;
use crate::models::{CreateEventTypes, DeleteEventTypes};
use crate::repos::event_types as repo;
use crate::services::{account_id_str, account_limits, assess_resource_limit};

fn custom_events_help(ctx: &Ctx) -> String {
    ctx.config
        .opt_str("CUSTOM_EVENTS_EXTENDED_HELP")
        .unwrap_or("")
        .to_string()
}

/// `get_event_types(event_type_ids, cursor)` → `(event_types, total)`.
pub async fn get_event_types(
    ctx: &Ctx,
    account: &AuthAccount,
    event_type_ids: Option<&[String]>,
    cursor: i64,
) -> Result<(Vec<Value>, i64), ApiError> {
    let account_id = account_id_str(account)?;

    if let Some(ids) = event_type_ids {
        if cursor == 0 {
            let event_types = repo::get_event_type_by_ids(ctx, &account_id, ids).await?;
            let count = event_types.len() as i64;
            if count < 1 {
                return Err(ApiError::reason(
                    400,
                    "Invalid event type identifier provided",
                    "No custom event types were found with the provided event_type_id",
                    custom_events_help(ctx),
                ));
            }
            return Ok((event_types, count));
        }
        // Unreachable from the router (cursor is always 0 when ids are
        // supplied); Python returned None here → TypeError → 500.
        return Err(ApiError::internal("get_event_types returned None"));
    }

    let (event_types, total) = repo::get_all_event_types(ctx, &account_id, cursor).await?;
    if event_types.is_empty() {
        return Err(ApiError::reason(
            400,
            "No custom event types found",
            "No custom event types found with the provided query parameters or pagination cursor exhausted",
            custom_events_help(ctx),
        ));
    }
    Ok((event_types, total))
}

/// `create_event_types` → `(created, failed)`.
pub async fn create_event_types(
    ctx: &Ctx,
    account: &AuthAccount,
    event_types: &CreateEventTypes,
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let account_id = account_id_str(account)?;

    // assess allowed limits (resource_key=2 → event types slot)
    let current_count = repo::get_event_types_count(ctx, &account_id).await?;
    let limits = account_limits(account)?;
    let (res, counts) = assess_resource_limit(
        &limits,
        current_count,
        event_types.event_types.len() as i64,
        2,
    )?;
    if !res {
        return Err(ApiError::reason(
            400,
            "Resource limit exceeded",
            format!(
                "This request could not be completed as the number of event types sent with this request exceeds the allowed limit of : {}. This account can create another {} event types.",
                counts.limit, counts.remainder
            ),
            ctx.config.opt_str("RATE_LIMIT_EXTENDED_HELP").unwrap_or(""),
        ));
    }

    let event_type_batch: Vec<Value> = event_types
        .event_types
        .iter()
        .map(|event_type| {
            json!({
                "event_type_id": generate_uid("custom_event_type"),
                "account_id": account_id,
                "event_type": event_type.event_type,
                "event_properties": event_type.event_properties,
            })
        })
        .collect();

    let system_event_types: Vec<String> = ctx
        .config
        .get_array("SYSTEM_EVENT_TYPES")
        .map_err(ApiError::from)?
        .iter()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect();

    let (created, failed) =
        repo::create_event_types(ctx, &event_type_batch, &system_event_types).await?;

    if created.is_empty() {
        return Err(ApiError::octy(400, "No event types created!", failed));
    }
    Ok((created, failed))
}

/// `delete_all_event_types` → `(deleted, failed)`.
pub async fn delete_all_event_types(
    ctx: &Ctx,
    account: &AuthAccount,
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let account_id = account_id_str(account)?;
    let (deleted, failed) = repo::delete_all_event_types_by_account(ctx, &account_id).await?;
    if deleted.is_empty() {
        return Err(ApiError::octy(400, "No event types deleted!", failed));
    }
    Ok((deleted, failed))
}

/// `delete_event_types` → `(deleted, failed)`.
pub async fn delete_event_types(
    ctx: &Ctx,
    account: &AuthAccount,
    event_type_ids: &DeleteEventTypes,
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let account_id = account_id_str(account)?;
    let event_type_id_batch: Vec<Value> = event_type_ids
        .event_type_ids
        .iter()
        .map(|et| json!({ "event_type_id": et, "account_id": account_id }))
        .collect();

    let (deleted, failed) = repo::delete_event_types(ctx, &event_type_id_batch).await?;
    if deleted.is_empty() {
        return Err(ApiError::octy(400, "No event types deleted!", failed));
    }
    Ok((deleted, failed))
}

/// `get_event_types_internal` → `(found, not_found)`.
pub async fn get_event_types_internal(
    ctx: &Ctx,
    account_id: &str,
    event_type_names: &[String],
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let (found_event_types, not_found) =
        repo::get_event_types_by_name(ctx, account_id, event_type_names).await?;
    if found_event_types.is_empty() {
        return Err(ApiError::reason(
            400,
            "None found!",
            "No custom event types were found with the provided event type names",
            "",
        ));
    }
    Ok((found_event_types, not_found))
}
