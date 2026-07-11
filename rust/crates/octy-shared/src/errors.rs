//! Port of `account/api/routers/error_handlers.py`.
//!
//! Every non-2xx response the Python services emit is built from
//! `Config['ERROR_TEMPLATE']`:
//!
//! ```json
//! {
//!   "request_meta": { "request_status": "Failed", "message": "..." },
//!   "error": { "code": 400, "reason": "...", "errors": [ {"error_message": "...", "extended_help": "..."} ] }
//! }
//! ```
//!
//! `OctyError` carries the same information and renders the same JSON body, so
//! API consumers see byte-compatible error envelopes.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// One entry of the `error.errors` array.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorReason {
    pub error_message: String,
    pub extended_help: String,
}

impl ErrorReason {
    pub fn new(message: impl Into<String>, extended_help: impl Into<String>) -> Self {
        Self {
            error_message: message.into(),
            extended_help: extended_help.into(),
        }
    }
}

/// Port of `OctyException(code, error_description, reasons)`.
#[derive(Debug, Clone)]
pub struct OctyError {
    pub code: u16,
    pub error_description: String,
    pub reasons: Vec<ErrorReason>,
}

/// `status_code_detail_map` from the Python error handlers.
pub fn status_code_detail(code: u16) -> &'static str {
    match code {
        400 => "Bad request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Resource not found",
        411 => "Length Required",
        415 => "Unsupported media type",
        422 => "Unprocessable Entity",
        429 => "Too many requests",
        501 => "Not Implemented",
        _ => "Unknown server error",
    }
}

impl OctyError {
    pub fn new(
        code: u16,
        error_description: impl Into<String>,
        reasons: Vec<ErrorReason>,
    ) -> Self {
        Self {
            code,
            error_description: error_description.into(),
            reasons,
        }
    }

    /// Convenience for unexpected failures — mirrors the generic 500 handler.
    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(
            500,
            "Internal Server Error",
            vec![ErrorReason::new(detail, "")],
        )
    }

    /// Mirrors the FastAPI `RequestValidationError` handler (422).
    pub fn validation(errors: Vec<Value>) -> Self {
        let reasons = errors
            .into_iter()
            .map(|e| ErrorReason::new(e.to_string(), ""))
            .collect();
        Self::new(422, "Missing or invalid JSON parameters", reasons)
    }

    /// Render the Octy error envelope body.
    pub fn to_body(&self) -> Value {
        let message = if self.code == 500 {
            "Unknown server error".to_string()
        } else {
            status_code_detail(self.code).to_string()
        };
        json!({
            "request_meta": {
                "request_status": "Failed",
                "message": message,
            },
            "error": {
                "code": self.code,
                "reason": self.error_description,
                "errors": self.reasons,
            }
        })
    }
}

impl std::fmt::Display for OctyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.error_description)
    }
}

impl std::error::Error for OctyError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_python_compatible_envelope() {
        let err = OctyError::new(
            401,
            "Authentication failed",
            vec![ErrorReason::new("Invalid public_key or secret_key provided", "help")],
        );
        let body = err.to_body();
        assert_eq!(body["request_meta"]["message"], "Unauthorized");
        assert_eq!(body["error"]["code"], 401);
        assert_eq!(body["error"]["reason"], "Authentication failed");
        assert_eq!(
            body["error"]["errors"][0]["error_message"],
            "Invalid public_key or secret_key provided"
        );
    }
}
