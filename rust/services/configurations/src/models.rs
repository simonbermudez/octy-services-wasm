//! Port of `configurations/api/routers/request_models/configurations.py` —
//! pydantic-v1-equivalent request parsing and validation.
//!
//! Validation failures surface as lists of pydantic-style error dicts
//! (`{"loc": [...], "msg": ..., "type": ...}`); the handlers wrap them in the
//! 422 envelope the FastAPI `RequestValidationError` handler produced (each
//! error dict becomes `error_message`, with `Config['CONFIG_EXTENDED_HELP']`
//! as `extended_help` — see `http_util::validation_response`).
//!
//! Loc prefixes mirror the Python exactly:
//!   * models validated by FastAPI from the request body → `["body", ...]`
//!   * `SetRecAlgoConfigs` / `SetChurnAlgoConfigs`, constructed manually from
//!     `request._json['configurations']` and re-raised as
//!     `RequestValidationError` → `["configurations", ...]` (no `body`).

use octy_shared::models::{validate_email, validate_http_url};
use serde_json::{json, Value};

/// pydantic-style error entry. `loc` items may be strings or list indices.
pub fn field_error(loc: Vec<Value>, msg: &str, kind: &str) -> Value {
    json!({ "loc": loc, "msg": msg, "type": kind })
}

fn missing(loc: Vec<Value>) -> Value {
    field_error(loc, "field required", "value_error.missing")
}

/// pydantic v1 `str` coercion: str as-is, numbers/bools via Python `str()`.
fn coerce_str(v: &Value) -> Result<String, (&'static str, &'static str)> {
    match v {
        Value::String(s) => Ok(s.clone()),
        // Python: str(True) == 'True' (bool is an int subclass).
        Value::Bool(b) => Ok(if *b { "True".to_string() } else { "False".to_string() }),
        Value::Number(n) => Ok(n.to_string()),
        Value::Null => Err(("none is not an allowed value", "type_error.none.not_allowed")),
        _ => Err(("str type expected", "type_error.str")),
    }
}

/// pydantic v1 `bool` coercion.
fn coerce_bool(v: &Value) -> Result<bool, (&'static str, &'static str)> {
    const ERR: (&'static str, &'static str) =
        ("value could not be parsed to a boolean", "type_error.bool");
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => n.as_i64().map(|i| i != 0).ok_or(ERR),
        Value::String(s) => match s.to_lowercase().as_str() {
            "on" | "t" | "true" | "y" | "yes" | "1" => Ok(true),
            "off" | "f" | "false" | "n" | "no" | "0" => Ok(false),
            _ => Err(ERR),
        },
        Value::Null => Err(("none is not an allowed value", "type_error.none.not_allowed")),
        _ => Err(ERR),
    }
}

/// pydantic v1 `List[str]`: list check + per-item str coercion.
fn coerce_str_list(v: &Value, loc: &[Value]) -> Result<Vec<String>, Vec<Value>> {
    let Some(arr) = v.as_array() else {
        return Err(vec![field_error(
            loc.to_vec(),
            "value is not a valid list",
            "type_error.list",
        )]);
    };
    let mut out = Vec::with_capacity(arr.len());
    let mut errs = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        match coerce_str(item) {
            Ok(s) => out.push(s),
            Err((msg, kind)) => {
                let mut item_loc = loc.to_vec();
                item_loc.push(json!(i));
                errs.push(field_error(item_loc, msg, kind));
            }
        }
    }
    if errs.is_empty() {
        Ok(out)
    } else {
        Err(errs)
    }
}

/// `Optional[str] = <default>` field: absent → default, `null` → null,
/// otherwise str-coerced. (The Python models let callers override
/// `event_type` / `*_item_identifier` this way — kept bug-for-bug.)
fn optional_str_field(
    root: &Value,
    key: &str,
    default: Value,
    loc_head: &str,
    errs: &mut Vec<Value>,
) -> Value {
    match root.get(key) {
        None => default,
        Some(Value::Null) => Value::Null,
        Some(v) => match coerce_str(v) {
            Ok(s) => Value::String(s),
            Err((msg, kind)) => {
                errs.push(field_error(vec![json!(loc_head), json!(key)], msg, kind));
                Value::Null
            }
        },
    }
}

