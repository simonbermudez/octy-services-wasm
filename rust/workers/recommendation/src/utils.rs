//! Crate-local port of `utils/utils.py` pieces the shared crate does not
//! cover, plus the Python value/size formatting quirks the pipeline relies on.
//!
//! NB: `octy_shared::utils::generate_uid` lacks the `hp-t-job` /
//! `training-job` formatting entries this worker needs, so the full Python
//! `uid_formatting` table is reproduced here.

use serde_json::Value;
use uuid::Uuid;

/// `generate_uid` — exact port of the worker's formatting table.
pub fn generate_uid(prefix: &str) -> String {
    let (len, separator) = match prefix {
        "bucket" => (27, '-'),
        "training-job" => (22, '-'),
        "hp-t-job" => (22, '-'),
        "notification" => (20, '-'),
        _ => (34, '_'),
    };
    let uuid = Uuid::new_v4().to_string();
    let tail: String = uuid.chars().take(len).collect();
    format!("{prefix}{separator}{tail}")
}

/// `str(datetime.now())` — used in webhook payloads (`date_time` field).
pub fn now_str() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}

/// `sys.getsizeof(str)` approximation for an ASCII `str` (49-byte header).
/// The Python used it both for billing data units and the upload size
/// thresholds, so the +49 is kept for fidelity around the boundaries.
pub fn py_sizeof_str(s: &str) -> i64 {
    49 + s.len() as i64
}

/// `_get_size({'data': …, 'type': …})` approximation from
/// `services/billing.py` — dict header + key strings + value strings.
pub fn py_sizeof_csv_object(data: &str, type_: &str) -> i64 {
    232 + py_sizeof_str(data) + py_sizeof_str(type_) + py_sizeof_str("data") + py_sizeof_str("type")
}

/// `_bytes_to_metric` from `services/billing.py` (Python `round` — banker's
/// rounding; `round_ties_even` matches).
pub fn bytes_to_metric(bytes: i64, metric: &str) -> anyhow::Result<i64> {
    let divisor: f64 = match metric {
        "KB" => 1_000.0,
        "MB" => 1_000_000.0,
        "GB" => 1_000_000_000.0,
        "TB" => 1_000_000_000_000.0,
        _ => anyhow::bail!("Unknown data metric specified: {metric}"),
    };
    Ok((bytes as f64 / divisor).round_ties_even() as i64)
}

/// `_required_gb` — walks bytes→KB→MB→GB→TB in steps of 1000. Anything at or
/// beyond 1000 TB fell off the end of the Python loop and returned `None`
/// (which then crashed the SageMaker call) — surfaced here as an error.
pub fn required_gb(num_bytes: i64) -> anyhow::Result<i64> {
    let mut num_bytes = num_bytes as f64;
    let step_unit = 1000.0;
    for unit in ["bytes", "KB", "MB", "GB", "TB"] {
        if num_bytes < step_unit {
            return Ok(match unit {
                // Add 1 to ensure rounding doesn't create a memory discrepancy
                "GB" => num_bytes as i64 + 1,
                "bytes" | "KB" | "MB" => 1,
                // there are 1000 GB in 1 TB
                "TB" => (num_bytes * 1000.0) as i64 + 1,
                _ => unreachable!(),
            });
        }
        num_bytes /= step_unit;
    }
    anyhow::bail!("could not determine required training volume size (>= 1000 TB)")
}

/// Render a JSON value the way `str(python_object)` / pandas `to_csv` would
/// print a cell: `True`/`False` bools, empty string for null/NaN, floats with
/// a trailing `.0`, and Python `repr` style for nested lists/dicts.
pub fn py_cell_str(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => (if *b { "True" } else { "False" }).to_string(),
        Value::String(s) => s.clone(),
        Value::Number(_) => value.to_string(),
        Value::Array(_) | Value::Object(_) => py_repr(value),
    }
}

/// Python `repr()` of the JSON value (single-quoted strings, True/False/None)
/// — what pandas would have written for dict/list cells.
pub fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(b) => (if *b { "True" } else { "False" }).to_string(),
        Value::Number(_) => value.to_string(),
        Value::String(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("'{}': {}", k.replace('\\', "\\\\").replace('\'', "\\'"), py_repr(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

/// Numeric-aware equality for merge keys (pandas matches `1 == 1.0`).
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => x == y,
        },
        _ => a == b,
    }
}
