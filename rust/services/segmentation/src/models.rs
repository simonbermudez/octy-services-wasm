//! Port of `segmentation/api/routers/request_models/segmentation.py` and
//! `segmentation/data/models/segments.py` — request bodies with
//! pydantic-v1-equivalent validation.
//!
//! Validation failures return the raw pydantic-style error dicts
//! (`{"loc": [...], "msg": ..., "type": ...}`) so the 422 envelope carries
//! the exact objects FastAPI's `RequestValidationError` handler emitted.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

// ---------------------------------------------------------------------------
// pydantic-style error helpers
// ---------------------------------------------------------------------------

pub fn field_error(loc: Vec<Value>, msg: &str, kind: &str) -> Value {
    json!({ "loc": loc, "msg": msg, "type": kind })
}

fn missing(loc: Vec<Value>) -> Value {
    field_error(loc, "field required", "value_error.missing")
}

fn none_not_allowed(loc: Vec<Value>) -> Value {
    field_error(loc, "none is not an allowed value", "type_error.none.not_allowed")
}

/// pydantic v1 `str` coercion: str as-is; int/float/bool via Python `str()`.
fn coerce_str(loc: Vec<Value>, value: &Value) -> Result<String, Value> {
    match value {
        Value::String(s) => Ok(s.clone()),
        // bool is an int subclass in Python: str(True) == "True"
        Value::Bool(b) => Ok(if *b { "True".into() } else { "False".into() }),
        Value::Number(n) => Ok(n.to_string()),
        Value::Null => Err(none_not_allowed(loc)),
        _ => Err(field_error(loc, "str type expected", "type_error.str")),
    }
}

/// pydantic v1 `int` coercion: `int(v)` semantics.
fn coerce_int(loc: Vec<Value>, value: &Value) -> Result<i64, Value> {
    let err = || field_error(loc.clone(), "value is not a valid integer", "type_error.integer");
    match value {
        Value::Bool(b) => Ok(if *b { 1 } else { 0 }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i)
            } else if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.is_finite() {
                    Ok(f as i64)
                } else {
                    Err(err())
                }
            } else {
                Err(err())
            }
        }
        Value::String(s) => s.trim().parse::<i64>().map_err(|_| err()),
        Value::Null => Err(none_not_allowed(loc)),
        _ => Err(err()),
    }
}

/// Parse a request body into a JSON object, mirroring FastAPI's body errors.
pub fn parse_body_object(body: &[u8]) -> Result<Map<String, Value>, Vec<Value>> {
    let value: Value = serde_json::from_slice(body).map_err(|e| {
        vec![field_error(
            vec![json!("body")],
            &e.to_string(),
            "value_error.jsondecode",
        )]
    })?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(vec![field_error(
            vec![json!("body")],
            "value is not a valid dict",
            "type_error.dict",
        )]),
    }
}

// ---------------------------------------------------------------------------
// CreateSegment (`request_models/segmentation.py`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct EventSequenceEvent {
    pub exp_timeframe: i64,
    pub action_inaction: String,
    pub event_type: String,
    /// `Optional[dict]`; the pydantic validator turns `{}` into `None`.
    pub event_properties: Option<Map<String, Value>>,
}

impl EventSequenceEvent {
    /// `{'exp_timeframe': …, 'action_inaction': …, 'event_type': …,
    ///  'event_properties': …}` — the shape `es.dict()` produced (used for
    /// Mongo storage and the duplicate-segment comparison).
    pub fn to_dict(&self) -> Value {
        json!({
            "exp_timeframe": self.exp_timeframe,
            "action_inaction": self.action_inaction,
            "event_type": self.event_type,
            "event_properties": self.event_properties,
        })
    }

