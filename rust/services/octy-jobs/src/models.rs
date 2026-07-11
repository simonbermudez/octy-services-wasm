//! Ports of the pydantic request/message models:
//!   - `api/routers/request_models/octy_jobs.py` (`OctyJobCallBack`, `DeleteAccountJobs`)
//!   - `data/models/octy_jobs.py` (`CreateOctyJob`, `DeleteOctyJob`, `JobMeta`, `RequiredConfigs`)
//!
//! Validation mirrors pydantic v1 (FastAPI) behaviour: numbers coerce to str,
//! bools/strings coerce to int, `Optional[...]` fields default to `None`, and
//! failures produce `{"loc": [...], "msg": ..., "type": ...}` entries that
//! render inside the 422 envelope exactly like the Python
//! `RequestValidationError` handler did.

use serde_json::{json, Value};

pub fn field_error(loc: &[Value], msg: &str, kind: &str) -> Value {
    json!({ "loc": loc, "msg": msg, "type": kind })
}

fn loc_with(prefix: &[Value], field: &str) -> Vec<Value> {
    let mut loc = prefix.to_vec();
    loc.push(json!(field));
    loc
}

fn coerce_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        // pydantic v1 `str` coerces int/float/Decimal via str(v)
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn coerce_int(v: &Value) -> Option<i64> {
    match v {
        // bool is an int subclass in Python; pydantic v1 accepts it
        Value::Bool(b) => Some(*b as i64),
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn take_str(obj: &Value, prefix: &[Value], field: &str, errors: &mut Vec<Value>) -> Option<String> {
    let loc = loc_with(prefix, field);
    match obj.get(field) {
        None => {
            errors.push(field_error(&loc, "field required", "value_error.missing"));
            None
        }
        Some(Value::Null) => {
            errors.push(field_error(&loc, "none is not an allowed value", "type_error.none.not_allowed"));
            None
        }
        Some(v) => coerce_str(v).or_else(|| {
            errors.push(field_error(&loc, "str type expected", "type_error.str"));
            None
        }),
    }
}

/// `Optional[str]` — missing/null is fine, wrong types still error.
fn opt_str(obj: &Value, prefix: &[Value], field: &str, errors: &mut Vec<Value>) -> Option<String> {
    match obj.get(field) {
        None | Some(Value::Null) => None,
        Some(v) => coerce_str(v).or_else(|| {
            errors.push(field_error(&loc_with(prefix, field), "str type expected", "type_error.str"));
            None
        }),
    }
}

fn take_int(obj: &Value, prefix: &[Value], field: &str, errors: &mut Vec<Value>) -> Option<i64> {
    let loc = loc_with(prefix, field);
    match obj.get(field) {
        None => {
            errors.push(field_error(&loc, "field required", "value_error.missing"));
            None
        }
        Some(Value::Null) => {
            errors.push(field_error(&loc, "none is not an allowed value", "type_error.none.not_allowed"));
            None
        }
        Some(v) => coerce_int(v).or_else(|| {
            errors.push(field_error(&loc, "value is not a valid integer", "type_error.integer"));
            None
        }),
    }
}

fn str_list_items(
    items: &[Value],
    loc: &[Value],
    errors: &mut Vec<Value>,
) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(items.len());
    let mut ok = true;
    for (i, item) in items.iter().enumerate() {
        match coerce_str(item) {
            Some(s) => out.push(s),
            None => {
                let mut item_loc = loc.to_vec();
                item_loc.push(json!(i));
                errors.push(field_error(&item_loc, "str type expected", "type_error.str"));
                ok = false;
            }
        }
    }
    ok.then_some(out)
}

fn take_str_list(obj: &Value, prefix: &[Value], field: &str, errors: &mut Vec<Value>) -> Option<Vec<String>> {
    let loc = loc_with(prefix, field);
    match obj.get(field) {
        None => {
            errors.push(field_error(&loc, "field required", "value_error.missing"));
            None
        }
        Some(Value::Null) => {
            errors.push(field_error(&loc, "none is not an allowed value", "type_error.none.not_allowed"));
            None
        }
        Some(Value::Array(items)) => str_list_items(items, &loc, errors),
        Some(_) => {
            errors.push(field_error(&loc, "value is not a valid list", "type_error.list"));
            None
        }
    }
}

/// `Optional[List[str]]` — missing/null becomes `None`.
fn opt_str_list(obj: &Value, prefix: &[Value], field: &str, errors: &mut Vec<Value>) -> Option<Vec<String>> {
    let loc = loc_with(prefix, field);
    match obj.get(field) {
        None | Some(Value::Null) => None,
        Some(Value::Array(items)) => str_list_items(items, &loc, errors),
        Some(_) => {
            errors.push(field_error(&loc, "value is not a valid list", "type_error.list"));
            None
        }
    }
}

fn take_int_list(obj: &Value, prefix: &[Value], field: &str, errors: &mut Vec<Value>) -> Option<Vec<i64>> {
    let loc = loc_with(prefix, field);
    match obj.get(field) {
        None => {
            errors.push(field_error(&loc, "field required", "value_error.missing"));
            None
        }
        Some(Value::Null) => {
            errors.push(field_error(&loc, "none is not an allowed value", "type_error.none.not_allowed"));
            None
        }
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            let mut ok = true;
            for (i, item) in items.iter().enumerate() {
                match coerce_int(item) {
                    Some(n) => out.push(n),
                    None => {
                        let mut item_loc = loc.clone();
                        item_loc.push(json!(i));
                        errors.push(field_error(&item_loc, "value is not a valid integer", "type_error.integer"));
                        ok = false;
                    }
                }
            }
            ok.then_some(out)
        }
        Some(_) => {
            errors.push(field_error(&loc, "value is not a valid list", "type_error.list"));
            None
        }
    }
}

