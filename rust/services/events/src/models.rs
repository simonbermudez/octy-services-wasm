//! Port of `api/routers/request_models/events.py`, `event_types.py` and
//! `data/models/events.py` — pydantic-equivalent request models. Validation
//! failures render the same 422 envelope FastAPI's `RequestValidationError`
//! handler produced (raw `{"loc": …, "msg": …, "type": …}` entries in
//! `error.errors`).

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::http_util::ApiError;

pub fn field_error(loc: Vec<Value>, msg: &str, kind: &str) -> Value {
    json!({ "loc": loc, "msg": msg, "type": kind })
}

fn push_loc(prefix: &[Value], tail: &str) -> Vec<Value> {
    let mut loc = prefix.to_vec();
    loc.push(json!(tail));
    loc
}

fn push_loc_idx(prefix: &[Value], idx: usize) -> Vec<Value> {
    let mut loc = prefix.to_vec();
    loc.push(json!(idx));
    loc
}

/// Parse the raw request body into a JSON object, mirroring FastAPI's
/// json-decode / dict-shape errors.
fn parse_body_object(body: &[u8]) -> Result<Map<String, Value>, ApiError> {
    let value: Value = serde_json::from_slice(body).map_err(|e| {
        ApiError::validation(vec![field_error(
            vec![json!("body")],
            &e.to_string(),
            "value_error.jsondecode",
        )])
    })?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(ApiError::validation(vec![field_error(
            vec![json!("body")],
            "value is not a valid dict",
            "type_error.dict",
        )])),
    }
}

/// pydantic v1 `str` coercion (str, int, float and bool are accepted).
fn coerce_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(if *b { "True" } else { "False" }.to_string()),
        _ => None,
    }
}

/// pydantic v1 `int` coercion.
fn coerce_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        Value::Bool(b) => Some(*b as i64),
        _ => None,
    }
}

fn require_str(
    map: &Map<String, Value>,
    field: &str,
    prefix: &[Value],
    errors: &mut Vec<Value>,
) -> Option<String> {
    match map.get(field) {
        None => {
            errors.push(field_error(push_loc(prefix, field), "field required", "value_error.missing"));
            None
        }
        Some(Value::Null) => {
            errors.push(field_error(
                push_loc(prefix, field),
                "none is not an allowed value",
                "type_error.none.not_allowed",
            ));
            None
        }
        Some(v) => coerce_str(v).or_else(|| {
            errors.push(field_error(push_loc(prefix, field), "str type expected", "type_error.str"));
            None
        }),
    }
}

fn optional_str(
    map: &Map<String, Value>,
    field: &str,
    prefix: &[Value],
    errors: &mut Vec<Value>,
) -> Option<String> {
    match map.get(field) {
        None | Some(Value::Null) => None,
        Some(v) => coerce_str(v).or_else(|| {
            errors.push(field_error(push_loc(prefix, field), "str type expected", "type_error.str"));
            None
        }),
    }
}

fn require_str_list(
    map: &Map<String, Value>,
    field: &str,
    prefix: &[Value],
    errors: &mut Vec<Value>,
) -> Option<Vec<String>> {
    match map.get(field) {
        None => {
            errors.push(field_error(push_loc(prefix, field), "field required", "value_error.missing"));
            None
        }
        Some(Value::Null) => {
            errors.push(field_error(
                push_loc(prefix, field),
                "none is not an allowed value",
                "type_error.none.not_allowed",
            ));
            None
        }
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            let mut ok = true;
            for (i, item) in items.iter().enumerate() {
                match coerce_str(item) {
                    Some(s) => out.push(s),
                    None => {
                        ok = false;
                        errors.push(field_error(
                            push_loc_idx(&push_loc(prefix, field), i),
                            "str type expected",
                            "type_error.str",
                        ));
                    }
                }
            }
            ok.then_some(out)
        }
        Some(_) => {
            errors.push(field_error(
                push_loc(prefix, field),
                "value is not a valid list",
                "type_error.list",
            ));
            None
        }
    }
}

