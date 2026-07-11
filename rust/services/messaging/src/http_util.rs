//! Messaging-service DTOs (ports of `api/routers/dto/messaging.py`) plus the
//! raw-reason error envelope used by the messaging routes.
//!
//! Generic envelopes/validation live in `octy_spin::http_util` and are
//! re-exported so handlers keep a single import.

pub use octy_spin::http_util::*;

use octy_shared::errors::{status_code_detail, OctyError};
use serde_json::{json, Value};
use spin_sdk::http::Response;

/// The messaging service raises `OctyException`s whose `reasons` are *not*
/// always `{error_message, extended_help}` dicts — e.g. `No templates
/// created!` carries the raw `failed_to_create` objects. `MsgError` keeps
/// those raw reason payloads so the rendered envelope matches the Python
/// byte-for-byte.
#[derive(Debug, Clone)]
pub enum MsgError {
    Octy(OctyError),
    /// `OctyException(code, reason, [<raw json reasons>])`
    Raw {
        code: u16,
        reason: String,
        errors: Vec<Value>,
    },
}

impl From<OctyError> for MsgError {
    fn from(err: OctyError) -> Self {
        MsgError::Octy(err)
    }
}

impl MsgError {
    pub fn internal(detail: impl Into<String>) -> Self {
        MsgError::Octy(OctyError::internal(detail))
    }

    pub fn response(&self) -> Response {
        match self {
            MsgError::Octy(err) => error_response(err),
            MsgError::Raw { code, reason, errors } => {
                if *code >= 500 {
                    eprintln!("[octy-messaging] [{code}] {reason}: {errors:?}");
                }
                let message = if *code == 500 {
                    "Unknown server error"
                } else {
                    status_code_detail(*code)
                };
                json_response(
                    *code,
                    &json!({
                        "request_meta": { "request_status": "Failed", "message": message },
                        "error": { "code": code, "reason": reason, "errors": errors }
                    }),
                )
            }
        }
    }
}

/// FastAPI `RequestValidationError` envelope with the raw pydantic-style
/// `{loc, msg, type}` error objects (the Python handler appended them as-is).
pub fn validation_raw(errors: Vec<Value>) -> MsgError {
    MsgError::Raw {
        code: 422,
        reason: "Missing or invalid JSON parameters".to_string(),
        errors,
    }
}

// ---- DTOs (ports of api/routers/dto/messaging.py) ----

/// `GetTemplatesDTO` — 200 with a `cursor` response header.
pub fn get_templates_dto(templates: &[Value], total: i64, cursor: i64) -> Response {
    let body = json!({
        "request_meta": {
            "request_status": "Success",
            "message": "Messaging templates found.",
            "count": templates.len(),
            "total": total,
        },
        "templates": templates,
    });
    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("cursor", (cursor + templates.len() as i64).to_string())
        .body(serde_json::to_vec(&body).expect("serializable json"))
        .build()
}

/// `CreateTemplatesDTO` — 201.
pub fn create_templates_dto(templates: &[Value], failed_to_create: &[Value]) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Successfully created new message templates." },
            "templates": templates,
            "failed_to_create": failed_to_create,
        }),
    )
}

/// `UpdateTemplatesDTO` — 200.
pub fn update_templates_dto(templates: &[Value], failed_to_update: &[Value]) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Message templates updated." },
            "templates": templates,
            "failed_to_update": failed_to_update,
        }),
    )
}

/// `DeleteTemplatesDTO` — 200.
pub fn delete_templates_dto(deleted_templates: &[Value], failed_to_delete: &[Value]) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Message templates deleted." },
            "deleted_templates": deleted_templates,
            "failed_to_delete": failed_to_delete,
        }),
    )
}

/// `GenerateContentDTO` — 200.
pub fn generate_content_dto(
    created_messages: &[Value],
    failed_messages: &[Value],
    failed_templates: &[Value],
) -> Response {
    let msg = if created_messages.is_empty() {
        "No content generated"
    } else {
        "Successfully generated content"
    };
    json_response(
        200,
        &json!({
            "request_meta": { "request_status": "Success", "message": msg, "count": created_messages.len() },
            "generated_messages": created_messages,
            "failed_messages": failed_messages,
            "failed_templates": failed_templates,
        }),
    )
}

/// `DeleteAccountMessagingDTO` — 201 (message text kept verbatim from the
/// Python, including the copy-pasted "Octy Jobs deleted.").
pub fn delete_account_messaging_dto(is_deleted: bool) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Octy Jobs deleted." },
            "is_deleted": is_deleted,
        }),
    )
}