/// `Optional[Dict]` — missing/null becomes `None`.
fn opt_dict(obj: &Value, prefix: &[Value], field: &str, errors: &mut Vec<Value>) -> Option<Value> {
    match obj.get(field) {
        None | Some(Value::Null) => None,
        Some(v) if v.is_object() => Some(v.clone()),
        Some(_) => {
            errors.push(field_error(&loc_with(prefix, field), "value is not a valid dict", "type_error.dict"));
            None
        }
    }
}

fn parse_root(body: &[u8]) -> Result<Value, Vec<Value>> {
    serde_json::from_slice(body)
        .map_err(|e| vec![field_error(&[json!("body")], &e.to_string(), "value_error.jsondecode")])
}

// ---------------------------------------------------------------------------
// HTTP request models (loc includes the FastAPI "body" prefix)
// ---------------------------------------------------------------------------

/// `OctyJobCallBack` — POST /v1/internal/jobs/callback.
#[derive(Debug, Clone)]
pub struct OctyJobCallBack {
    pub account_id: String,
    pub octy_job_id: String,
    pub message: String,
    pub status: String,
}

impl OctyJobCallBack {
    pub fn from_json(body: &[u8]) -> Result<Self, Vec<Value>> {
        let root = parse_root(body)?;
        let prefix = [json!("body")];
        let mut errors = Vec::new();
        let account_id = take_str(&root, &prefix, "account_id", &mut errors);
        let octy_job_id = take_str(&root, &prefix, "octy_job_id", &mut errors);
        let message = take_str(&root, &prefix, "message", &mut errors);
        let status = take_str(&root, &prefix, "status", &mut errors);
        if errors.is_empty() {
            Ok(Self {
                account_id: account_id.unwrap(),
                octy_job_id: octy_job_id.unwrap(),
                message: message.unwrap(),
                status: status.unwrap(),
            })
        } else {
            Err(errors)
        }
    }
}

/// `DeleteAccountJobs` — POST /v1/internal/jobs/delete.
#[derive(Debug, Clone)]
pub struct DeleteAccountJobs {
    pub account_id: String,
}

impl DeleteAccountJobs {
    pub fn from_json(body: &[u8]) -> Result<Self, Vec<Value>> {
        let root = parse_root(body)?;
        let prefix = [json!("body")];
        let mut errors = Vec::new();
        let account_id = take_str(&root, &prefix, "account_id", &mut errors);
        if errors.is_empty() {
            Ok(Self { account_id: account_id.unwrap() })
        } else {
            Err(errors)
        }
    }
}

// ---------------------------------------------------------------------------
// AMQP message models (constructed directly, no "body" loc prefix)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RequiredConfigs {
    pub account_attributes: Vec<String>,
    pub algorithm_configuration_idxs: Vec<i64>,
}