// ---------------------------------------------------------------------------
// events request models
// ---------------------------------------------------------------------------

/// `CreateEvent` — `{event_type, event_properties, profile_id, created_at?}`.
#[derive(Debug, Clone)]
pub struct CreateEvent {
    pub event_type: String,
    pub event_properties: Map<String, Value>,
    pub profile_id: String,
    pub created_at: Option<String>,
}

impl CreateEvent {
    fn from_value(value: &Value, prefix: &[Value], errors: &mut Vec<Value>) -> Option<Self> {
        let Value::Object(map) = value else {
            errors.push(field_error(prefix.to_vec(), "value is not a valid dict", "type_error.dict"));
            return None;
        };

        let event_type = require_str(map, "event_type", prefix, errors);
        let profile_id = require_str(map, "profile_id", prefix, errors);
        let created_at = optional_str(map, "created_at", prefix, errors);
        let event_properties = match map.get("event_properties") {
            None => {
                errors.push(field_error(
                    push_loc(prefix, "event_properties"),
                    "field required",
                    "value_error.missing",
                ));
                None
            }
            Some(Value::Object(props)) => Some(props.clone()),
            Some(_) => {
                errors.push(field_error(
                    push_loc(prefix, "event_properties"),
                    "value is not a valid dict",
                    "type_error.dict",
                ));
                None
            }
        };

        Some(CreateEvent {
            event_type: event_type?,
            event_properties: event_properties?,
            profile_id: profile_id?,
            created_at,
        })
    }

    pub fn from_json(body: &[u8]) -> Result<Self, ApiError> {
        let map = parse_body_object(body)?;
        let mut errors = Vec::new();
        let parsed = Self::from_value(&Value::Object(map), &[json!("body")], &mut errors);
        match parsed {
            Some(event) if errors.is_empty() => Ok(event),
            _ => Err(ApiError::validation(errors)),
        }
    }
}

/// `BatchCreateEvents` — `{events: [CreateEvent, …]}` with the
/// `MAX_CREATE_EVENTS` length validator.
#[derive(Debug, Clone)]
pub struct BatchCreateEvents {
    pub events: Vec<CreateEvent>,
}

impl BatchCreateEvents {
    pub fn from_json(body: &[u8], max_create_events: i64) -> Result<Self, ApiError> {
        let map = parse_body_object(body)?;
        let mut errors = Vec::new();

        let items = match map.get("events") {
            None => {
                errors.push(field_error(
                    vec![json!("body"), json!("events")],
                    "field required",
                    "value_error.missing",
                ));
                None
            }
            Some(Value::Array(items)) => Some(items),
            Some(_) => {
                errors.push(field_error(
                    vec![json!("body"), json!("events")],
                    "value is not a valid list",
                    "type_error.list",
                ));
                None
            }
        };

        let mut events = Vec::new();
        if let Some(items) = items {
            for (i, item) in items.iter().enumerate() {
                let prefix = vec![json!("body"), json!("events"), json!(i)];
                if let Some(event) = CreateEvent::from_value(item, &prefix, &mut errors) {
                    events.push(event);
                }
            }
            // The pydantic list validator only runs once every item is valid.
            if errors.is_empty() && items.len() as i64 > max_create_events {
                errors.push(field_error(
                    vec![json!("body"), json!("events")],
                    &format!("You can only create up to {max_create_events} events per request."),
                    "value_error",
                ));
            }
        }

        if errors.is_empty() {
            Ok(BatchCreateEvents { events })
        } else {
            Err(ApiError::validation(errors))
        }
    }
}

/// `GetEventsInternal` — internal events lookup body.
#[derive(Debug, Clone)]
pub struct GetEventsInternal {
    pub timeframe: i64,
    pub account_id: String,
    pub event_sequence_event: Option<Value>,
    pub profile_ids: Option<Vec<String>>,
    pub event_type: Option<String>,
}

