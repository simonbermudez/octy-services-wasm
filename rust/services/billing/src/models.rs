//! Ports of `billing/api/routers/request_models/billing.py` and
//! `billing/data/models/billing.py` — request/message bodies with
//! pydantic-equivalent validation. Validation failures render the same 422
//! envelope FastAPI's `RequestValidationError` handler produced (via
//! `octy_shared::models::validation_error`).

use octy_shared::errors::OctyError;
use octy_shared::models::validation_error;
use serde_json::{json, Value};

/// pydantic-style error entry: `{"loc": [...], "msg": ..., "type": ...}`.
fn field_error(loc: Vec<Value>, msg: &str, kind: &str) -> Value {
    json!({ "loc": loc, "msg": msg, "type": kind })
}

/// pydantic v1 `str` coercion: strings pass through, ints/floats/bools are
/// coerced to their string form; anything else is a type error.
fn coerce_str(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(if *b { "True".to_string() } else { "False".to_string() }),
        _ => None,
    }
}

/// pydantic v1 `int` coercion: ints pass through, integral floats are
/// truncated, numeric strings are parsed.
fn coerce_int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else {
                n.as_f64().map(|f| f as i64)
            }
        }
        Value::String(s) => s.trim().parse::<i64>().ok(),
        Value::Bool(b) => Some(if *b { 1 } else { 0 }),
        _ => None,
    }
}

fn require_str(
    obj: &Value,
    field: &str,
    loc_prefix: &[Value],
    errors: &mut Vec<Value>,
) -> Option<String> {
    let mut loc: Vec<Value> = loc_prefix.to_vec();
    loc.push(json!(field));
    match obj.get(field) {
        None => {
            errors.push(field_error(loc, "field required", "value_error.missing"));
            None
        }
        Some(Value::Null) => {
            errors.push(field_error(loc, "none is not an allowed value", "type_error.none.not_allowed"));
            None
        }
        Some(v) => match coerce_str(v) {
            Some(s) => Some(s),
            None => {
                errors.push(field_error(loc, "str type expected", "type_error.str"));
                None
            }
        },
    }
}

fn require_int(
    obj: &Value,
    field: &str,
    loc_prefix: &[Value],
    errors: &mut Vec<Value>,
) -> Option<i64> {
    let mut loc: Vec<Value> = loc_prefix.to_vec();
    loc.push(json!(field));
    match obj.get(field) {
        None => {
            errors.push(field_error(loc, "field required", "value_error.missing"));
            None
        }
        Some(Value::Null) => {
            errors.push(field_error(loc, "none is not an allowed value", "type_error.none.not_allowed"));
            None
        }
        Some(v) => match coerce_int(v) {
            Some(i) => Some(i),
            None => {
                errors.push(field_error(loc, "value is not a valid integer", "type_error.integer"));
                None
            }
        },
    }
}

/// Port of `request_models/billing.py::DeleteAccountBilling`.
#[derive(Debug, Clone)]
pub struct DeleteAccountBilling {
    pub account_id: String,
}

impl DeleteAccountBilling {
    pub fn from_json(body: &[u8]) -> Result<Self, OctyError> {
        let value: Value = serde_json::from_slice(body).map_err(|e| {
            validation_error(vec![field_error(
                vec![json!("body")],
                &e.to_string(),
                "value_error.jsondecode",
            )])
        })?;
        let mut errors = Vec::new();
        let account_id = require_str(&value, "account_id", &[json!("body")], &mut errors);
        match (account_id, errors.is_empty()) {
            (Some(account_id), true) => Ok(Self { account_id }),
            _ => Err(validation_error(errors)),
        }
    }
}

/// Port of `data/models/billing.py::UnitsChild`.
#[derive(Debug, Clone)]
pub struct UnitsChild {
    pub unit_type: String,
    pub metric: String,
    pub process_name: String,
    pub quantity: i64,
    pub account_id: String,
    pub account_currency: String,
    pub account_type: String,
}

/// Port of `data/models/billing.py::BillableUnits` — the AMQP
/// `account.billing.cmd.capture` message payload.
#[derive(Debug, Clone)]
pub struct BillableUnits {
    pub units: Vec<UnitsChild>,
}

impl BillableUnits {
    pub fn from_value(value: &Value) -> Result<Self, OctyError> {
        let mut errors = Vec::new();

        let raw_units = match value.get("units") {
            None => {
                errors.push(field_error(vec![json!("units")], "field required", "value_error.missing"));
                return Err(validation_error(errors));
            }
            Some(Value::Array(arr)) => arr,
            Some(_) => {
                errors.push(field_error(vec![json!("units")], "value is not a valid list", "type_error.list"));
                return Err(validation_error(errors));
            }
        };

        let mut units = Vec::with_capacity(raw_units.len());
        for (i, raw) in raw_units.iter().enumerate() {
            let prefix = vec![json!("units"), json!(i)];
            if !raw.is_object() {
                errors.push(field_error(prefix, "value is not a valid dict", "type_error.dict"));
                continue;
            }
            let unit_type = require_str(raw, "unit_type", &prefix, &mut errors);
            let metric = require_str(raw, "metric", &prefix, &mut errors);
            let process_name = require_str(raw, "process_name", &prefix, &mut errors);
            let quantity = require_int(raw, "quantity", &prefix, &mut errors);
            let account_id = require_str(raw, "account_id", &prefix, &mut errors);
            let account_currency = require_str(raw, "account_currency", &prefix, &mut errors);
            let account_type = require_str(raw, "account_type", &prefix, &mut errors);

            if let (
                Some(unit_type),
                Some(metric),
                Some(process_name),
                Some(quantity),
                Some(account_id),
                Some(account_currency),
                Some(account_type),
            ) = (unit_type, metric, process_name, quantity, account_id, account_currency, account_type)
            {
                units.push(UnitsChild {
                    unit_type,
                    metric,
                    process_name,
                    quantity,
                    account_id,
                    account_currency,
                    account_type,
                });
            }
        }

        if errors.is_empty() {
            Ok(Self { units })
        } else {
            Err(validation_error(errors))
        }
    }
}
