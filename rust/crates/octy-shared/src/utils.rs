//! Port of `account/utils/utils.py`.

use base64::Engine;
use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use serde_json::Value;
use uuid::Uuid;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// `dt_to_int` — datetime encoded as the integer `YYYYMMDDHHMMSS`.
/// (This is what the auth JWT uses for `iat`/`exp`.)
pub fn dt_to_int(dt: DateTime<Utc>) -> i64 {
    (dt.year() as i64) * 10_000_000_000
        + (dt.month() as i64) * 100_000_000
        + (dt.day() as i64) * 1_000_000
        + (dt.hour() as i64) * 10_000
        + (dt.minute() as i64) * 100
        + dt.second() as i64
}

/// `int_to_dt` — epoch **milliseconds** to datetime (the Python divides by 1e3).
pub fn int_to_dt(millis: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_millis_opt(millis).single()
}

/// `str_to_dt` — parses `%Y-%m-%dT%H:%M:%S.%f`.
pub fn str_to_dt(s: &str) -> Option<DateTime<Utc>> {
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|naive| Utc.from_utc_datetime(&naive))
}

/// `generate_uid` — prefixed UID with service-specific formatting rules.
pub fn generate_uid(prefix: &str) -> String {
    let (len, separator) = match prefix {
        "bucket" => (27, '-'),
        "training-job" => (22, '-'),
        "notification" => (20, '-'),
        _ => (34, '_'),
    };
    let uuid = Uuid::new_v4().to_string();
    let tail: String = uuid.chars().take(len).collect();
    format!("{prefix}{separator}{tail}")
}

/// `basic_auth_parse` — decodes a Basic Authorization header, with the same
/// quirk as the Python: a `Bearer pk:sk` token is accepted un-encoded.
///
/// Returns `(ok, username, password)`.
pub fn basic_auth_parse(token: &str) -> (bool, String, String) {
    if token.is_empty() {
        return (false, String::new(), String::new());
    }

    if token.contains("Bearer") {
        let raw = if token.len() > 7 { &token[7..] } else { "" };
        let mut parts = raw.splitn(2, ':');
        let username = parts.next().unwrap_or("").to_string();
        let password = parts.next().unwrap_or("").to_string();
        return (true, username, password);
    }

    // basicauth.decode: strip optional "Basic ", base64-decode, split on ':'
    let encoded = token.strip_prefix("Basic ").unwrap_or(token).trim();
    let decoded = match B64.decode(encoded.as_bytes()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return (false, String::new(), String::new()),
        },
        Err(_) => return (false, String::new(), String::new()),
    };
    let mut parts = decoded.splitn(2, ':');
    let username = parts.next().unwrap_or("").to_string();
    let password = match parts.next() {
        Some(p) => p.to_string(),
        None => return (false, String::new(), String::new()),
    };
    (true, username, password)
}

/// `base64_encode_json` — minified JSON, base64-encoded.
pub fn base64_encode_json(value: &Value) -> String {
    B64.encode(serde_json::to_string(value).expect("serializable json").as_bytes())
}

/// `base64_decode(..., is_json=True)` — falls back to treating the input as
/// plain JSON when it is not valid base64 (matches the Python behaviour).
pub fn base64_decode_json(input: &str) -> Result<Value, serde_json::Error> {
    if let Ok(bytes) = B64.decode(input.trim().as_bytes()) {
        if let Ok(text) = String::from_utf8(bytes) {
            if let Ok(v) = serde_json::from_str(&text) {
                return Ok(v);
            }
        }
    }
    serde_json::from_str(input)
}

/// `base64_decode(..., is_json=False)` — plain string payloads (e.g. PEM keys),
/// falling back to the raw input when it is not base64.
pub fn base64_decode_str(input: &str) -> String {
    if let Ok(bytes) = B64.decode(input.trim().as_bytes()) {
        if let Ok(text) = String::from_utf8(bytes) {
            return text;
        }
    }
    input.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dt_to_int_matches_strftime() {
        let dt = Utc.with_ymd_and_hms(2026, 7, 9, 14, 3, 5).unwrap();
        assert_eq!(dt_to_int(dt), 20260709140305);
    }

    #[test]
    fn generate_uid_formats() {
        let bucket = generate_uid("bucket");
        assert!(bucket.starts_with("bucket-"));
        assert_eq!(bucket.len(), "bucket-".len() + 27);

        let account = generate_uid("account");
        assert!(account.starts_with("account_"));
        assert_eq!(account.len(), "account_".len() + 34);
    }

    #[test]
    fn basic_auth_parse_base64() {
        let token = format!("Basic {}", B64.encode(b"pk_123:sk_456"));
        let (ok, user, pass) = basic_auth_parse(&token);
        assert!(ok);
        assert_eq!(user, "pk_123");
        assert_eq!(pass, "sk_456");
    }

    #[test]
    fn basic_auth_parse_bearer_quirk() {
        let (ok, user, pass) = basic_auth_parse("Bearer pk_123:sk_456");
        assert!(ok);
        assert_eq!(user, "pk_123");
        assert_eq!(pass, "sk_456");

        let (ok, user, pass) = basic_auth_parse("Bearer pk_only");
        assert!(ok);
        assert_eq!(user, "pk_only");
        assert_eq!(pass, "");
    }

    #[test]
    fn basic_auth_parse_rejects_garbage() {
        let (ok, ..) = basic_auth_parse("");
        assert!(!ok);
        let (ok, ..) = basic_auth_parse("Basic not-base64!!!");
        assert!(!ok);
    }

    #[test]
    fn base64_json_roundtrip() {
        let value = serde_json::json!({"a": 1, "b": ["x"]});
        let encoded = base64_encode_json(&value);
        assert_eq!(base64_decode_json(&encoded).unwrap(), value);
        // plain JSON fallback
        assert_eq!(base64_decode_json(r#"{"a":1,"b":["x"]}"#).unwrap(), value);
    }
}
