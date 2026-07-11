//! Port of `items/api/routers/request_models/items.py` — pydantic-v1
//! equivalent parsing/validation producing the same 422 error entries
//! (`{"loc": …, "msg": …, "type": …}`) the FastAPI handler emitted.
//!
//! The session `Account` model (`request_models/account.py`) is covered by
//! `octy_spin::auth::AuthAccount`.

use octy_shared::errors::OctyError;
use serde_json::{json, Value};

use crate::errors::ApiError;

fn field_error(loc: Vec<Value>, msg: impl Into<String>, kind: &str) -> Value {
    json!({ "loc": loc, "msg": msg.into(), "type": kind })
}

/// Python `str(float)`-ish rendering for pydantic's number→str coercion.
fn py_number_str(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    let f = n.as_f64().unwrap_or(0.0);
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e16 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

/// pydantic v1 `str` field coercion (str | number | bool accepted).
fn coerce_str(value: Option<&Value>, loc: Vec<Value>, errors: &mut Vec<Value>) -> Option<String> {
    match value {
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
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Bool(b)) => Some(if *b { "True".into() } else { "False".into() }),
        Some(Value::Number(n)) => Some(py_number_str(n)),
        Some(_) => {
            errors.push(field_error(loc, "str type expected", "type_error.str"));
            None
        }
    }
}

/// pydantic v1 `int` field coercion (int | float | numeric str | bool).
fn coerce_int(value: Option<&Value>, loc: Vec<Value>, errors: &mut Vec<Value>) -> Option<i64> {
    match value {
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
        Some(Value::Bool(b)) => Some(if *b { 1 } else { 0 }),
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else if let Some(f) = n.as_f64() {
                Some(f.trunc() as i64) // Python int(float) truncates toward zero
            } else {
                errors.push(field_error(loc, "value is not a valid integer", "type_error.integer"));
                None
            }
        }
        Some(Value::String(s)) => match s.trim().parse::<i64>() {
            Ok(i) => Some(i),
            Err(_) => {
                errors.push(field_error(loc, "value is not a valid integer", "type_error.integer"));
                None
            }
        },
        Some(_) => {
            errors.push(field_error(loc, "value is not a valid integer", "type_error.integer"));
            None
        }
    }
}

// ---- field validators (`@validator` ports) ----

/// Python `repr()` of each disallowed character, so the error message matches
/// the f-string interpolation of the found-characters list.
const ITEM_ID_DISALLOWED: &[(char, &str)] = &[
    (',', "','"),
    ('"', "'\"'"),
    ('\'', "\"'\""),
    ('.', "'.'"),
];

fn validate_item_id(value: &str) -> Result<(), String> {
    let n = value.chars().count();
    if n > 60 || n < 1 {
        return Err(
            "Item identifiers must be at least 1 character long and less than 60 characters long."
                .to_string(),
        );
    }
    let found: Vec<&str> = ITEM_ID_DISALLOWED
        .iter()
        .filter(|(c, _)| value.contains(*c))
        .map(|(_, repr)| *repr)
        .collect();
    if !found.is_empty() {
        return Err(format!(
            "Illegal character(s) found in provided item identifier : [{}]",
            found.join(", ")
        ));
    }
    Ok(())
}

fn validate_item_description(value: &str) -> Result<(), String> {
    let n = value.chars().count();
    if n > 40 || n < 1 {
        return Err(
            "Item description must be at least 1 character long and less than 40 characters long."
                .to_string(),
        );
    }
    Ok(())
}

fn validate_status(value: &str) -> Result<(), String> {
    if value != "active" && value != "inactive" {
        return Err(
            "Invalid item status provided. Allowed statuses : 'active', 'inactive'".to_string(),
        );
    }
    Ok(())
}

// ---- request models ----

#[derive(Debug, Clone)]
pub struct CreateItem {
    pub item_id: String,
    pub item_category: String,
    pub item_name: String,
    pub item_description: String,
    pub item_price: i64,
}

#[derive(Debug, Clone)]
pub struct CreateItems {
    pub items: Vec<CreateItem>,
}

