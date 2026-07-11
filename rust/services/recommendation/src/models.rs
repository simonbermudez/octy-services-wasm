//! Port of `recommendation/api/routers/request_models/recommendation.py` and
//! `recommendation/data/models/recommendations.py` — request bodies with
//! pydantic-v1-equivalent validation. Failures render the same 422 envelope
//! the FastAPI `RequestValidationError` handler produced.

use octy_shared::errors::OctyError;
use octy_shared::models::validation_error;
use serde_json::{json, Value};

fn parse_json_body(body: &[u8]) -> Result<Value, OctyError> {
    serde_json::from_slice(body).map_err(|e| {
        validation_error(vec![json!({
            "loc": ["body"], "msg": e.to_string(), "type": "value_error.jsondecode"
        })])
    })
}

/// pydantic v1 `str` coercion: str passes through, int/float/bool are
/// stringified (bool via Python's `str(True)` → `"True"`).
fn coerce_str(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(if *b { "True".to_string() } else { "False".to_string() }),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn require_str(body: &Value, field: &str, errors: &mut Vec<Value>) -> Option<String> {
    match body.get(field) {
        None => {
            errors.push(json!({
                "loc": ["body", field], "msg": "field required", "type": "value_error.missing"
            }));
            None
        }
        Some(Value::Null) => {
            errors.push(json!({
                "loc": ["body", field],
                "msg": "none is not an allowed value",
                "type": "type_error.none.not_allowed"
            }));
            None
        }
        Some(v) => match coerce_str(v) {
            Some(s) => Some(s),
            None => {
                errors.push(json!({
                    "loc": ["body", field], "msg": "str type expected", "type": "type_error.str"
                }));
                None
            }
        },
    }
}

fn require_str_list(body: &Value, field: &str, errors: &mut Vec<Value>) -> Option<Vec<String>> {
    match body.get(field) {
        None => {
            errors.push(json!({
                "loc": ["body", field], "msg": "field required", "type": "value_error.missing"
            }));
            None
        }
        Some(Value::Null) => {
            errors.push(json!({
                "loc": ["body", field],
                "msg": "none is not an allowed value",
                "type": "type_error.none.not_allowed"
            }));
            None
        }
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            let mut ok = true;
            for (i, item) in items.iter().enumerate() {
                match item {
                    Value::Null => {
                        errors.push(json!({
                            "loc": ["body", field, i],
                            "msg": "none is not an allowed value",
                            "type": "type_error.none.not_allowed"
                        }));
                        ok = false;
                    }
                    other => match coerce_str(other) {
                        Some(s) => out.push(s),
                        None => {
                            errors.push(json!({
                                "loc": ["body", field, i],
                                "msg": "str type expected",
                                "type": "type_error.str"
                            }));
                            ok = false;
                        }
                    },
                }
            }
            if ok {
                Some(out)
            } else {
                None
            }
        }
        Some(_) => {
            errors.push(json!({
                "loc": ["body", field], "msg": "value is not a valid list", "type": "type_error.list"
            }));
            None
        }
    }
}

/// `GetRecomendations` — POST /v1/retention/recommendations body.
/// NB: the pydantic validator message hardcodes "100" regardless of the
/// configured `MAX_REC_PREDICTIONS`, exactly like the Python.
#[derive(Debug, Clone)]
pub struct GetRecomendations {
    pub profile_ids: Vec<String>,
}

impl GetRecomendations {
    pub fn from_json(body: &[u8], max_rec_predictions: i64) -> Result<Self, OctyError> {
        let value = parse_json_body(body)?;
        let mut errors = Vec::new();
        let profile_ids = require_str_list(&value, "profile_ids", &mut errors);
        if let Some(ids) = &profile_ids {
            if ids.len() as i64 > max_rec_predictions {
                errors.push(json!({
                    "loc": ["body", "profile_ids"],
                    "msg": "You can only generate up to 100 item recommendations per request.",
                    "type": "value_error"
                }));
            }
        }
        match (profile_ids, errors.is_empty()) {
            (Some(profile_ids), true) => Ok(Self { profile_ids }),
            _ => Err(validation_error(errors)),
        }
    }
}

/// `GetRecomendationsInternal` — POST /v1/internal/recommendations body.
#[derive(Debug, Clone)]
pub struct GetRecomendationsInternal {
    pub account_id: String,
    pub profile_ids: Vec<String>,
}

impl GetRecomendationsInternal {
    pub fn from_json(body: &[u8]) -> Result<Self, OctyError> {
        let value = parse_json_body(body)?;
        let mut errors = Vec::new();
        let account_id = require_str(&value, "account_id", &mut errors);
        let profile_ids = require_str_list(&value, "profile_ids", &mut errors);
        match (account_id, profile_ids) {
            (Some(account_id), Some(profile_ids)) => Ok(Self {
                account_id,
                profile_ids,
            }),
            _ => Err(validation_error(errors)),
        }
    }
}

/// `DeleteAccountRecommendations` — POST /v1/internal/recommendations/delete body.
#[derive(Debug, Clone)]
pub struct DeleteAccountRecommendations {
    pub account_id: String,
}

impl DeleteAccountRecommendations {
    pub fn from_json(body: &[u8]) -> Result<Self, OctyError> {
        let value = parse_json_body(body)?;
        let mut errors = Vec::new();
        match require_str(&value, "account_id", &mut errors) {
            Some(account_id) => Ok(Self { account_id }),
            None => Err(validation_error(errors)),
        }
    }
}

/// `DeleteRecCache` — AMQP `reccache.cmd.delete` payload.
#[derive(Debug, Clone)]
pub struct DeleteRecCache {
    pub account_id: String,
    pub profiles: Vec<String>,
}

impl DeleteRecCache {
    pub fn from_json(body: &[u8]) -> Result<Self, OctyError> {
        let value = parse_json_body(body)?;
        let mut errors = Vec::new();
        let account_id = require_str(&value, "account_id", &mut errors);
        let profiles = require_str_list(&value, "profiles", &mut errors);
        match (account_id, profiles) {
            (Some(account_id), Some(profiles)) => Ok(Self {
                account_id,
                profiles,
            }),
            _ => Err(validation_error(errors)),
        }
    }
}
