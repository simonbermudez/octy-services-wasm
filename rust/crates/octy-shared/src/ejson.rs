//! Mongo "legacy extended JSON" helpers.
//!
//! The Python services cache Mongo documents in Redis with
//! `bson.json_util.dumps(...)`, which uses the *legacy* extended JSON
//! encoding: `ObjectId` → `{"$oid": "<hex>"}` and `datetime` →
//! `{"$date": <epoch millis int>}`. Downstream code (and the auth JWT) reads
//! those shapes directly, so the Rust port produces the identical encoding.

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

/// `{"$date": <epoch millis>}`
pub fn legacy_date(dt: DateTime<Utc>) -> Value {
    json!({ "$date": dt.timestamp_millis() })
}

pub fn now_legacy_date() -> Value {
    legacy_date(Utc::now())
}

/// Extract epoch millis from either a legacy `{"$date": int}` or canonical
/// `{"$date": {"$numberLong": "…"}}` / `{"$date": "<iso8601>"}` value.
pub fn date_millis(value: &Value) -> Option<i64> {
    let inner = value.get("$date")?;
    if let Some(ms) = inner.as_i64() {
        return Some(ms);
    }
    if let Some(nl) = inner.get("$numberLong").and_then(Value::as_str) {
        return nl.parse().ok();
    }
    if let Some(iso) = inner.as_str() {
        return DateTime::parse_from_rfc3339(iso)
            .ok()
            .map(|d| d.timestamp_millis());
    }
    None
}

/// Extract the hex string from `{"$oid": "…"}`.
pub fn oid_hex(value: &Value) -> Option<&str> {
    value.get("$oid").and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn legacy_date_shape() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        let v = legacy_date(dt);
        assert_eq!(v["$date"], dt.timestamp_millis());
        assert_eq!(date_millis(&v), Some(dt.timestamp_millis()));
    }

    #[test]
    fn reads_canonical_forms() {
        let v = json!({"$date": {"$numberLong": "1750000000000"}});
        assert_eq!(date_millis(&v), Some(1750000000000));
        let v = json!({"$date": "2026-01-02T03:04:05Z"});
        assert!(date_millis(&v).is_some());
    }
}
