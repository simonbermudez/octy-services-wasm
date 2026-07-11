//! Port of `api/routers/request_models/messaging.py` — pydantic-equivalent
//! request models. Validation failures render the same 422 envelope FastAPI's
//! `RequestValidationError` handler produced, with the raw `{loc, msg, type}`
//! pydantic error objects.

use octy_shared::config::Config;
use octy_shared::errors::OctyError;
use serde_json::{json, Map, Value};

use crate::http_util::{validation_raw, MsgError};

// ---- Python string/`repr()` helpers (error messages embed Python reprs) ----

/// `str(v)` for a JSON value the way Python renders parsed-JSON values.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => py_repr(other),
    }
}

/// `repr(v)` for a JSON value the way Python renders parsed-JSON values
/// (dict/list literals with single-quoted strings, `True`/`False`/`None`).
pub fn py_repr(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => py_str_repr(s),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", py_str_repr(k), py_repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr()` of a str: single quotes unless the string contains a
/// single quote (and no double quote).
pub fn py_str_repr(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        format!("\"{s}\"")
    } else {
        let escaped = s.replace('\\', "\\\\").replace('\'', "\\'");
        format!("'{escaped}'")
    }
}

/// Python `repr()` of a list of strings.
pub fn py_str_list_repr(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| py_str_repr(s)).collect();
    format!("[{}]", inner.join(", "))
}

// ---- pydantic error plumbing ----

fn field_error(loc: Vec<Value>, msg: &str, kind: &str) -> Value {
    json!({ "loc": loc, "msg": msg, "type": kind })
}

fn json_decode_error(detail: &str) -> MsgError {
    validation_raw(vec![field_error(
        vec![json!("body")],
        detail,
        "value_error.jsondecode",
    )])
}

/// pydantic v1 `str` coercion: str as-is, int/float/bool via `str(...)`.
fn coerce_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(true) => Some("True".to_string()),
        Value::Bool(false) => Some("False".to_string()),
        _ => None,
    }
}

