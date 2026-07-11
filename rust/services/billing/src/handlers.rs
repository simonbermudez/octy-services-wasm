//! Route handlers — ports of `api/routers/billing.py` and `healthz.py`.
//!
//! Note on rate limits: the FastAPI service instantiated a slowapi `Limiter`
//! but registered no per-route limits; either way, Spin components are
//! stateless per request, so rate limiting belongs at the ingress layer.

use octy_shared::errors::{ErrorReason, OctyError};
use serde_json::json;
use spin_sdk::http::{Params, Request, Response};

use octy_spin::ctx::Ctx;

use crate::amqp;
use crate::http_util::*;
use crate::models::DeleteAccountBilling;
use crate::services::billing::{self as billing_service, GetBillableUnitsParams};

fn ctx_or_response() -> Result<Ctx, Response> {
    Ctx::load("billing").map_err(|e| error_response(&e))
}

/// Port of the inline `_str_to_list`: strip whitespace and zero-width
/// characters, split on commas, drop empties, dedupe preserving order.
fn str_to_list(raw: Option<String>) -> Option<Vec<String>> {
    let raw = raw?;
    if raw.is_empty() {
        return None;
    }
    let cleaned: String = raw
        .chars()
        .filter(|c| {
            !(c.is_whitespace()
                || matches!(c, '\u{180B}' | '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}'))
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    let params: Vec<String> = cleaned
        .split(',')
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(s.to_string()))
        .map(|s| s.to_string())
        .collect();
    Some(params)
}

/// FastAPI validated `Optional[int]` query params before the endpoint ran;
/// an un-castable value produced the 422 envelope.
fn int_query_param(req: &Request, name: &str) -> Result<Option<i64>, OctyError> {
    match query_param(req, name) {
        None => Ok(None),
        Some(raw) => raw.trim().parse::<i64>().map(Some).map_err(|_| {
            octy_shared::models::validation_error(vec![json!({
                "loc": ["query", name],
                "msg": "value is not a valid integer",
                "type": "type_error.integer"
            })])
        }),
    }
}

pub async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &json!("OK"))
}

/// GET /v1/admin/billing/units
/// (admin pk/sk auth is enforced upstream of this service, as in the Python)
pub async fn get_billable_units(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    // Query-string type validation happened before the route body in FastAPI.
    let cost_upper_range = match int_query_param(&req, "cost_upper_range") {
        Ok(v) => v,
        Err(err) => return error_response(&err),
    };
    let cost_lower_range = match int_query_param(&req, "cost_lower_range") {
        Ok(v) => v,
        Err(err) => return error_response(&err),
    };

    let cursor = match validate_billing_pagination(&req) {
        Ok(cursor) => cursor,
        Err(message) => {
            return error_response(&OctyError::new(
                400,
                "Missing Parameters",
                vec![ErrorReason::new(message, "")],
            ))
        }
    };

    let params = GetBillableUnitsParams {
        account_ids: str_to_list(query_param(&req, "account_ids")),
        account_types: str_to_list(query_param(&req, "account_types")),
        unit_types: str_to_list(query_param(&req, "unit_types")),
        metrics: str_to_list(query_param(&req, "metrics")),
        process_names: str_to_list(query_param(&req, "process_names")),
        cost_upper_range,
        cost_lower_range,
        currencies: str_to_list(query_param(&req, "currencies")),
        created_at_upper_range: query_param(&req, "created_at_upper_range"),
        created_at_lower_range: query_param(&req, "created_at_lower_range"),
    };

    match billing_service::get_billable_units(&ctx, params, cursor).await {
        Ok((units, total)) => get_billable_units_dto(&units, total, cursor),
        Err(err) => error_response(&err),
    }
}

/// GET /v1/admin/billing/subscriptions
pub async fn get_subscription_plans(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let account_types = str_to_list(query_param(&req, "account_types"));

    let mut subscriptions = Vec::new();
    match account_types {
        // With account_types the Python wrapped each lookup in
        // `try/except Exception: pass`, so even a missing SUBSCRIPTIONS key
        // silently yielded an empty list.
        Some(account_types) => {
            if let Some(plans) = ctx.config.raw().get("SUBSCRIPTIONS").and_then(|v| v.as_array()) {
                for account_type in &account_types {
                    if let Some(plan) = plans
                        .iter()
                        .find(|p| p.get("plan").and_then(|v| v.as_str()) == Some(account_type.as_str()))
                    {
                        subscriptions.push(plan.clone());
                    }
                }
            }
        }
        // Without account_types a missing SUBSCRIPTIONS key raised → 500.
        None => match ctx.config.get_array("SUBSCRIPTIONS") {
            Ok(plans) => subscriptions = plans.clone(),
            Err(err) => return error_response(&err),
        },
    }

    get_subscription_plans_dto(&subscriptions)
}

/// POST /v1/internal/billing/delete  (cluster-internal only — keep off the ingress)
pub async fn delete_billing_internal(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let payload = match DeleteAccountBilling::from_json(req.body()) {
        Ok(payload) => payload,
        Err(err) => return error_response(&err),
    };

    match billing_service::delete_account_billing_internal(&ctx, &payload.account_id).await {
        Ok(is_deleted) => delete_account_billing_dto(is_deleted),
        Err(err) => error_response(&err),
    }
}

/// POST /internal/amqp/consume — deliveries forwarded by the data gateway.
pub async fn amqp_consume(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let outcome = amqp::handle_delivery(&ctx, req.body()).await;
    json_response(outcome.status, &json!({ "detail": outcome.detail }))
}

pub async fn fallback(_req: Request, _params: Params) -> Response {
    not_found()
}