impl RequiredConfigs {
    fn parse(v: &Value, prefix: &[Value], errors: &mut Vec<Value>) -> Option<Self> {
        let account_attributes = take_str_list(v, prefix, "account_attributes", errors);
        let algorithm_configuration_idxs = take_int_list(v, prefix, "algorithm_configuration_idxs", errors);
        Some(Self {
            account_attributes: account_attributes?,
            algorithm_configuration_idxs: algorithm_configuration_idxs?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct JobMeta {
    pub job_type: String,
    pub amqp_routing_key: String,
    pub required_permissions: Vec<String>,
    pub required_configurations: RequiredConfigs,
    pub desired_runs: i64,
    /// minutes
    pub time_interval: i64,
    pub fail_threshold: i64,
}

impl JobMeta {
    fn parse(v: &Value, prefix: &[Value], errors: &mut Vec<Value>) -> Option<Self> {
        let job_type = take_str(v, prefix, "job_type", errors);
        let amqp_routing_key = take_str(v, prefix, "amqp_routing_key", errors);
        let required_permissions = take_str_list(v, prefix, "required_permissions", errors);
        let required_configurations = match v.get("required_configurations") {
            None => {
                errors.push(field_error(
                    &loc_with(prefix, "required_configurations"),
                    "field required",
                    "value_error.missing",
                ));
                None
            }
            Some(Value::Null) => {
                errors.push(field_error(
                    &loc_with(prefix, "required_configurations"),
                    "none is not an allowed value",
                    "type_error.none.not_allowed",
                ));
                None
            }
            Some(rc) if rc.is_object() => {
                RequiredConfigs::parse(rc, &loc_with(prefix, "required_configurations"), errors)
            }
            Some(_) => {
                errors.push(field_error(
                    &loc_with(prefix, "required_configurations"),
                    "value is not a valid dict",
                    "type_error.dict",
                ));
                None
            }
        };
        let desired_runs = take_int(v, prefix, "desired_runs", errors);
        let time_interval = take_int(v, prefix, "time_interval", errors);
        let fail_threshold = take_int(v, prefix, "fail_threshold", errors);
        Some(Self {
            job_type: job_type?,
            amqp_routing_key: amqp_routing_key?,
            required_permissions: required_permissions?,
            required_configurations: required_configurations?,
            desired_runs: desired_runs?,
            time_interval: time_interval?,
            fail_threshold: fail_threshold?,
        })
    }
}

/// `CreateOctyJob` — consumed from `octy.job.cmd.create`.
#[derive(Debug, Clone)]
pub struct CreateOctyJob {
    pub account_id: String,
    /// sic — the misspelling is a persisted field name in `tbl_octy_jobs`.
    pub alt_dentifier: Option<String>,
    pub job_meta: JobMeta,
    pub job_data: Option<Value>,
}

impl CreateOctyJob {
    pub fn from_value(payload: &Value) -> Result<Self, Vec<Value>> {
        if !payload.is_object() {
            return Err(vec![field_error(&[], "value is not a valid dict", "type_error.dict")]);
        }
        let prefix: [Value; 0] = [];
        let mut errors = Vec::new();
        let account_id = take_str(payload, &prefix, "account_id", &mut errors);
        let alt_dentifier = opt_str(payload, &prefix, "alt_dentifier", &mut errors);
        let job_meta = match payload.get("job_meta") {
            None => {
                errors.push(field_error(&loc_with(&prefix, "job_meta"), "field required", "value_error.missing"));
                None
            }
            Some(Value::Null) => {
                errors.push(field_error(
                    &loc_with(&prefix, "job_meta"),
                    "none is not an allowed value",
                    "type_error.none.not_allowed",
                ));
                None
            }
            Some(v) if v.is_object() => JobMeta::parse(v, &loc_with(&prefix, "job_meta"), &mut errors),
            Some(_) => {
                errors.push(field_error(&loc_with(&prefix, "job_meta"), "value is not a valid dict", "type_error.dict"));
                None
            }
        };
        let job_data = opt_dict(payload, &prefix, "job_data", &mut errors);
        if errors.is_empty() {
            Ok(Self {
                account_id: account_id.unwrap(),
                alt_dentifier,
                job_meta: job_meta.unwrap(),
                job_data,
            })
        } else {
            Err(errors)
        }
    }
}

/// `DeleteOctyJob` — consumed from `octy.job.cmd.delete`.
#[derive(Debug, Clone)]
pub struct DeleteOctyJob {
    pub account_id: String,
    pub octy_job_ids: Option<Vec<String>>,
    pub alt_identifiers: Option<Vec<String>>,
}

impl DeleteOctyJob {
    pub fn from_value(payload: &Value) -> Result<Self, Vec<Value>> {
        if !payload.is_object() {
            return Err(vec![field_error(&[], "value is not a valid dict", "type_error.dict")]);
        }
        let prefix: [Value; 0] = [];
        let mut errors = Vec::new();
        let account_id = take_str(payload, &prefix, "account_id", &mut errors);
        let octy_job_ids = opt_str_list(payload, &prefix, "octy_job_ids", &mut errors);
        let alt_identifiers = opt_str_list(payload, &prefix, "alt_identifiers", &mut errors);
        if errors.is_empty() {
            Ok(Self {
                account_id: account_id.unwrap(),
                octy_job_ids,
                alt_identifiers,
            })
        } else {
            Err(errors)
        }
    }
}
