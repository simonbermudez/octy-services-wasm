//! Items-service error plumbing.
//!
//! Most errors are plain `OctyError`s (typed `error_message`/`extended_help`
//! reasons). The Python service, however, also raised `OctyException`s whose
//! `reasons` were *arbitrary dicts* — e.g. `raise OctyException(400, 'No items
//! created!', failed)` where `failed` is a list of `{"item_id": …,
//! "error_message": …}` objects, and the FastAPI `RequestValidationError`
//! handler appended raw pydantic `{"loc": …, "msg": …, "type": …}` entries.
//! `ApiError::Raw` renders those envelopes byte-compatibly.

use octy_shared::errors::{status_code_detail, OctyError};
use serde_json::{json, Value};
use spin_sdk::http::Response;

pub enum ApiError {
    Octy(OctyError),
    /// `error.errors` entries are arbitrary JSON values (not ErrorReason).
    Raw {
        code: u16,
        reason: String,
        errors: Vec<Value>,
    },
}

impl From<OctyError> for ApiError {
    fn from(err: OctyError) -> Self {
        ApiError::Octy(err)
    }
}

impl ApiError {
    pub fn raw(code: u16, reason: impl Into<String>, errors: Vec<Value>) -> Self {
        ApiError::Raw {
            code,
            reason: reason.into(),
            errors,
        }
    }

    /// The FastAPI `RequestValidationError` handler: 422, reason
    /// `'Missing or invalid JSON parameters'`, raw pydantic error entries.
    pub fn validation(errors: Vec<Value>) -> Self {
        Self::raw(422, "Missing or invalid JSON parameters", errors)
    }

    pub fn response(&self) -> Response {
        match self {
            ApiError::Octy(err) => octy_spin::http_util::error_response(err),
            ApiError::Raw {
                code,
                reason,
                errors,
            } => {
                let message = if *code >= 500 {
                    "Unknown server error"
                } else {
                    status_code_detail(*code)
                };
                octy_spin::http_util::json_response(
                    *code,
                    &json!({
                        "request_meta": {
                            "request_status": "Failed",
                            "message": message,
                        },
                        "error": {
                            "code": code,
                            "reason": reason,
                            "errors": errors,
                        }
                    }),
                )
            }
        }
    }
}
