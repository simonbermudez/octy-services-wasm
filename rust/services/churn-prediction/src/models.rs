//! Port of the pydantic request models in
//! `api/routers/request_models/account.py` (v1.8 semantics), producing the
//! same 422 `RequestValidationError` envelopes on failure.
//!
//! The `Account` session model is covered by `octy_spin::auth::AuthAccount`;
//! only the `DeleteAccountChurnPredictions` body model is needed here.

use octy_shared::errors::OctyError;
use octy_shared::models::validation_error;
use serde_json::{json, Value};

/// `DeleteAccountChurnPredictions { account_id: str }`
#[derive(Debug, Clone)]
pub struct DeleteAccountChurnPredictions {
    pub account_id: String,
}

impl DeleteAccountChurnPredictions {
    pub fn from_json(body: &[u8]) -> Result<Self, OctyError> {
        let root: Value = serde_json::from_slice(body).map_err(|e| {
            validation_error(vec![json!({
                "loc": ["body"], "msg": e.to_string(), "type": "value_error.jsondecode"
            })])
        })?;

        if !root.is_object() {
            return Err(validation_error(vec![json!({
                "loc": ["body"], "msg": "value is not a valid dict", "type": "type_error.dict"
            })]));
        }

        let account_id = match root.get("account_id") {
            None => {
                return Err(validation_error(vec![json!({
                    "loc": ["body", "account_id"],
                    "msg": "field required",
                    "type": "value_error.missing"
                })]))
            }
            Some(Value::Null) => {
                return Err(validation_error(vec![json!({
                    "loc": ["body", "account_id"],
                    "msg": "none is not an allowed value",
                    "type": "type_error.none.not_allowed"
                })]))
            }
            Some(Value::String(s)) => s.clone(),
            // pydantic v1 `str` coerces numbers (and bytes) to strings.
            Some(Value::Number(n)) => n.to_string(),
            Some(_) => {
                return Err(validation_error(vec![json!({
                    "loc": ["body", "account_id"],
                    "msg": "str type expected",
                    "type": "type_error.str"
                })]))
            }
        };

        Ok(Self { account_id })
    }
}
