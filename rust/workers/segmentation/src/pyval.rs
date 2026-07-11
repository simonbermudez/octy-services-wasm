//! Pure helpers replicating the Python value semantics the segmentation
//! engine relies on: `==` on arbitrary JSON values and `strconv.convert`
//! type inference. No spin-sdk imports — natively testable.

use serde_json::Value;

/// Python-style `==` for JSON values: numbers and bools compare numerically
/// (`5 == 5.0`, `True == 1`), containers recurse, `None == None`, and
/// mismatched types are unequal (never an error).
pub fn json_eq(a: &Value, b: &Value) -> bool {
    fn as_num(v: &Value) -> Option<f64> {
        match v {
            Value::Number(n) => n.as_f64(),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(l, r)| json_eq(l, r))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter().all(|(k, v)| y.get(k).map_or(false, |w| json_eq(v, w)))
        }
        _ => match (as_num(a), as_num(b)) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
    }
}

/// Port of `strconv.convert` for the subset `_property_evaluation` uses:
/// strings are inferred to int, then float, then bool; unconvertible strings
/// stay strings; non-strings pass through unchanged.
///
/// DIVERGENCE (documented): strconv's `time`/`date`/`datetime` converters are
/// not ported. In Python a date-like string became a `datetime` object that
/// could never equal a JSON profile value (the comparison always failed); here
/// the raw string is kept, so a profile property stored as the same string
/// now *matches*.
pub fn strconv_convert(v: &Value) -> Value {
    let Value::String(s) = v else { return v.clone() };
    let t = s.trim();
    if let Ok(i) = t.parse::<i64>() {
        return Value::from(i);
    }
    if let Ok(f) = t.parse::<f64>() {
        if f.is_finite() {
            if let Some(n) = serde_json::Number::from_f64(f) {
                return Value::Number(n);
            }
        }
    }
    // strconv's boolean converter (true/yes/on — false/no/off, case-insensitive).
    match t.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" => return Value::Bool(true),
        "false" | "no" | "off" => return Value::Bool(false),
        _ => {}
    }
    v.clone()
}

/// `value == "s"` where the Python compared a JSON value to a str literal.
pub fn v_is(v: &Value, s: &str) -> bool {
    v.as_str() == Some(s)
}