#[derive(Debug, Clone)]
pub struct UpdateItem {
    pub item_id: String,
    pub item_category: String,
    pub item_name: String,
    pub item_description: String,
    pub item_price: i64,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct UpdateItems {
    pub items: Vec<UpdateItem>,
}

#[derive(Debug, Clone)]
pub struct DeleteItems {
    pub items: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DeleteAccountItemsInternal {
    pub account_id: String,
}

// ---- parsing ----

fn parse_body_object(body: &[u8]) -> Result<Value, ApiError> {
    let root: Value = serde_json::from_slice(body).map_err(|e| {
        ApiError::validation(vec![field_error(
            vec![json!("body")],
            e.to_string(),
            "value_error.jsondecode",
        )])
    })?;
    if !root.is_object() {
        return Err(ApiError::validation(vec![field_error(
            vec![json!("body")],
            "value is not a valid dict",
            "type_error.dict",
        )]));
    }
    Ok(root)
}

fn items_array(root: &Value) -> Result<&Vec<Value>, ApiError> {
    match root.get("items") {
        None | Some(Value::Null) => Err(ApiError::validation(vec![field_error(
            vec![json!("body"), json!("items")],
            if root.get("items").is_some() {
                "none is not an allowed value"
            } else {
                "field required"
            },
            if root.get("items").is_some() {
                "type_error.none.not_allowed"
            } else {
                "value_error.missing"
            },
        )])),
        Some(Value::Array(arr)) => Ok(arr),
        Some(_) => Err(ApiError::validation(vec![field_error(
            vec![json!("body"), json!("items")],
            "value is not a valid list",
            "type_error.list",
        )])),
    }
}

/// The `items` length validators read `Config[...]` — a missing config key was
/// a `KeyError` inside pydantic (→ 500). `max_items` therefore arrives as a
/// `Result` and is only consulted once all element validation passed, matching
/// pydantic's whole-field-validator ordering.
fn check_length(
    len: usize,
    max_items: &Result<i64, OctyError>,
    message: impl FnOnce(i64) -> String,
) -> Result<(), ApiError> {
    match max_items {
        Ok(max) => {
            if len as i64 > *max {
                Err(ApiError::validation(vec![field_error(
                    vec![json!("body"), json!("items")],
                    message(*max),
                    "value_error",
                )]))
            } else {
                Ok(())
            }
        }
        Err(e) => Err(ApiError::Octy(e.clone())),
    }
}

fn loc_item(idx: usize, field: &str) -> Vec<Value> {
    vec![json!("body"), json!("items"), json!(idx), json!(field)]
}

pub fn parse_create_items(
    body: &[u8],
    max_items: Result<i64, OctyError>,
) -> Result<CreateItems, ApiError> {
    let root = parse_body_object(body)?;
    let raw_items = items_array(&root)?;

    let mut errors: Vec<Value> = Vec::new();
    let mut items: Vec<CreateItem> = Vec::new();

    for (idx, raw) in raw_items.iter().enumerate() {
        if !raw.is_object() {
            errors.push(field_error(
                vec![json!("body"), json!("items"), json!(idx)],
                "value is not a valid dict",
                "type_error.dict",
            ));
            continue;
        }
        let item_id = coerce_str(raw.get("item_id"), loc_item(idx, "item_id"), &mut errors)
            .and_then(|v| match validate_item_id(&v) {
                Ok(()) => Some(v),
                Err(msg) => {
                    errors.push(field_error(loc_item(idx, "item_id"), msg, "value_error"));
                    None
                }
            });
        let item_category =
            coerce_str(raw.get("item_category"), loc_item(idx, "item_category"), &mut errors);
        let item_name = coerce_str(raw.get("item_name"), loc_item(idx, "item_name"), &mut errors);
        let item_description = coerce_str(
            raw.get("item_description"),
            loc_item(idx, "item_description"),
            &mut errors,
        )
        .and_then(|v| match validate_item_description(&v) {
            Ok(()) => Some(v),
            Err(msg) => {
                errors.push(field_error(loc_item(idx, "item_description"), msg, "value_error"));
                None
            }
        });
        let item_price =
            coerce_int(raw.get("item_price"), loc_item(idx, "item_price"), &mut errors);

        if let (Some(item_id), Some(item_category), Some(item_name), Some(item_description), Some(item_price)) =
            (item_id, item_category, item_name, item_description, item_price)
        {
            items.push(CreateItem {
                item_id,
                item_category,
                item_name,
                item_description,
                item_price,
            });
        }
    }

    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }

    check_length(items.len(), &max_items, |max| {
        format!("You can only create up to {max} items per request. For larger uploads, please use the octy cli.")
    })?;

