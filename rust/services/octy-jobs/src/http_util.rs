//! octy_jobs DTOs (ports of `api/dto/octy_jobs.py` and the router's inline
//! responses). Generic envelopes live in `octy_spin::http_util` and are
//! re-exported so handlers keep a single import.

pub use octy_spin::http_util::*;

use serde_json::{json, Value};
use spin_sdk::http::Response;

/// `DeleteAccountJobsDTO` — 201 like the FastAPI JSONResponse.
pub fn delete_account_jobs_dto(is_deleted: bool) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Octy Jobs deleted." },
            "is_deleted": is_deleted,
        }),
    )
}

/// The FastAPI `RequestValidationError` handler: raw pydantic error dicts
/// (`{"loc": ..., "msg": ..., "type": ...}`) inside the Octy 422 envelope.
pub fn validation_response(errors: &[Value]) -> Response {
    json_response(
        422,
        &json!({
            "request_meta": { "request_status": "Failed", "message": "Unprocessable Entity" },
            "error": {
                "code": 422,
                "reason": "Missing or invalid JSON parameters",
                "errors": errors,
            }
        }),
    )
}