    fn parse(loc_prefix: Vec<Value>, value: &Value, errors: &mut Vec<Value>) -> Option<Self> {
        let loc = |field: &str| {
            let mut l = loc_prefix.clone();
            l.push(json!(field));
            l
        };
        let Value::Object(obj) = value else {
            errors.push(field_error(loc_prefix.clone(), "value is not a valid dict", "type_error.dict"));
            return None;
        };
        let mut ok = true;

        let exp_timeframe = match obj.get("exp_timeframe") {
            None => {
                errors.push(missing(loc("exp_timeframe")));
                ok = false;
                0
            }
            Some(v) => match coerce_int(loc("exp_timeframe"), v) {
                Ok(i) => i,
                Err(e) => {
                    errors.push(e);
                    ok = false;
                    0
                }
            },
        };

        let action_inaction = match obj.get("action_inaction") {
            None => {
                errors.push(missing(loc("action_inaction")));
                ok = false;
                String::new()
            }
            Some(v) => match coerce_str(loc("action_inaction"), v) {
                Ok(s) => {
                    // @validator('action_inaction') allowed_status
                    if s != "action" && s != "inaction" {
                        errors.push(field_error(
                            loc("action_inaction"),
                            "Invalid event provided. Please ensure 'action_inaction' parameter is either 'action' or 'inaction'",
                            "value_error",
                        ));
                        ok = false;
                    }
                    s
                }
                Err(e) => {
                    errors.push(e);
                    ok = false;
                    String::new()
                }
            },
        };

        let event_type = match obj.get("event_type") {
            None => {
                errors.push(missing(loc("event_type")));
                ok = false;
                String::new()
            }
            Some(v) => match coerce_str(loc("event_type"), v) {
                Ok(s) => s,
                Err(e) => {
                    errors.push(e);
                    ok = false;
                    String::new()
                }
            },
        };

        // Optional[dict]; type errors happen before the custom validator, so
        // the "single key value pair object" ValueError is unreachable — the
        // reachable behavior is: {} → None, non-dict → type_error.dict.
        let event_properties = match obj.get("event_properties") {
            None | Some(Value::Null) => None,
            Some(Value::Object(map)) => {
                if map.is_empty() {
                    None
                } else {
                    Some(map.clone())
                }
            }
            Some(_) => {
                errors.push(field_error(
                    loc("event_properties"),
                    "value is not a valid dict",
                    "type_error.dict",
                ));
                ok = false;
                None
            }
        };

        if ok {
            Some(Self {
                exp_timeframe,
                action_inaction,
                event_type,
                event_properties,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateSegment {
    pub segment_name: String,
    pub segment_type: String,
    pub segment_sub_type: i64,
    pub segment_timeframe: i64,
    pub event_sequence: Vec<EventSequenceEvent>,
    pub profile_property_name: Option<String>,
    pub profile_property_value: Option<Value>,
}

/// Python `repr()` of a char, as used inside the segment-name error message
/// (list of found characters, e.g. `[',', '.']`).
fn py_char_repr(c: char) -> String {
    if c == '\'' {
        "\"'\"".to_string()
    } else {
        format!("'{c}'")
    }
}

impl CreateSegment {
    pub fn from_json(body: &[u8]) -> Result<Self, Vec<Value>> {
        let obj = parse_body_object(body)?;
        let mut errors: Vec<Value> = Vec::new();
        let loc = |field: &str| vec![json!("body"), json!(field)];

        let segment_name = match obj.get("segment_name") {
            None => {
                errors.push(missing(loc("segment_name")));
                String::new()
            }
            Some(v) => match coerce_str(loc("segment_name"), v) {
                Ok(s) => {
                    // @validator('segment_name') validate_segment_name
                    if s.chars().count() > 60 || s.chars().count() < 1 {
                        errors.push(field_error(
                            loc("segment_name"),
                            "Segment names must be at least 1 character long and less than 60 characters long.",
                            "value_error",
                        ));
                    } else {
                        let disallowed = [',', '"', '\'', '.', ' '];
                        let found: Vec<char> =
                            disallowed.iter().copied().filter(|c| s.contains(*c)).collect();
                        if !found.is_empty() {
                            let listed = found
                                .iter()
                                .map(|c| py_char_repr(*c))
                                .collect::<Vec<_>>()
                                .join(", ");
                            errors.push(field_error(
                                loc("segment_name"),
                                &format!("Illegal character(s) found in provided segment name : [{listed}]"),
                                "value_error",
                            ));
                        }
                    }
                    s
                }
                Err(e) => {
                    errors.push(e);
                    String::new()
                }
            },
        };

        let segment_type = match obj.get("segment_type") {
            None => {
                errors.push(missing(loc("segment_type")));
                String::new()
            }
            Some(v) => coerce_str(loc("segment_type"), v).unwrap_or_else(|e| {
                errors.push(e);
                String::new()
            }),
        };

        let segment_sub_type = match obj.get("segment_sub_type") {
            None => {
                errors.push(missing(loc("segment_sub_type")));
                0
            }
            Some(v) => coerce_int(loc("segment_sub_type"), v).unwrap_or_else(|e| {
                errors.push(e);
                0
            }),
        };

        let segment_timeframe = match obj.get("segment_timeframe") {
            None => {
                errors.push(missing(loc("segment_timeframe")));
                0
            }
            Some(v) => coerce_int(loc("segment_timeframe"), v).unwrap_or_else(|e| {
                errors.push(e);
                0
            }),
        };

        let mut event_sequence = Vec::new();
        match obj.get("event_sequence") {
            None => errors.push(missing(loc("event_sequence"))),
            Some(Value::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    let loc_prefix = vec![json!("body"), json!("event_sequence"), json!(i)];
                    if let Some(event) = EventSequenceEvent::parse(loc_prefix, item, &mut errors) {
                        event_sequence.push(event);
                    }
                }
            }
            Some(_) => errors.push(field_error(
                loc("event_sequence"),
                "value is not a valid list",
                "type_error.list",
            )),
        }

        let profile_property_name = match obj.get("profile_property_name") {
            None | Some(Value::Null) => None,
            Some(v) => match coerce_str(loc("profile_property_name"), v) {
                Ok(s) => Some(s),
                Err(e) => {
                    errors.push(e);
                    None
                }
            },
        };

        // Optional[Any] with a validator that runs only when the field is
        // supplied. Quirk kept from the Python: an *explicit* null fails the
        // `type(value) not in [int, str, bool, float]` check (despite the
        // message saying "or null"); omitting the field is fine.
        let profile_property_value = match obj.get("profile_property_value") {
            None => None,
            Some(v) => {
                let allowed = matches!(v, Value::Number(_) | Value::String(_) | Value::Bool(_));
                if !allowed {
                    errors.push(field_error(
                        loc("profile_property_value"),
                        "The 'profile_property_value' parameter must be of type: int or str or bool or float or null.",
                        "value_error",
                    ));
                    None
                } else {
                    Some(v.clone())
                }
            }
        };

        if errors.is_empty() {
            Ok(Self {
                segment_name,
                segment_type,
                segment_sub_type,
                segment_timeframe,
                event_sequence,
                profile_property_name,
                profile_property_value,
            })
        } else {
            Err(errors)
        }
    }

    pub fn profile_property_value_or_null(&self) -> Value {
        self.profile_property_value.clone().unwrap_or(Value::Null)
    }
}

// ---------------------------------------------------------------------------
// DeleteSegments (`request_models/segmentation.py`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DeleteSegments {
    pub segments: Vec<String>,
}

impl DeleteSegments {
    pub fn from_json(body: &[u8], max_delete_segments: i64) -> Result<Self, Vec<Value>> {
        let obj = parse_body_object(body)?;
        let mut errors: Vec<Value> = Vec::new();
        let mut segments = Vec::new();

        match obj.get("segments") {
            None => errors.push(missing(vec![json!("body"), json!("segments")])),
            Some(Value::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    match coerce_str(vec![json!("body"), json!("segments"), json!(i)], item) {
                        Ok(s) => segments.push(s),
                        Err(e) => errors.push(e),
                    }
                }
                // @validator('segments') length
                if errors.is_empty() && (segments.len() as i64) > max_delete_segments {
                    errors.push(field_error(
                        vec![json!("body"), json!("segments")],
                        &format!("You can only delete up to {max_delete_segments} segments per request."),
                        "value_error",
                    ));
                }
            }
            Some(_) => errors.push(field_error(
                vec![json!("body"), json!("segments")],
                "value is not a valid list",
                "type_error.list",
            )),
        }

        if errors.is_empty() {
            Ok(Self { segments })
        } else {
            Err(errors)
        }
    }
}

// ---------------------------------------------------------------------------
// DeleteAccountSegmentations (`request_models/segmentation.py`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DeleteAccountSegmentations {
    pub account_id: String,
}

impl DeleteAccountSegmentations {
    pub fn from_json(body: &[u8]) -> Result<Self, Vec<Value>> {
        let obj = parse_body_object(body)?;
        match obj.get("account_id") {
            None => Err(vec![missing(vec![json!("body"), json!("account_id")])]),
            Some(v) => match coerce_str(vec![json!("body"), json!("account_id")], v) {
                Ok(account_id) => Ok(Self { account_id }),
                Err(e) => Err(vec![e]),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// UpdatePastSegementProfiles (`data/models/segments.py`, consumed from AMQP)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePastSegementProfilesChild {
    pub parent_profile: String,
    pub child_profiles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePastSegementProfiles {
    pub account_id: String,
    pub profiles: Vec<UpdatePastSegementProfilesChild>,
}