impl GetEventsInternal {
    pub fn from_json(body: &[u8]) -> Result<Self, ApiError> {
        let map = parse_body_object(body)?;
        let mut errors = Vec::new();
        let prefix = [json!("body")];

        let timeframe = match map.get("timeframe") {
            None | Some(Value::Null) => {
                errors.push(field_error(
                    push_loc(&prefix, "timeframe"),
                    "field required",
                    "value_error.missing",
                ));
                None
            }
            Some(v) => coerce_int(v).or_else(|| {
                errors.push(field_error(
                    push_loc(&prefix, "timeframe"),
                    "value is not a valid integer",
                    "type_error.integer",
                ));
                None
            }),
        };
        let account_id = require_str(&map, "account_id", &prefix, &mut errors);

        let event_sequence_event = match map.get("event_sequence_event") {
            None | Some(Value::Null) => None,
            Some(v @ Value::Object(_)) => Some(v.clone()),
            Some(_) => {
                errors.push(field_error(
                    push_loc(&prefix, "event_sequence_event"),
                    "value is not a valid dict",
                    "type_error.dict",
                ));
                None
            }
        };
        let profile_ids = match map.get("profile_ids") {
            None | Some(Value::Null) => None,
            Some(_) => require_str_list(&map, "profile_ids", &prefix, &mut errors),
        };
        let event_type = optional_str(&map, "event_type", &prefix, &mut errors);

        if !errors.is_empty() {
            return Err(ApiError::validation(errors));
        }
        Ok(GetEventsInternal {
            timeframe: timeframe.unwrap_or_default(),
            account_id: account_id.unwrap_or_default(),
            event_sequence_event,
            profile_ids,
            event_type,
        })
    }
}

/// `DeleteEventsInternal` — `{account_id}`.
#[derive(Debug, Clone)]
pub struct DeleteEventsInternal {
    pub account_id: String,
}

impl DeleteEventsInternal {
    pub fn from_json(body: &[u8]) -> Result<Self, ApiError> {
        let map = parse_body_object(body)?;
        let mut errors = Vec::new();
        let account_id = require_str(&map, "account_id", &[json!("body")], &mut errors);
        match account_id {
            Some(account_id) if errors.is_empty() => Ok(DeleteEventsInternal { account_id }),
            _ => Err(ApiError::validation(errors)),
        }
    }
}

// ---------------------------------------------------------------------------
// event-types request models
// ---------------------------------------------------------------------------

/// Python `repr()` of a char inside the "Illegal character(s)" message —
/// single-quoted, except `'` itself which Python renders as `"'"`.
fn py_repr_char(c: char) -> String {
    if c == '\'' {
        "\"'\"".to_string()
    } else {
        format!("'{c}'")
    }
}