fn parse_body_object(body: &[u8]) -> Result<Value, Vec<Value>> {
    let root: Value = serde_json::from_slice(body).map_err(|e| {
        vec![field_error(
            vec![json!("body")],
            &e.to_string(),
            "value_error.jsondecode",
        )]
    })?;
    if !root.is_object() {
        return Err(vec![field_error(
            vec![json!("body")],
            "value is not a valid dict",
            "type_error.dict",
        )]);
    }
    Ok(root)
}

/// The shared `profile_features` validator (identical on both algorithm
/// models — including the "recommendations" wording on the churn model).
fn validate_profile_features(features: &[String]) -> Result<(), String> {
    for f in features {
        if f == "charged" {
            return Err(
                "Unable to set configurations for recommendations algorithm. 'charged' can not be set as a profile feature."
                    .to_string(),
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// SetAccountConfigs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SetAccountConfigs {
    pub account_id: Option<String>,
    pub contact_email_address: String,
    pub contact_name: String,
    pub contact_surname: String,
    pub webhook_url: String,
    pub authenticated_id_key: Option<String>,
}

impl SetAccountConfigs {
    pub fn from_json(body: &[u8]) -> Result<Self, Vec<Value>> {
        let root = parse_body_object(body)?;
        let mut errs: Vec<Value> = Vec::new();

        let str_field = |key: &str, errs: &mut Vec<Value>| -> String {
            match root.get(key) {
                None => {
                    errs.push(missing(vec![json!("body"), json!(key)]));
                    String::new()
                }
                Some(v) => match coerce_str(v) {
                    Ok(s) => s,
                    Err((msg, kind)) => {
                        errs.push(field_error(vec![json!("body"), json!(key)], msg, kind));
                        String::new()
                    }
                },
            }
        };

        let opt_field = |key: &str, errs: &mut Vec<Value>| -> Option<String> {
            match root.get(key) {
                None | Some(Value::Null) => None,
                Some(v) => match coerce_str(v) {
                    Ok(s) => Some(s),
                    Err((msg, kind)) => {
                        errs.push(field_error(vec![json!("body"), json!(key)], msg, kind));
                        None
                    }
                },
            }
        };

        // Declaration order matters for the error list ordering.
        let account_id = opt_field("account_id", &mut errs);

        let mut contact_email_address = String::new();
        match root.get("contact_email_address") {
            None => errs.push(missing(vec![json!("body"), json!("contact_email_address")])),
            Some(v) => match coerce_str(v) {
                Ok(raw) => match validate_email(&raw) {
                    Ok(normalized) => contact_email_address = normalized,
                    // The Python validator maps EmailNotValidError to this message.
                    Err(_) => errs.push(field_error(
                        vec![json!("body"), json!("contact_email_address")],
                        "Invalid contact email address provided.",
                        "value_error",
                    )),
                },
                Err((msg, kind)) => errs.push(field_error(
                    vec![json!("body"), json!("contact_email_address")],
                    msg,
                    kind,
                )),
            },
        }

        let contact_name = str_field("contact_name", &mut errs);
        let contact_surname = str_field("contact_surname", &mut errs);

        let mut webhook_url = String::new();
        match root.get("webhook_url") {
            None => errs.push(missing(vec![json!("body"), json!("webhook_url")])),
            Some(v) => match coerce_str(v) {
                Ok(raw) => match validate_http_url(&raw) {
                    Ok(url) => webhook_url = url,
                    Err(msg) => errs.push(field_error(
                        vec![json!("body"), json!("webhook_url")],
                        &msg,
                        "value_error.url",
                    )),
                },
                Err((msg, kind)) => {
                    errs.push(field_error(vec![json!("body"), json!("webhook_url")], msg, kind))
                }
            },
        }

        let authenticated_id_key = opt_field("authenticated_id_key", &mut errs);

        if errs.is_empty() {
            Ok(Self {
                account_id,
                contact_email_address,
                contact_name,
                contact_surname,
                webhook_url,
                authenticated_id_key,
            })
        } else {
            Err(errs)
        }
    }
}

// ---------------------------------------------------------------------------
// BaseSetAlgoConfigs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BaseSetAlgoConfigs {
    pub algorithm_name: String,
    /// The raw `configurations` dict (validated per-algorithm afterwards).
    pub configurations: Value,
}

impl BaseSetAlgoConfigs {
    /// `allowed_algorithms` is `Config['OCTY_ALGO_TYPES']`.
    pub fn from_json(body: &[u8], allowed_algorithms: &[Value]) -> Result<Self, Vec<Value>> {
        let root = parse_body_object(body)?;
        let mut errs: Vec<Value> = Vec::new();

        let mut algorithm_name = String::new();
        match root.get("algorithm_name") {
            None => errs.push(missing(vec![json!("body"), json!("algorithm_name")])),
            Some(v) => match coerce_str(v) {
                Ok(name) => {
                    if allowed_algorithms.iter().any(|a| a == &Value::String(name.clone())) {
                        algorithm_name = name;
                    } else {
                        errs.push(field_error(
                            vec![json!("body"), json!("algorithm_name")],
                            "Invalid algorithm name provided. Allowed algorithm names : 'rec' or 'churn'",
                            "value_error",
                        ));
                    }
                }
                Err((msg, kind)) => {
                    errs.push(field_error(vec![json!("body"), json!("algorithm_name")], msg, kind))
                }
            },
        }

        let mut configurations = Value::Null;
        match root.get("configurations") {
            None => errs.push(missing(vec![json!("body"), json!("configurations")])),
            Some(v) if v.is_object() => configurations = v.clone(),
            Some(_) => errs.push(field_error(
                vec![json!("body"), json!("configurations")],
                "value is not a valid dict",
                "type_error.dict",
            )),
        }

        if errs.is_empty() {
            Ok(Self {
                algorithm_name,
                configurations,
            })
        } else {
            Err(errs)
        }
    }
}

// ---------------------------------------------------------------------------
// RecConfigs / ChurnPredConfigs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RecConfigs {
    pub recommend_interacted_items: bool,
    pub item_id_stop_list: Vec<String>,
    pub profile_features: Vec<String>,
    pub event_type: Value,
    pub rec_item_identifier: Value,
}

impl RecConfigs {
    /// Validate `request._json['configurations']` — loc prefix `configurations`.
    pub fn from_value(cfg: &Value) -> Result<Self, Vec<Value>> {
        let mut errs: Vec<Value> = Vec::new();

        let recommend_interacted_items = match cfg.get("recommend_interacted_items") {
            None => {
                errs.push(missing(vec![
                    json!("configurations"),
                    json!("recommend_interacted_items"),
                ]));
                false
            }
            Some(v) => match coerce_bool(v) {
                Ok(b) => b,
                Err((msg, kind)) => {
                    errs.push(field_error(
                        vec![json!("configurations"), json!("recommend_interacted_items")],
                        msg,
                        kind,
                    ));
                    false
                }
            },
        };

        let item_id_stop_list = match cfg.get("item_id_stop_list") {
            None => {
                errs.push(missing(vec![json!("configurations"), json!("item_id_stop_list")]));
                Vec::new()
            }
            Some(v) => match coerce_str_list(v, &[json!("configurations"), json!("item_id_stop_list")]) {
                Ok(list) => list,
                Err(mut e) => {
                    errs.append(&mut e);
                    Vec::new()
                }
            },
        };

        let profile_features =
            parse_profile_features(cfg, &mut errs);

        let event_type = optional_str_field(cfg, "event_type", json!("charged"), "configurations", &mut errs);
        let rec_item_identifier = optional_str_field(
            cfg,
            "rec_item_identifier",
            json!("item_id"),
            "configurations",
            &mut errs,
        );

        if errs.is_empty() {
            Ok(Self {
                recommend_interacted_items,
                item_id_stop_list,
                profile_features,
                event_type,
                rec_item_identifier,
            })
        } else {
            Err(errs)
        }
    }

    /// The full `configurations.dict()` published to AMQP, with the
    /// (possibly mutated) stop list. Field order follows the pydantic model.
    pub fn config_json(&self, item_id_stop_list: Value) -> Value {
        json!({
            "recommend_interacted_items": self.recommend_interacted_items,
            "item_id_stop_list": item_id_stop_list,
            "profile_features": self.profile_features,
            "event_type": self.event_type,
            "rec_item_identifier": self.rec_item_identifier,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ChurnPredConfigs {
    pub profile_features: Vec<String>,
    pub event_type: Value,
    pub churn_item_identifier: Value,
}

impl ChurnPredConfigs {
    pub fn from_value(cfg: &Value) -> Result<Self, Vec<Value>> {
        let mut errs: Vec<Value> = Vec::new();

        let profile_features = parse_profile_features(cfg, &mut errs);

        let event_type = optional_str_field(cfg, "event_type", json!("charged"), "configurations", &mut errs);
        let churn_item_identifier = optional_str_field(
            cfg,
            "churn_item_identifier",
            json!("item_id"),
            "configurations",
            &mut errs,
        );

        if errs.is_empty() {
            Ok(Self {
                profile_features,
                event_type,
                churn_item_identifier,
            })
        } else {
            Err(errs)
        }
    }

    pub fn config_json(&self) -> Value {
        json!({
            "profile_features": self.profile_features,
            "event_type": self.event_type,
            "churn_item_identifier": self.churn_item_identifier,
        })
    }
}

/// `profile_features : List[str]` + the `allowed_profile_features` validator
/// (the validator only runs when the type coercion succeeded, like pydantic).
fn parse_profile_features(cfg: &Value, errs: &mut Vec<Value>) -> Vec<String> {
    match cfg.get("profile_features") {
        None => {
            errs.push(missing(vec![json!("configurations"), json!("profile_features")]));
            Vec::new()
        }
        Some(v) => match coerce_str_list(v, &[json!("configurations"), json!("profile_features")]) {
            Ok(list) => {
                if let Err(msg) = validate_profile_features(&list) {
                    errs.push(field_error(
                        vec![json!("configurations"), json!("profile_features")],
                        &msg,
                        "value_error",
                    ));
                }
                list
            }
            Err(mut e) => {
                errs.append(&mut e);
                Vec::new()
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_account_configs_valid() {
        let body = json!({
            "contact_email_address": "Jane@ACME.io",
            "contact_name": "Jane",
            "contact_surname": "Doe",
            "webhook_url": "https://acme.io/hook"
        });
        let configs = SetAccountConfigs::from_json(&serde_json::to_vec(&body).unwrap()).unwrap();
        assert_eq!(configs.contact_email_address, "Jane@acme.io");
        assert!(configs.account_id.is_none());
        assert!(configs.authenticated_id_key.is_none());
    }

    #[test]
    fn set_account_configs_invalid_email_and_url() {
        let body = json!({
            "contact_email_address": "nope",
            "contact_name": "Jane",
            "contact_surname": "Doe",
            "webhook_url": "ftp://acme.io"
        });
        let errs = SetAccountConfigs::from_json(&serde_json::to_vec(&body).unwrap()).unwrap_err();
        assert_eq!(errs.len(), 2);
        assert_eq!(errs[0]["msg"], "Invalid contact email address provided.");
        assert_eq!(errs[1]["type"], "value_error.url");
    }

    #[test]
    fn base_algo_configs_rejects_unknown_algorithm() {
        let allowed = vec![json!("rec"), json!("churn")];
        let body = json!({ "algorithm_name": "rfm", "configurations": {} });
        let errs =
            BaseSetAlgoConfigs::from_json(&serde_json::to_vec(&body).unwrap(), &allowed).unwrap_err();
        assert_eq!(
            errs[0]["msg"],
            "Invalid algorithm name provided. Allowed algorithm names : 'rec' or 'churn'"
        );
    }

    #[test]
    fn rec_configs_rejects_charged_profile_feature() {
        let cfg = json!({
            "recommend_interacted_items": true,
            "item_id_stop_list": [],
            "profile_features": ["charged"]
        });
        let errs = RecConfigs::from_value(&cfg).unwrap_err();
        assert_eq!(errs[0]["loc"], json!(["configurations", "profile_features"]));
        assert_eq!(errs[0]["type"], "value_error");
    }

    #[test]
    fn rec_configs_defaults_and_overrides() {
        let cfg = json!({
            "recommend_interacted_items": false,
            "item_id_stop_list": ["item_1"],
            "profile_features": ["total_spend"]
        });
        let rec = RecConfigs::from_value(&cfg).unwrap();
        assert_eq!(rec.event_type, json!("charged"));
        assert_eq!(rec.rec_item_identifier, json!("item_id"));

        let cfg = json!({
            "recommend_interacted_items": false,
            "item_id_stop_list": [],
            "profile_features": [],
            "event_type": "viewed"
        });
        let rec = RecConfigs::from_value(&cfg).unwrap();
        assert_eq!(rec.event_type, json!("viewed"));
    }

    #[test]
    fn churn_configs_missing_field() {
        let errs = ChurnPredConfigs::from_value(&json!({})).unwrap_err();
        assert_eq!(errs[0]["loc"], json!(["configurations", "profile_features"]));
        assert_eq!(errs[0]["type"], "value_error.missing");
    }
}
