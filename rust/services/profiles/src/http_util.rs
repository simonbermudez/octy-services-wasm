//! Profiles-service DTOs (port of `api/routers/dto/profiles.py`) plus a
//! small error type that can carry either the canonical
//! `{error_message, extended_help}` reasons (`octy_shared::errors::OctyError`)
//! or the *raw* list-of-dicts several Python error paths returned verbatim
//! (e.g. `failed_to_create`/`failed_to_update`/`failed_to_delete` become the
//! `error.errors` array directly, with no `extended_help` key). Building
//! those responses by hand here (rather than forcing them through
//! `ErrorReason`, which always serializes both fields) keeps the JSON
//! byte-compatible without touching the shared `octy-shared` crate.

pub use octy_spin::http_util::*;

use octy_shared::errors::{status_code_detail, OctyError};
use serde_json::{json, Value};
use spin_sdk::http::Response;

pub enum ServiceError {
    Octy(OctyError),
    RawList { code: u16, reason: String, errors: Vec<Value> },
}

impl From<OctyError> for ServiceError {
    fn from(e: OctyError) -> Self {
        ServiceError::Octy(e)
    }
}

pub fn service_error_response(err: &ServiceError) -> Response {
    match err {
        ServiceError::Octy(e) => error_response(e),
        ServiceError::RawList { code, reason, errors } => {
            let message = if *code == 500 { "Unknown server error" } else { status_code_detail(*code) };
            let body = json!({
                "request_meta": { "request_status": "Failed", "message": message },
                "error": { "code": code, "reason": reason, "errors": errors }
            });
            json_response(*code, &body)
        }
    }
}

// ---------------------------------------------------------------------
// DTOs (api/routers/dto/profiles.py)
// ---------------------------------------------------------------------

pub fn get_profiles_dto(profiles: &[Value], total: i64, cursor: i64) -> Response {
    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("cursor", (cursor + profiles.len() as i64).to_string())
        .body(
            serde_json::to_vec(&json!({
                "request_meta": { "request_status": "Success", "message": "Customer profiles found.", "count": profiles.len(), "total": total },
                "profiles": profiles,
            }))
            .expect("serializable json"),
        )
        .build()
}

pub fn create_profiles_dto(created: &[Value], failed_to_create: &[Value]) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Customer profiles created.", "count": created.len() },
            "profiles": created,
            "failed_to_create": failed_to_create,
        }),
    )
}

pub fn update_profiles_dto(updated: &[Value], failed_to_update: &[Value]) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Customer profiles updated." },
            "profiles": updated,
            "failed_to_update": failed_to_update,
        }),
    )
}

pub fn delete_profiles_dto(deleted: &[Value], failed_to_delete: &[Value]) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Customer profiles deleted." },
            "deleted_profiles": deleted,
            "failed_to_delete": failed_to_delete,
        }),
    )
}

pub fn get_profiles_meta_dto(profiles_meta: &[Value]) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Profiles metadata returned." },
            "profiles_meta": profiles_meta,
        }),
    )
}

pub fn get_profiles_internal_dto(profiles: &[Value], not_found: &Value, total: i64, cursor: i64) -> Response {
    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("cursor", (cursor + profiles.len() as i64).to_string())
        .body(
            serde_json::to_vec(&json!({
                "request_meta": { "request_status": "Success", "message": "Customer profiles found.", "count": profiles.len(), "total": total },
                "profiles": profiles,
                "not_found": not_found,
            }))
            .expect("serializable json"),
        )
        .build()
}

pub fn delete_account_profiles_dto(is_deleted: bool) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Customer profiles deleted." },
            "is_deleted": is_deleted,
        }),
    )
}