/// The `CreateEventTypesChild.validate_event_type` pydantic validator.
fn validate_event_type_name(value: &str) -> Result<(), String> {
    let char_count = value.chars().count();
    if char_count > 60 || char_count < 1 {
        return Err(
            "Event types must be at least 1 character long and less than 60 characters long."
                .to_string(),
        );
    }
    let disallowed = [',', '"', '\'', '.', ' '];
    let found: Vec<String> = disallowed
        .iter()
        .filter(|c| value.contains(**c))
        .map(|c| py_repr_char(*c))
        .collect();
    if !found.is_empty() {
        return Err(format!(
            "Illegal character(s) found in provided event type : [{}]",
            found.join(", ")
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct CreateEventTypesChild {
    pub event_type: String,
    pub event_properties: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CreateEventTypes {
    pub event_types: Vec<CreateEventTypesChild>,
}

impl CreateEventTypes {
    pub fn from_json(body: &[u8], max_create_event_types: i64) -> Result<Self, ApiError> {
        let map = parse_body_object(body)?;
        let mut errors = Vec::new();

        let items = match map.get("event_types") {
            None => {
                errors.push(field_error(
                    vec![json!("body"), json!("event_types")],
                    "field required",
                    "value_error.missing",
                ));
                None
            }
            Some(Value::Array(items)) => Some(items),
            Some(_) => {
                errors.push(field_error(
                    vec![json!("body"), json!("event_types")],
                    "value is not a valid list",
                    "type_error.list",
                ));
                None
            }
        };

        let mut event_types = Vec::new();
        if let Some(items) = items {
            for (i, item) in items.iter().enumerate() {
                let prefix = vec![json!("body"), json!("event_types"), json!(i)];
                let Value::Object(child) = item else {
                    errors.push(field_error(prefix, "value is not a valid dict", "type_error.dict"));
                    continue;
                };
                let event_type = require_str(child, "event_type", &prefix, &mut errors);
                let event_type = event_type.and_then(|name| match validate_event_type_name(&name) {
                    Ok(()) => Some(name),
                    Err(msg) => {
                        errors.push(field_error(push_loc(&prefix, "event_type"), &msg, "value_error"));
                        None
                    }
                });
                let event_properties = require_str_list(child, "event_properties", &prefix, &mut errors);
                if let (Some(event_type), Some(event_properties)) = (event_type, event_properties) {
                    event_types.push(CreateEventTypesChild {
                        event_type,
                        event_properties,
                    });
                }
            }
            if errors.is_empty() && items.len() as i64 > max_create_event_types {
                errors.push(field_error(
                    vec![json!("body"), json!("event_types")],
                    &format!(
                        "You can only create up to {max_create_event_types} custom event types per request."
                    ),
                    "value_error",
                ));
            }
        }

        if errors.is_empty() {
            Ok(CreateEventTypes { event_types })
        } else {
            Err(ApiError::validation(errors))
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeleteEventTypes {
    pub event_type_ids: Vec<String>,
}

impl DeleteEventTypes {
    pub fn from_json(body: &[u8], max_delete_event_types: i64) -> Result<Self, ApiError> {
        let map = parse_body_object(body)?;
        let mut errors = Vec::new();
        let ids = require_str_list(&map, "event_type_ids", &[json!("body")], &mut errors);
        if let Some(ids) = &ids {
            if errors.is_empty() && ids.len() as i64 > max_delete_event_types {
                errors.push(field_error(
                    vec![json!("body"), json!("event_type_ids")],
                    &format!(
                        "You can only delete up to {max_delete_event_types} custom event types per request."
                    ),
                    "value_error",
                ));
            }
        }
        match ids {
            Some(event_type_ids) if errors.is_empty() => Ok(DeleteEventTypes { event_type_ids }),
            _ => Err(ApiError::validation(errors)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GetEventTypesInternal {
    pub account_id: String,
    pub event_type_names: Vec<String>,
}

impl GetEventTypesInternal {
    pub fn from_json(body: &[u8]) -> Result<Self, ApiError> {
        let map = parse_body_object(body)?;
        let mut errors = Vec::new();
        let prefix = [json!("body")];
        let account_id = require_str(&map, "account_id", &prefix, &mut errors);
        let event_type_names = require_str_list(&map, "event_type_names", &prefix, &mut errors);
        match (account_id, event_type_names) {
            (Some(account_id), Some(event_type_names)) if errors.is_empty() => Ok(GetEventTypesInternal {
                account_id,
                event_type_names,
            }),
            _ => Err(ApiError::validation(errors)),
        }
    }
}

// ---------------------------------------------------------------------------
// AMQP payload models (`data/models/events.py`) — parse failures reject the
// delivery without requeue, so plain serde is sufficient.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateEventsOwnerChild {
    pub parent_profile: String,
    pub child_profiles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateEventsOwner {
    pub account_id: String,
    pub profiles: Vec<UpdateEventsOwnerChild>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteProfiles {
    pub account_id: String,
    pub profile_id: String,
}
