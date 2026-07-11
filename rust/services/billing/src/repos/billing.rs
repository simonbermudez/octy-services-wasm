//! Port of `data/repositories/implementation/billing_repository.py`.
//!
//! MongoDB access goes through the data gateway. Two operations the Python
//! repository used have no generic `GatewayClient` method yet, so they are
//! called against sensibly-shaped hypothetical gateway endpoints (see the
//! final porting report):
//!
//! * bulk unordered insert (`initialize_unordered_bulk_op` + `insert`) →
//!   `POST /v1/mongo/{collection}/insert-many`
//!   body `{"documents": [...], "ordered": false}`
//! * multi-document delete (`objects(account_id=...).delete()`) →
//!   `POST /v1/mongo/{collection}/delete-many`
//!   body `{"filter": {...}}` → `{"deleted_count": n}`

use chrono::{DateTime, Utc};
use octy_shared::ejson::{date_millis, legacy_date, now_legacy_date};
use octy_shared::errors::OctyError;
use octy_shared::utils::int_to_dt;
use serde_json::{json, Value};
use spin_sdk::http::Method;

use octy_spin::ctx::Ctx;
use octy_spin::gateway::http_send;

const COLLECTION: &str = "tbl_billable_units";

/// Page size hard-coded in the Python repository (`.limit(2000)`).
const PAGE_LIMIT: i64 = 2000;

/// POST to a gateway endpoint the shared `GatewayClient` does not expose yet
/// (same base URL / error semantics as `GatewayClient::post`).
async fn gateway_post(path: &str, body: &Value) -> Result<Value, OctyError> {
    let base = octy_spin::ctx::variable("gateway_url", "GATEWAY_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string());
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let (status, response_body) = http_send(
        Method::Post,
        &url,
        &[("content-type", "application/json")],
        Some(serde_json::to_vec(body).expect("serializable json")),
    )
    .await?;

    let parsed: Value = serde_json::from_slice(&response_body).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(parsed);
    }
    let detail = parsed
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("gateway error")
        .to_string();
    Err(OctyError::internal(format!("gateway {path}: {detail}")))
}

/// Filters accepted by `filter_billable_units` (the keys of the Python
/// `filters` dict; absent == not filtered).
#[derive(Debug, Default)]
pub struct BillingFilters {
    pub account_ids: Option<Vec<String>>,
    pub account_types: Option<Vec<String>>,
    pub unit_types: Option<Vec<String>>,
    pub metrics: Option<Vec<String>>,
    pub process_names: Option<Vec<String>>,
    pub cost_upper_range: Option<i64>,
    pub cost_lower_range: Option<i64>,
    pub currencies: Option<Vec<String>>,
    pub created_at_upper_range: Option<DateTime<Utc>>,
    pub created_at_lower_range: Option<DateTime<Utc>>,
}

/// `create_billable_units_ref` — bulk unordered insert of computed units.
/// Each document gets the mongoengine `created_at` default (`dt.now`).
pub async fn create_billable_units_ref(_ctx: &Ctx, units: &[Value]) -> Result<(), OctyError> {
    let documents: Vec<Value> = units
        .iter()
        .map(|unit| {
            let mut doc = unit.clone();
            doc["created_at"] = now_legacy_date();
            doc
        })
        .collect();

    gateway_post(
        &format!("/v1/mongo/{COLLECTION}/insert-many"),
        &json!({ "documents": documents, "ordered": false }),
    )
    .await
    .map_err(|e| {
        OctyError::internal(format!("Failed to create billable units reference. Exception: {e}"))
    })?;
    Ok(())
}

/// `filter_billable_units` — returns `(units, total)` where `units` is the
/// raw legacy-extended-JSON page (2000 docs max, offset by `cursor`) with
/// `created_at` re-encoded as `'%a, %d %b %Y %H:%M:%S GMT'`, exactly like the
/// Python `int_to_dt(..., as_str=True)` post-processing.
pub async fn filter_billable_units(
    ctx: &Ctx,
    filters: &BillingFilters,
    cursor: i64,
) -> Result<(Vec<Value>, i64), OctyError> {
    let mut queries: Vec<Value> = Vec::new();

    if let Some(v) = &filters.account_ids {
        queries.push(json!({ "account_id": { "$in": v } }));
    }
    if let Some(v) = &filters.account_types {
        queries.push(json!({ "account_type": { "$in": v } }));
    }
    if let Some(v) = &filters.unit_types {
        queries.push(json!({ "unit_type": { "$in": v } }));
    }
    if let Some(v) = &filters.metrics {
        queries.push(json!({ "metric": { "$in": v } }));
    }
    if let Some(v) = &filters.process_names {
        queries.push(json!({ "process_name": { "$in": v } }));
    }
    if let Some(v) = filters.cost_upper_range {
        queries.push(json!({ "total_cost": { "$lte": v } }));
    }
    if let Some(v) = filters.cost_lower_range {
        queries.push(json!({ "total_cost": { "$gte": v } }));
    }
    if let Some(v) = &filters.currencies {
        queries.push(json!({ "currency": { "$in": v } }));
    }
    if let Some(v) = filters.created_at_upper_range {
        queries.push(json!({ "created_at": { "$lte": legacy_date(v) } }));
    }
    if let Some(v) = filters.created_at_lower_range {
        queries.push(json!({ "created_at": { "$gte": legacy_date(v) } }));
    }

    // Python passed query=None (match everything) when no filters were given.
    let query = if queries.is_empty() {
        json!({})
    } else {
        json!({ "$and": queries })
    };

    let mut units = ctx.gateway.find(COLLECTION, query.clone(), cursor, PAGE_LIMIT).await?;
    let total = ctx.gateway.count(COLLECTION, query).await?;

    for unit in &mut units {
        let created_at = unit
            .get("created_at")
            .cloned()
            // Python: unit['created_at'] on a doc without the field → KeyError → 500.
            .ok_or_else(|| OctyError::internal("billable unit document missing created_at"))?;
        if created_at.is_null() {
            unit["created_at"] = Value::Null;
        } else {
            // Python: unit['created_at']['$date'] then int_to_dt(millis, as_str=True).
            let millis = date_millis(&created_at).ok_or_else(|| {
                OctyError::internal("billable unit created_at is not a {\"$date\": …} value")
            })?;
            let dt = int_to_dt(millis)
                .ok_or_else(|| OctyError::internal("billable unit created_at out of range"))?;
            unit["created_at"] = json!(dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string());
        }
    }

    Ok((units, total))
}

/// `delete_account_billable_units` — delete every billable unit belonging to
/// the account (`tbl_billable_units.objects(account_id=...).delete()`).
pub async fn delete_account_billable_units(_ctx: &Ctx, account_id: &str) -> Result<bool, OctyError> {
    gateway_post(
        &format!("/v1/mongo/{COLLECTION}/delete-many"),
        &json!({ "filter": { "account_id": account_id } }),
    )
    .await
    .map_err(|e| OctyError::internal(format!("Failed to delete billable units. Exception: {e}")))?;
    Ok(true)
}
