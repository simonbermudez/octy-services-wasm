//! Events-service DTOs (ports of `api/routers/dto/events.py` /
//! `event_types.py`) plus `ApiError`, an Octy error envelope that — unlike
//! `octy_shared::errors::OctyError` — carries *raw* JSON entries in
//! `error.errors`. The Python handlers append arbitrary dicts (pydantic
//! `{loc,msg,type}` entries, failed-item dicts …) to the envelope, so the
//! error list cannot be constrained to `{error_message, extended_help}`.

pub use octy_spin::http_util::*;

use chrono::Utc;
use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::http::Response;

/// `status_code_detail_map` from `api/routers/error_handlers.py` (note:
/// unlike the shared map, 500 renders as "Internal Server Error" — that is
/// what the `OctyException` handler produced for explicit 500s).
fn status_code_detail_py(code: u16) -> &'static str {
    match code {
        400 => "Bad request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Resource not found",
        411 => "Length Required",
        415 => "Unsupported media type",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        _ => "Unknown server error",
    }
}

#[derive(Debug, Clone)]
pub struct ApiError {
    pub code: u16,
    pub description: String,
    pub errors: Vec<Value>,
    /// `true` for unexpected failures: the response body is the generic 500
    /// envelope (like FastAPI's 500 handler) and the detail only reaches the
    /// component log.
    masked: bool,
}

impl ApiError {
    /// Port of `raise OctyException(code, description, reasons)` — rendered
    /// verbatim by the `octy_exception_handler`.
    pub fn octy(code: u16, description: impl Into<String>, errors: Vec<Value>) -> Self {
        Self {
            code,
            description: description.into(),
            errors,
            masked: false,
        }
    }

    /// OctyException with a single `{error_message, extended_help}` reason.
    pub fn reason(
        code: u16,
        description: impl Into<String>,
        error_message: impl Into<String>,
        extended_help: impl Into<String>,
    ) -> Self {
        Self::octy(
            code,
            description,
            vec![json!({
                "error_message": error_message.into(),
                "extended_help": extended_help.into(),
            })],
        )
    }

    /// Unexpected failure — mirrors the generic FastAPI 500 handler.
    pub fn internal(detail: impl Into<String>) -> Self {
        Self {
            code: 500,
            description: detail.into(),
            errors: vec![],
            masked: true,
        }
    }

    /// Mirrors the `RequestValidationError` handler (raw pydantic errors).
    pub fn validation(errors: Vec<Value>) -> Self {
        Self {
            code: 422,
            description: "Missing or invalid JSON parameters".to_string(),
            errors,
            masked: false,
        }
    }

    pub fn to_response(&self) -> Response {
        if self.masked {
            eprintln!("[events-service] 500: {} {:?}", self.description, self.errors);
            return json_response(
                500,
                &json!({
                    "request_meta": { "request_status": "Failed", "message": "Unknown server error" },
                    "error": {
                        "code": 500,
                        "reason": "Internal Server Error",
                        "errors": [{
                            "error_message": "Unexpected error occurred when attempting to process this request",
                            "extended_help": "",
                        }],
                    }
                }),
            );
        }
        let message = if self.code == 422 {
            // validation_handler sets request_meta.message itself
            "Unprocessable Entity"
        } else {
            status_code_detail_py(self.code)
        };
        json_response(
            self.code,
            &json!({
                "request_meta": { "request_status": "Failed", "message": message },
                "error": {
                    "code": self.code,
                    "reason": self.description,
                    "errors": self.errors,
                }
            }),
        )
    }
}

impl From<OctyError> for ApiError {
    fn from(err: OctyError) -> Self {
        if err.code >= 500 {
            return ApiError::internal(format!("{err}: {:?}", err.reasons));
        }
        ApiError::octy(
            err.code,
            err.error_description,
            err.reasons
                .into_iter()
                .map(|r| serde_json::to_value(r).expect("serializable reason"))
                .collect(),
        )
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.description)
    }
}

/// `'%a, %d %b %Y %H:%M:%S GMT'` — the strftime format shared by the DTOs
/// and `int_to_dt(..., as_str=True)`.
pub const GMT_FMT: &str = "%a, %d %b %Y %H:%M:%S GMT";

// ---------------------------------------------------------------------------
// DTOs — `api/routers/dto/events.py`
// ---------------------------------------------------------------------------

pub fn create_event_dto(event: &Value) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Event created." },
            "event_id": event["event_id"],
            "event_type": event["event_type"],
            "event_properties": event["event_properties"],
            "profile_id": event["profile_id"],
            "created_at": Utc::now().format(GMT_FMT).to_string(),
        }),
    )
}

pub fn batch_create_events_dto(valid_events: &[Value], invalid_events: &[Value]) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Events created.",
                "count": valid_events.len(),
            },
            "created_events": valid_events,
            "failed_to_create": invalid_events,
        }),
    )
}

pub fn internal_get_events_dto(events: &[Value], total: i64) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Events found.",
                "count": events.len(),
                "total": total,
            },
            "events": events,
        }),
    )
}

pub fn get_event_dto(event: &Value) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Event found." },
            "event": event,
        }),
    )
}

pub fn internal_delete_events_dto() -> Response {
    json_response(
        200,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "All Events associated with account deleted.",
            },
        }),
    )
}

// ---------------------------------------------------------------------------
// DTOs — `api/routers/dto/event_types.py`
// ---------------------------------------------------------------------------

pub fn get_event_types_dto(event_types: &[Value], total: i64, cursor: i64) -> Response {
    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("cursor", (cursor + event_types.len() as i64).to_string())
        .body(
            serde_json::to_vec(&json!({
                "request_meta": {
                    "request_status": "Success",
                    "message": "Custom event type(s) found.",
                    "count": event_types.len(),
                    "total": total,
                },
                "event_types": event_types,
            }))
            .expect("serializable json"),
        )
        .build()
}

pub fn create_event_types_dto(event_types: &[Value], failed_to_create: &[Value]) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Custom event type(s) created." },
            "event_types": event_types,
            "failed_to_create": failed_to_create,
        }),
    )
}

pub fn delete_event_types_dto(deleted_event_types: &[Value], failed_to_delete: &[Value]) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Custom event type(s) deleted." },
            "deleted_event_types": deleted_event_types,
            "failed_to_delete": failed_to_delete,
        }),
    )
}

pub fn get_event_types_internal_dto(event_types: &[Value], not_found: &[Value]) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Custom event type(s) found." },
            "event_types": event_types,
            "not_found": not_found,
        }),
    )
}
