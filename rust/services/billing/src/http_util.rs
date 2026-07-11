//! Billing-service DTOs (ports of `api/routers/dto/billing.py`) plus the
//! billing-specific pagination validation from `api/routers/utils.py`.
//! Generic envelopes live in `octy_spin::http_util` and are re-exported so
//! handlers keep a single import.

pub use octy_spin::http_util::*;

use serde_json::{json, Value};
use spin_sdk::http::{Request, Response};

/// Port of billing's `validate_pagination_request`.
///
/// Unlike the account service, billing requires the `cursor` header on the
/// units listing and reports the same message for a missing header and an
/// un-castable value (the "must be of type int" branch in the Python is
/// unreachable — `int()` raises before the type check).
pub fn validate_billing_pagination(req: &Request) -> Result<i64, String> {
    header_str(req, "cursor")
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .ok_or_else(|| "Please set a pagination header (-H cursor: int)".to_string())
}

/// Port of `GetBillableUnitsDTO` — 200 with a `cursor` response header set to
/// `cursor + len(units)`.
pub fn get_billable_units_dto(units: &[Value], total: i64, cursor: i64) -> Response {
    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("cursor", (cursor + units.len() as i64).to_string())
        .body(
            serde_json::to_vec(&json!({
                "request_meta": {
                    "request_status": "Success",
                    "message": "",
                    "count": units.len(),
                    "total": total,
                },
                "units": units,
            }))
            .expect("serializable json"),
        )
        .build()
}

/// Port of `GetSubscriptionPlansDTO`.
pub fn get_subscription_plans_dto(subscriptions: &[Value]) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": { "request_status": "Success", "message": "" },
            "subscriptions": subscriptions,
        }),
    )
}

/// Port of `DeleteAccountBillingDTO` (201; the Python reused the "Octy Jobs
/// deleted." message verbatim).
pub fn delete_account_billing_dto(is_deleted: bool) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Octy Jobs deleted." },
            "is_deleted": is_deleted,
        }),
    )
}
