//! Port of `account/config.py` / `app_secrets.py`.
//!
//! The Python services load their whole configuration from a single
//! base64-encoded JSON blob in an environment variable (e.g. `ACCOUNT_CONFIG`).
//! The Rust services keep that contract so existing Kubernetes secrets and
//! config maps work unchanged.

use serde_json::Value;

use crate::errors::OctyError;
use crate::utils::base64_decode_json;

/// A parsed service configuration blob with typed accessors.
#[derive(Debug, Clone)]
pub struct Config {
    root: Value,
}

impl Config {
    /// Parse a config blob as delivered in the env var: base64-encoded JSON,
    /// with a fallback to plain JSON (mirrors the Python `try/except TypeError`).
    pub fn from_encoded(encoded: &str) -> Result<Self, OctyError> {
        let root = base64_decode_json(encoded)
            .map_err(|e| OctyError::internal(format!("invalid config blob: {e}")))?;
        Ok(Self { root })
    }

    pub fn from_value(root: Value) -> Self {
        Self { root }
    }

    pub fn raw(&self) -> &Value {
        &self.root
    }

    /// `Config['KEY']` equivalent. Missing keys are a configuration error.
    pub fn get(&self, key: &str) -> Result<&Value, OctyError> {
        self.root
            .get(key)
            .ok_or_else(|| OctyError::internal(format!("missing config key: {key}")))
    }

    pub fn get_str(&self, key: &str) -> Result<&str, OctyError> {
        self.get(key)?
            .as_str()
            .ok_or_else(|| OctyError::internal(format!("config key {key} is not a string")))
    }

    pub fn get_i64(&self, key: &str) -> Result<i64, OctyError> {
        self.get(key)?
            .as_i64()
            .ok_or_else(|| OctyError::internal(format!("config key {key} is not an integer")))
    }

    pub fn get_array(&self, key: &str) -> Result<&Vec<Value>, OctyError> {
        self.get(key)?
            .as_array()
            .ok_or_else(|| OctyError::internal(format!("config key {key} is not an array")))
    }

    /// String value that may be missing — returns `None` instead of erroring.
    pub fn opt_str(&self, key: &str) -> Option<&str> {
        self.root.get(key).and_then(Value::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn parses_base64_and_plain_json() {
        let json = r#"{"ENV":"dev","REDIS_PORT":6379}"#;
        let b64 = base64::engine::general_purpose::STANDARD.encode(json);

        for blob in [b64.as_str(), json] {
            let cfg = Config::from_encoded(blob).unwrap();
            assert_eq!(cfg.get_str("ENV").unwrap(), "dev");
            assert_eq!(cfg.get_i64("REDIS_PORT").unwrap(), 6379);
            assert!(cfg.get("MISSING").is_err());
        }
    }
}