    Ok(CreateItems { items })
}

pub fn parse_update_items(
    body: &[u8],
    max_items: Result<i64, OctyError>,
) -> Result<UpdateItems, ApiError> {
    let root = parse_body_object(body)?;
    let raw_items = items_array(&root)?;

    let mut errors: Vec<Value> = Vec::new();
    let mut items: Vec<UpdateItem> = Vec::new();

    for (idx, raw) in raw_items.iter().enumerate() {
        if !raw.is_object() {
            errors.push(field_error(
                vec![json!("body"), json!("items"), json!(idx)],
                "value is not a valid dict",
                "type_error.dict",
            ));
            continue;
        }
        let item_id = coerce_str(raw.get("item_id"), loc_item(idx, "item_id"), &mut errors)
            .and_then(|v| match validate_item_id(&v) {
                Ok(()) => Some(v),
                Err(msg) => {
                    errors.push(field_error(loc_item(idx, "item_id"), msg, "value_error"));
                    None
                }
            });
        let item_category =
            coerce_str(raw.get("item_category"), loc_item(idx, "item_category"), &mut errors);
        let item_name = coerce_str(raw.get("item_name"), loc_item(idx, "item_name"), &mut errors);
        let item_description = coerce_str(
            raw.get("item_description"),
            loc_item(idx, "item_description"),
            &mut errors,
        )
        .and_then(|v| match validate_item_description(&v) {
            Ok(()) => Some(v),
            Err(msg) => {
                errors.push(field_error(loc_item(idx, "item_description"), msg, "value_error"));
                None
            }
        });
        let item_price =
            coerce_int(raw.get("item_price"), loc_item(idx, "item_price"), &mut errors);
        let status = coerce_str(raw.get("status"), loc_item(idx, "status"), &mut errors)
            .and_then(|v| match validate_status(&v) {
                Ok(()) => Some(v),
                Err(msg) => {
                    errors.push(field_error(loc_item(idx, "status"), msg, "value_error"));
                    None
                }
            });

        if let (Some(item_id), Some(item_category), Some(item_name), Some(item_description), Some(item_price), Some(status)) =
            (item_id, item_category, item_name, item_description, item_price, status)
        {
            items.push(UpdateItem {
                item_id,
                item_category,
                item_name,
                item_description,
                item_price,
                status,
            });
        }
    }

    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }

    check_length(items.len(), &max_items, |max| {
        format!("You can only update up to {max} items per request.")
    })?;

    Ok(UpdateItems { items })
}

pub fn parse_delete_items(
    body: &[u8],
    max_items: Result<i64, OctyError>,
) -> Result<DeleteItems, ApiError> {
    let root = parse_body_object(body)?;
    let raw_items = items_array(&root)?;

    let mut errors: Vec<Value> = Vec::new();
    let mut items: Vec<String> = Vec::new();

    for (idx, raw) in raw_items.iter().enumerate() {
        if let Some(v) = coerce_str(
            Some(raw),
            vec![json!("body"), json!("items"), json!(idx)],
            &mut errors,
        ) {
            items.push(v);
        }
    }

    if !errors.is_empty() {
        return Err(ApiError::validation(errors));
    }

    check_length(items.len(), &max_items, |max| {
        format!("You can only delete up to {max} items per request.")
    })?;

    Ok(DeleteItems { items })
}

pub fn parse_delete_account_items_internal(
    body: &[u8],
) -> Result<DeleteAccountItemsInternal, ApiError> {
    let root = parse_body_object(body)?;
    let mut errors: Vec<Value> = Vec::new();
    let account_id = coerce_str(
        root.get("account_id"),
        vec![json!("body"), json!("account_id")],
        &mut errors,
    );
    match account_id {
        Some(account_id) if errors.is_empty() => Ok(DeleteAccountItemsInternal { account_id }),
        _ => Err(ApiError::validation(errors)),
    }
}

/// FastAPI/pydantic bool query-param parsing (`ids: bool`).
pub fn parse_query_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_lowercase().as_str() {
        "0" | "off" | "f" | "false" | "n" | "no" => Some(false),
        "1" | "on" | "t" | "true" | "y" | "yes" => Some(true),
        _ => None,
    }
}

/// Shared with the handlers for query-param 422s.
pub fn query_error(name: &str, msg: &str, kind: &str) -> Value {
    field_error(vec![json!("query"), json!(name)], msg, kind)
}