fn require_str_field(
    obj: &Map<String, Value>,
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
            errors.push(field_error(
                loc,
                "none is not an allowed value",
                "type_error.none.not_allowed",
            ));
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

// ---- Template placeholder extraction (re.finditer(r"\{\{(.*?)\}\}", DOTALL)) ----

pub fn extract_placeholder_tags(content: &str) -> Vec<String> {
    let re = regex_lite::Regex::new(r"(?s)\{\{(.*?)\}\}").expect("valid regex");
    re.captures_iter(content)
        .map(|c| c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default())
        .collect()
}

// ---- Request models ----

/// `CreateTemplatesChild` / `UpdateTemplatesChild` (identical except for the
/// leading `template_id` on updates).
#[derive(Debug, Clone)]
pub struct TemplateChild {
    /// `Some` only for update requests.
    pub template_id: Option<String>,
    pub friendly_name: String,
    pub template_type: String,
    pub title: String,
    pub content: String,
    /// `Optional[Dict[str, str]]` with the `or {}` pre-validator applied.
    pub default_values: Map<String, Value>,
    /// `Optional[Dict[str, Any]]` — `None` when the field was absent.
    pub metadata: Option<Value>,
}

pub struct CreateTemplates {
    pub templates: Vec<TemplateChild>,
}

pub struct UpdateTemplates {
    pub templates: Vec<TemplateChild>,
}

pub struct DeleteTemplates {
    pub template_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GenerateContentChild {
    pub template_id: String,
    /// `List[Dict[str, Optional[str]]]` — values coerced to strings / null.
    pub data: Vec<Map<String, Value>>,
}

pub struct GenerateContent {
    pub messages: Vec<GenerateContentChild>,
}

pub struct DeleteAccountMessaging {
    pub account_id: String,
}

fn parse_body(body: &[u8]) -> Result<Value, MsgError> {
    serde_json::from_slice(body).map_err(|e| json_decode_error(&e.to_string()))
}

/// `validate_friendly_name` — length + disallowed characters.
fn validate_friendly_name(value: &str) -> Result<(), String> {
    // Python len() counts characters, not bytes.
    let char_len = value.chars().count();
    if char_len > 60 || char_len < 1 {
        return Err(
            "Message template friendly names must be at least 1 character long and less than 60 characters long."
                .to_string(),
        );
    }
    let disallowed = [',', '"', '\'', '.'];
    let found: Vec<String> = disallowed
        .iter()
        .filter(|c| value.contains(**c))
        .map(|c| c.to_string())
        .collect();
    if !found.is_empty() {
        return Err(format!(
            "Illegal character(s) found in provided message template friendly name : {}",
            py_str_list_repr(&found)
        ));
    }
    Ok(())
}

/// `_metadata_validation`. Returns `Err(MsgError)` (a 500) for the Python
/// `None.items()` AttributeError when `metadata` is explicitly `null`.
fn validate_metadata(
    value: &Value,
    loc_prefix: &[Value],
    errors: &mut Vec<Value>,
) -> Result<(), MsgError> {
    let mut loc: Vec<Value> = loc_prefix.to_vec();
    loc.push(json!("metadata"));
    let map = match value {
        // Explicit null → the pydantic validator ran `None.items()` →
        // AttributeError → FastAPI 500. Preserved.
        Value::Null => {
            return Err(MsgError::internal(
                "AttributeError: 'NoneType' object has no attribute 'items'",
            ))
        }
        Value::Object(map) => map,
        _ => {
            errors.push(field_error(loc, "value is not a valid dict", "type_error.dict"));
            return Ok(());
        }
    };
    for (k, v) in map {
        let key_len = k.chars().count();
        if key_len > 40 || key_len < 1 {
            errors.push(field_error(
                loc,
                "Metadata keys must be at least 1 character long and less than 40 characters long.",
                "value_error",
            ));
            return Ok(());
        }
        let val_len = py_str(v).chars().count();
        if val_len > 500 || val_len < 1 {
            errors.push(field_error(
                loc,
                "Metadata values must be at least 1 character long and less than 500 characters long.",
                "value_error",
            ));
            return Ok(());
        }
    }
    Ok(())
}

/// `default_values : Optional[Dict[str, str]]` with the pre/always
/// `default_values or {}` validator.
fn parse_default_values(
    obj: &Map<String, Value>,
    loc_prefix: &[Value],
    errors: &mut Vec<Value>,
) -> Map<String, Value> {
    let raw = obj.get("default_values");
    // `default_values or {}`: missing / null / falsy → {}
    let falsy = match raw {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => !*b,
        Some(Value::Number(n)) => n.as_f64() == Some(0.0),
        Some(Value::String(s)) => s.is_empty(),
        Some(Value::Array(a)) => a.is_empty(),
        Some(Value::Object(o)) => o.is_empty(),
    };
    if falsy {
        return Map::new();
    }
    let raw = raw.expect("checked above");
    let mut loc: Vec<Value> = loc_prefix.to_vec();
    loc.push(json!("default_values"));
    let Some(map) = raw.as_object() else {
        errors.push(field_error(loc, "value is not a valid dict", "type_error.dict"));
        return Map::new();
    };
    let mut out = Map::new();
    for (k, v) in map {
        let mut value_loc = loc.clone();
        value_loc.push(json!(k));
        match v {
            Value::Null => errors.push(field_error(
                value_loc,
                "none is not an allowed value",
                "type_error.none.not_allowed",
            )),
            other => match coerce_str(other) {
                Some(s) => {
                    out.insert(k.clone(), Value::String(s));
                }
                None => errors.push(field_error(value_loc, "str type expected", "type_error.str")),
            },
        }
    }
    out
}

/// `_template_content_validation` — placeholder count + default-value match.
fn template_content_validation(
    templates: &[TemplateChild],
    config: &Config,
) -> Result<Result<(), String>, OctyError> {
    let max_required = config.get_i64("MAX_REQUIRED_DATA")?;
    for t in templates {
        let required_data = extract_placeholder_tags(&t.content);
        if required_data.len() as i64 > max_required {
            return Ok(Err(format!(
                "Template : {}. A maximum number of {} placeholder tags allowed per template.",
                t.friendly_name, max_required
            )));
        }
        let default_value_keys: Vec<String> = t.default_values.keys().cloned().collect();
        // set differences, first-seen order
        let df_rd: Vec<String> = {
            let mut seen: Vec<String> = Vec::new();
            for k in &default_value_keys {
                if !required_data.contains(k) && !seen.contains(k) {
                    seen.push(k.clone());
                }
            }
            seen
        };
        let rd_df: Vec<String> = {
            let mut seen: Vec<String> = Vec::new();
            for k in &required_data {
                if !default_value_keys.contains(k) && !seen.contains(k) {
                    seen.push(k.clone());
                }
            }
            seen
        };
        if !df_rd.is_empty() || !rd_df.is_empty() {
            let mismatches = if !df_rd.is_empty() { &df_rd } else { &rd_df };
            return Ok(Err(format!(
                "Template : {}. Please ensure the placeholder tags set in the 'content' parameter match the values provided in the 'default_values' parameter. Found mismatches : {}",
                t.friendly_name,
                py_str_list_repr(mismatches)
            )));
        }
    }
    Ok(Ok(()))
}

fn parse_template_children(
    body: &[u8],
    config: &Config,
    is_update: bool,
) -> Result<Vec<TemplateChild>, MsgError> {
    let root = parse_body(body)?;
    let mut errors: Vec<Value> = Vec::new();

    let templates_val = match root.get("templates") {
        None => {
            return Err(validation_raw(vec![field_error(
                vec![json!("body"), json!("templates")],
                "field required",
                "value_error.missing",
            )]))
        }
        Some(v) => v,
    };
    let Some(list) = templates_val.as_array() else {
        return Err(validation_raw(vec![field_error(
            vec![json!("body"), json!("templates")],
            "value is not a valid list",
            "type_error.list",
        )]));
    };

    let mut out: Vec<TemplateChild> = Vec::new();
    for (i, item) in list.iter().enumerate() {
        let loc_prefix: Vec<Value> = vec![json!("body"), json!("templates"), json!(i)];
        let Some(obj) = item.as_object() else {
            errors.push(field_error(
                loc_prefix,
                "value is not a valid dict",
                "type_error.dict",
            ));
            continue;
        };

        let template_id = if is_update {
            require_str_field(obj, "template_id", &loc_prefix, &mut errors)
        } else {
            None
        };
        let friendly_name = require_str_field(obj, "friendly_name", &loc_prefix, &mut errors);
        if let Some(name) = &friendly_name {
            if let Err(msg) = validate_friendly_name(name) {
                let mut loc = loc_prefix.clone();
                loc.push(json!("friendly_name"));
                errors.push(field_error(loc, &msg, "value_error"));
            }
        }
        let template_type = require_str_field(obj, "template_type", &loc_prefix, &mut errors);
        let title = require_str_field(obj, "title", &loc_prefix, &mut errors);
        let content = require_str_field(obj, "content", &loc_prefix, &mut errors);
        let default_values = parse_default_values(obj, &loc_prefix, &mut errors);
        let metadata = match obj.get("metadata") {
            None => None,
            Some(v) => {
                validate_metadata(v, &loc_prefix, &mut errors)?;
                Some(v.clone())
            }
        };

        if is_update && template_id.is_none() {
            continue;
        }
        match (friendly_name, template_type, title, content) {
            (Some(friendly_name), Some(template_type), Some(title), Some(content)) => {
                out.push(TemplateChild {
                    template_id,
                    friendly_name,
                    template_type,
                    title,
                    content,
                    default_values,
                    metadata,
                });
            }
            _ => {}
        }
    }

    if !errors.is_empty() {
        return Err(validation_raw(errors));
    }

    // Parent validators run only when every child validated: `length`, then
    // `content_validation`.
    let (max_key, verb) = if is_update {
        ("MAX_UPDATE_DELETE_TEMPLATES", "update")
    } else {
        ("MAX_CREATE_TEMPLATES", "create")
    };
    let max = config.get_i64(max_key).map_err(MsgError::Octy)?;
    if out.len() as i64 > max {
        return Err(validation_raw(vec![field_error(
            vec![json!("body"), json!("templates")],
            &format!("You can only {verb} up to {max} templates per request."),
            "value_error",
        )]));
    }
    match template_content_validation(&out, config).map_err(MsgError::Octy)? {
        Ok(()) => {}
        Err(msg) => {
            return Err(validation_raw(vec![field_error(
                vec![json!("body"), json!("templates")],
                &msg,
                "value_error",
            )]))
        }
    }

    Ok(out)
}

impl CreateTemplates {
    pub fn from_json(body: &[u8], config: &Config) -> Result<Self, MsgError> {
        Ok(Self {
            templates: parse_template_children(body, config, false)?,
        })
    }
}

impl UpdateTemplates {
    pub fn from_json(body: &[u8], config: &Config) -> Result<Self, MsgError> {
        Ok(Self {
            templates: parse_template_children(body, config, true)?,
        })
    }
}

impl DeleteTemplates {
    pub fn from_json(body: &[u8], config: &Config) -> Result<Self, MsgError> {
        let root = parse_body(body)?;
        let mut errors: Vec<Value> = Vec::new();
        let ids_val = match root.get("template_ids") {
            None => {
                return Err(validation_raw(vec![field_error(
                    vec![json!("body"), json!("template_ids")],
                    "field required",
                    "value_error.missing",
                )]))
            }
            Some(v) => v,
        };
        let Some(list) = ids_val.as_array() else {
            return Err(validation_raw(vec![field_error(
                vec![json!("body"), json!("template_ids")],
                "value is not a valid list",
                "type_error.list",
            )]));
        };
        let mut template_ids = Vec::new();
        for (i, item) in list.iter().enumerate() {
            match coerce_str(item) {
                Some(s) => template_ids.push(s),
                None => errors.push(field_error(
                    vec![json!("body"), json!("template_ids"), json!(i)],
                    "str type expected",
                    "type_error.str",
                )),
            }
        }
        if !errors.is_empty() {
            return Err(validation_raw(errors));
        }
        let max = config.get_i64("MAX_UPDATE_DELETE_TEMPLATES").map_err(MsgError::Octy)?;
        if template_ids.len() as i64 > max {
            return Err(validation_raw(vec![field_error(
                vec![json!("body"), json!("template_ids")],
                &format!("You can only delete up to {max} templates per request."),
                "value_error",
            )]));
        }
        Ok(Self { template_ids })
    }
}

impl DeleteAccountMessaging {
    pub fn from_json(body: &[u8]) -> Result<Self, MsgError> {
        let root = parse_body(body)?;
        let account_id = match root.get("account_id") {
            None | Some(Value::Null) => {
                let kind = if root.get("account_id").is_some() {
                    ("none is not an allowed value", "type_error.none.not_allowed")
                } else {
                    ("field required", "value_error.missing")
                };
                return Err(validation_raw(vec![field_error(
                    vec![json!("body"), json!("account_id")],
                    kind.0,
                    kind.1,
                )]));
            }
            Some(v) => match coerce_str(v) {
                Some(s) => s,
                None => {
                    return Err(validation_raw(vec![field_error(
                        vec![json!("body"), json!("account_id")],
                        "str type expected",
                        "type_error.str",
                    )]))
                }
            },
        };
        Ok(Self { account_id })
    }
}

// ---- GenerateContent + its three validator classes ----

impl GenerateContent {
    pub fn from_json(body: &[u8], config: &Config) -> Result<Self, MsgError> {
        let root = parse_body(body)?;
        let mut errors: Vec<Value> = Vec::new();

        let messages_val = match root.get("messages") {
            None => {
                return Err(validation_raw(vec![field_error(
                    vec![json!("body"), json!("messages")],
                    "field required",
                    "value_error.missing",
                )]))
            }
            Some(v) => v,
        };
        let Some(list) = messages_val.as_array() else {
            return Err(validation_raw(vec![field_error(
                vec![json!("body"), json!("messages")],
                "value is not a valid list",
                "type_error.list",
            )]));
        };

        let mut messages: Vec<GenerateContentChild> = Vec::new();
        for (i, item) in list.iter().enumerate() {
            let loc_prefix: Vec<Value> = vec![json!("body"), json!("messages"), json!(i)];
            let Some(obj) = item.as_object() else {
                errors.push(field_error(
                    loc_prefix,
                    "value is not a valid dict",
                    "type_error.dict",
                ));
                continue;
            };
            let template_id = require_str_field(obj, "template_id", &loc_prefix, &mut errors);

            let mut data: Vec<Map<String, Value>> = Vec::new();
            let mut data_ok = true;
            match obj.get("data") {
                None => {
                    let mut loc = loc_prefix.clone();
                    loc.push(json!("data"));
                    errors.push(field_error(loc, "field required", "value_error.missing"));
                    data_ok = false;
                }
                Some(v) => match v.as_array() {
                    None => {
                        let mut loc = loc_prefix.clone();
                        loc.push(json!("data"));
                        errors.push(field_error(loc, "value is not a valid list", "type_error.list"));
                        data_ok = false;
                    }
                    Some(entries) => {
                        for (j, entry) in entries.iter().enumerate() {
                            let Some(entry_obj) = entry.as_object() else {
                                let mut loc = loc_prefix.clone();
                                loc.push(json!("data"));
                                loc.push(json!(j));
                                errors.push(field_error(
                                    loc,
                                    "value is not a valid dict",
                                    "type_error.dict",
                                ));
                                data_ok = false;
                                continue;
                            };
                            // Dict[str, Optional[str]] — coerce values.
                            let mut coerced = Map::new();
                            for (k, v) in entry_obj {
                                match v {
                                    Value::Null => {
                                        coerced.insert(k.clone(), Value::Null);
                                    }
                                    other => match coerce_str(other) {
                                        Some(s) => {
                                            coerced.insert(k.clone(), Value::String(s));
                                        }
                                        None => {
                                            let mut loc = loc_prefix.clone();
                                            loc.push(json!("data"));
                                            loc.push(json!(j));
                                            loc.push(json!(k));
                                            errors.push(field_error(
                                                loc,
                                                "str type expected",
                                                "type_error.str",
                                            ));
                                            data_ok = false;
                                        }
                                    },
                                }
                            }
                            data.push(coerced);
                        }
                    }
                },
            }

            if let (Some(template_id), true) = (template_id, data_ok) {
                messages.push(GenerateContentChild { template_id, data });
            }
        }

        if !errors.is_empty() {
            return Err(validation_raw(errors));
        }

        // Parent validators: `length` then the three content validator chains.
        let limit = config.get_i64("MESSAGE_GEN_LIMIT").map_err(MsgError::Octy)?;
        if messages.len() as i64 > limit {
            // NB: message hardcodes 100 (with typo) regardless of the limit —
            // preserved from the Python.
            return Err(validation_raw(vec![field_error(
                vec![json!("body"), json!("messages")],
                "You can only generate up to 100 messagess per request.",
                "value_error",
            )]));
        }
        if let Err(err) = validate_message_content(&messages, config) {
            return Err(err);
        }
        if let Err(err) = validate_item_rec_message_content(&messages, config) {
            return Err(err);
        }
        if let Err(err) = validate_rybbon_message_content(&messages) {
            return Err(err);
        }

        Ok(Self { messages })
    }
}

fn messages_value_error(msg: String) -> MsgError {
    validation_raw(vec![field_error(
        vec![json!("body"), json!("messages")],
        &msg,
        "value_error",
    )])
}

/// `ValidateMessageContent`
fn validate_message_content(
    messages: &[GenerateContentChild],
    config: &Config,
) -> Result<(), MsgError> {
    let max_data = config.get_i64("MAX_MESSAGE_DATA").map_err(MsgError::Octy)?;
    let mut templates: Vec<&str> = Vec::new();
    for (midx, message) in messages.iter().enumerate() {
        templates.push(&message.template_id);
        if message.data.len() as i64 > max_data {
            return Err(messages_value_error(format!(
                "loc: messages : {midx}. A maximum number of {max_data} data objects allowed per message."
            )));
        }
    }
    let unique: std::collections::HashSet<&&str> = templates.iter().collect();
    if unique.len() != templates.len() {
        return Err(messages_value_error(
            "loc: messages : Duplicate template identifiers found in messages.".to_string(),
        ));
    }
    Ok(())
}

const PROFILE_ID_RE: &str = r"^profile_[a-zA-Z0-9]";

fn is_profile_id(value: &str) -> bool {
    regex_lite::Regex::new(PROFILE_ID_RE)
        .expect("valid regex")
        .is_match(value)
}

/// `ValidateItemRecMessageContent`
fn validate_item_rec_message_content(
    messages: &[GenerateContentChild],
    config: &Config,
) -> Result<(), MsgError> {
    let item_attributes = config.get("ITEM_ATTRIBUTES").map_err(MsgError::Octy)?.clone();
    let allowed_currencies = config.get("ALLOWED_CURRENCIES").map_err(MsgError::Octy)?.clone();
    let item_attr_list: Vec<String> = item_attributes
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let validate_key_structure = |key: &str| -> String {
        let dot_count = key.matches('.').count();
        if dot_count != 1 {
            return format!(
                " -> key: '{key}'. item_rec keys must contain only one '.' character seperating the keyword 'item_rec' and the specified item attribute."
            );
        }
        let params: Vec<&str> = key.split('.').collect();
        if params[0] != "item_rec" {
            return format!(
                " -> key: '{key}'. item_rec keys must contain only one '.' character seperating the keyword 'item_rec' and the specified item attribute."
            );
        }
        if !item_attr_list.iter().any(|a| a == params[1]) {
            return format!(
                " -> key: '{key}'. Illegal item attribute provided: '{}'. Allowed item attributes : {}",
                params[1],
                py_repr(&item_attributes)
            );
        }
        String::new()
    };

    let validate_item_price_params = |value: &str| -> String {
        let params: Vec<&str> = value.split("::").collect();
        if params.len() != 4 {
            return format!(
                " . Invalid value provided for item_price parameter. item_price parameters must contain four values separated by three sets of '::'. Expected : profile_id::currency_from::curency_to::discount. Value provided : {value}"
            );
        }
        for (i, param) in params.iter().enumerate() {
            match i {
                0 => {
                    if !is_profile_id(param) {
                        return format!(
                            ". Invalid value provided for item_price 'profile_id' parameter. Must be a valid Octy profile identifier. Value provided : {param}"
                        );
                    }
                }
                1 => {
                    if allowed_currencies.get(*param).is_none() {
                        return format!(
                            ". Invalid value provided for the item_price 'currency_from' parameter. Must be a valid accpeted currency code : {}. Value provided : {param}",
                            py_repr(&allowed_currencies)
                        );
                    }
                }
                2 => {
                    if allowed_currencies.get(*param).is_none() {
                        return format!(
                            ". Invalid value provided for the item_price 'currency_to' parameter. Must be a valid accpeted currency code : {}. Value provided : {param}",
                            py_repr(&allowed_currencies)
                        );
                    }
                }
                3 => {
                    let ok = param
                        .trim()
                        .parse::<i64>()
                        .map(|n| (0..=90).contains(&n))
                        .unwrap_or(false);
                    if !ok {
                        return format!(
                            ". Invalid value provided for item_price 'discount' parameter. Must be a number greater than or equal to (if no discount is to be applied) 0. Value provided : {param}"
                        );
                    }
                }
                _ => {}
            }
        }
        String::new()
    };

    let mut message_profiles: Vec<String> = Vec::new();
    for (midx, message) in messages.iter().enumerate() {
        let mut is_rec = false;
        let mut data_profiles: Vec<String> = Vec::new();
        for (didx, d) in message.data.iter().enumerate() {
            data_profiles.clear();
            for (k, raw_value) in d {
                if !k.contains('.') {
                    continue;
                }
                // Assume it's an item_rec key — no others allow '.'.
                is_rec = true;
                let err = validate_key_structure(k);
                if !err.is_empty() {
                    return Err(messages_value_error(format!(
                        "loc: messages : {midx} -> data: {didx}{err}"
                    )));
                }
                // _validate_contains_profile_id
                let profile_value: String = if k.contains("item_price") {
                    match raw_value.as_str() {
                        Some(s) => s.split("::").next().unwrap_or("").to_string(),
                        // None.split(...) → caught by the bare `except` in Python.
                        None => {
                            let err = format!(
                                " -> key: '{k}'. item_rec.item_price key values must contain '::' seperated values using the following sytax: profile_id::currency_from::currency_to::discount"
                            );
                            return Err(messages_value_error(format!(
                                "loc: messages : {midx} -> data: {didx}{err}"
                            )));
                        }
                    }
                } else {
                    match raw_value.as_str() {
                        Some(s) => s.to_string(),
                        // re.match(..., None) → TypeError → 500 in Python.
                        None => {
                            return Err(MsgError::internal(
                                "TypeError: expected string or bytes-like object",
                            ))
                        }
                    }
                };
                if !is_profile_id(&profile_value) {
                    let err = format!(
                        " -> key: '{k}'. item_rec key values must contain a valid Octy generated profile identifier as their first value."
                    );
                    return Err(messages_value_error(format!(
                        "loc: messages : {midx} -> data: {didx}{err}"
                    )));
                }
                data_profiles.push(profile_value.clone());
                if !message_profiles.contains(&profile_value) {
                    message_profiles.push(profile_value);
                }
                if k.contains("item_price") {
                    let value_str = raw_value.as_str().unwrap_or_default();
                    let err = validate_item_price_params(value_str);
                    if !err.is_empty() {
                        return Err(messages_value_error(format!(
                            "loc: messages : {midx} -> data: {didx}{err}"
                        )));
                    }
                }
            }
            if is_rec {
                let unique: std::collections::HashSet<&String> = data_profiles.iter().collect();
                if unique.len() > 1 {
                    return Err(messages_value_error(format!(
                        "loc: messages : {midx} -> data: {didx}. All profile identifiers provided within any single data object must match"
                    )));
                }
            }
        }
        if is_rec {
            let unique: std::collections::HashSet<&String> = message_profiles.iter().collect();
            if unique.len() != message.data.len() {
                return Err(messages_value_error(format!(
                    "loc: messages : {midx} . Identical profile identifiers found across more than one data object. Each data object within any message object should represent one profile or person."
                )));
            }
        }
        message_profiles.clear();
    }
    Ok(())
}

/// `ValidateRybbonMessageContent`
fn validate_rybbon_message_content(messages: &[GenerateContentChild]) -> Result<(), MsgError> {
    let mut message_customer_ids: Vec<String> = Vec::new();
    // NB: never reset across messages — preserved Python quirk (mixing rybbon
    // and non-rybbon messages in one request fails the duplicate check).
    let mut is_reward_card = false;

    for (midx, message) in messages.iter().enumerate() {
        for (didx, d) in message.data.iter().enumerate() {
            for (k, v) in d {
                if k != "rybbon_reward_card" {
                    continue;
                }
                is_reward_card = true;
                let value = match v.as_str() {
                    Some(s) => s,
                    // None.split(...) → AttributeError → 500 in Python.
                    None => {
                        return Err(MsgError::internal(
                            "AttributeError: 'NoneType' object has no attribute 'split'",
                        ))
                    }
                };
                let params: Vec<&str> = value.split("::").collect();
                if params.len() != 3 {
                    let err = format!(
                        " . Invalid value provided for rybbon_reward_card parameter. 'rybbon_reward_card' parameters must contain four values seperated by three sets of '::'. Expected : customer_id::rybbon_campaign_key::value. Value provided : {value}"
                    );
                    return Err(messages_value_error(format!(
                        "loc: messages : {midx} -> data: {didx}{err}"
                    )));
                }
                for (i, param) in params.iter().enumerate() {
                    let field = match i {
                        0 => "customer_id",
                        1 => "rybbon_campaign_key",
                        2 => "reward_name",
                        _ => continue,
                    };
                    if param.is_empty() {
                        let err = format!(
                            ". Invalid value provided for rybbon_reward_card '{field}' parameter. Must not be a null value. Value provided : {param}"
                        );
                        return Err(messages_value_error(format!(
                            "loc: messages : {midx} -> data: {didx}{err}"
                        )));
                    }
                    if i == 0 {
                        message_customer_ids.push(param.to_string());
                    }
                }
            }
        }
        if is_reward_card {
            let unique: std::collections::HashSet<&String> = message_customer_ids.iter().collect();
            if unique.len() != message.data.len() {
                return Err(messages_value_error(format!(
                    "loc: messages : {midx} . Identical customer identifiers found across more than one data object. Each data object within any message object should represent one customer or person."
                )));
            }
        }
        message_customer_ids.clear();
    }
    Ok(())
}
