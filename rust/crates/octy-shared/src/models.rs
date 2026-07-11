//! Port of `account/api/routers/request_models/account.py` and
//! `account/data/models/account.py` — request bodies with pydantic-equivalent
//! validation. Validation failures render the same 422 envelope FastAPI's
//! `RequestValidationError` handler produced.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::errors::{ErrorReason, OctyError};

/// pydantic-style error entry: `{"loc": [...], "msg": ..., "type": ...}`.
fn field_error(loc: &[&str], msg: &str, kind: &str) -> Value {
    json!({ "loc": loc, "msg": msg, "type": kind })
}

/// Build the 422 error FastAPI produced for invalid request bodies.
pub fn validation_error(errors: Vec<Value>) -> OctyError {
    OctyError {
        code: 422,
        error_description: "Missing or invalid JSON parameters".to_string(),
        reasons: errors
            .into_iter()
            .map(|e| ErrorReason::new(e.to_string(), ""))
            .collect(),
    }
}

/// Same normalization the `email_validator` package applies for the common
/// case: syntax check + lowercased domain.
pub fn validate_email(email: &str) -> Result<String, String> {
    let email = email.trim();
    let (local, domain) = email
        .split_once('@')
        .ok_or_else(|| "Invalid contact email address provided.".to_string())?;
    let re_local = regex_lite::Regex::new(r"^[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+$").unwrap();
    let re_domain =
        regex_lite::Regex::new(r"^[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?(\.[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?)+$")
            .unwrap();
    if local.is_empty() || !re_local.is_match(local) || !re_domain.is_match(domain) {
        return Err("Invalid contact email address provided.".to_string());
    }
    Ok(format!("{local}@{}", domain.to_lowercase()))
}

/// pydantic `HttpUrl`: http/https scheme with a host.
pub fn validate_http_url(raw: &str) -> Result<String, String> {
    let parsed = url::Url::parse(raw).map_err(|_| "invalid or missing URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("URL scheme not permitted".to_string());
    }
    if parsed.host_str().is_none() {
        return Err("URL host invalid".to_string());
    }
    Ok(raw.to_string())
}

pub const ALLOWED_ACCOUNT_TYPES: &[&str] = &["startup", "pro", "enterprise"];
pub const ALLOWED_PERMISSIONS: &[&str] = &["rec", "churn", "rfm", "seg", "mes"];
pub const ALLOWED_PLATFORM_TYPES: &[&str] = &[
    "shopify",
    "woocommerce",
    "bigcommerce",
    "magento",
    "prestashop",
    "squarespace",
    "custom",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedPlatform {
    pub platform_type: String,
    pub store_url: String,
    pub store_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccount {
    pub contact_email_address: String,
    pub account_name: String,
    pub account_type: String,
    #[serde(default)]
    pub authenticated_id_key: Option<String>,
    pub account_currency: String,
    pub contact_name: String,
    pub contact_surname: String,
    pub webhook_url: String,
    pub permissions: Vec<String>,
    #[serde(default)]
    pub connected_platforms: Vec<ConnectedPlatform>,
}

impl CreateAccount {
    /// Parse + validate a request body, mirroring the pydantic validators.
    pub fn from_json(body: &[u8]) -> Result<Self, OctyError> {
        let mut account: CreateAccount = serde_json::from_slice(body).map_err(|e| {
            validation_error(vec![field_error(&["body"], &e.to_string(), "value_error.jsondecode")])
        })?;

        let mut errors: Vec<Value> = Vec::new();

        match validate_email(&account.contact_email_address) {
            Ok(normalized) => account.contact_email_address = normalized,
            Err(msg) => errors.push(field_error(
                &["body", "contact_email_address"],
                &msg,
                "value_error",
            )),
        }

        if !ALLOWED_ACCOUNT_TYPES.contains(&account.account_type.as_str()) {
            errors.push(field_error(
                &["body", "account_type"],
                "Invalid account type provided. Allowed permissions : 'startup', 'pro' or 'enterprise'",
                "value_error",
            ));
        }

        if let Err(msg) = validate_http_url(&account.webhook_url) {
            errors.push(field_error(&["body", "webhook_url"], &msg, "value_error.url"));
        }

        for permission in &account.permissions {
            if !ALLOWED_PERMISSIONS.contains(&permission.as_str()) {
                errors.push(field_error(
                    &["body", "permissions"],
                    "Invalid permission provided. Allowed permissions : 'rec', 'churn' or 'rfm' or 'seg' or 'mes'",
                    "value_error",
                ));
                break;
            }
        }

        let mut store_urls: Vec<String> = Vec::new();
        for (i, platform) in account.connected_platforms.iter_mut().enumerate() {
            let loc_type = format!("body.connected_platforms.{i}.platform_type");
            if !ALLOWED_PLATFORM_TYPES.contains(&platform.platform_type.as_str()) {
                errors.push(field_error(&[&loc_type], "invalid platform_type", "value_error"));
            }
            platform.store_url = platform.store_url.trim().to_string();
            platform.store_name = platform.store_name.trim().to_string();
            if platform.store_url.is_empty() {
                errors.push(field_error(
                    &["body", "connected_platforms"],
                    "Store URL cannot be empty",
                    "value_error",
                ));
            }
            if platform.store_name.is_empty() {
                errors.push(field_error(
                    &["body", "connected_platforms"],
                    "Store name cannot be empty",
                    "value_error",
                ));
            }
            store_urls.push(platform.store_url.clone());
        }
        let unique: std::collections::HashSet<&String> = store_urls.iter().collect();
        if unique.len() != store_urls.len() {
            errors.push(field_error(
                &["body", "connected_platforms"],
                "Duplicate store URLs are not allowed",
                "value_error",
            ));
        }

        if errors.is_empty() {
            Ok(account)
        } else {
            Err(validation_error(errors))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlgorithmConfig {
    pub algorithm_name: String,
    #[serde(default)]
    pub config_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChurnInfo {
    pub churn_percentage: f64,
    pub churn_indicator: String,
    pub churn_difference: f64,
    #[serde(default)]
    pub features: Option<Vec<Value>>,
}

/// Port of `data/models/account.py::UpdateAccount` — consumed from AMQP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAccount {
    pub account_id: String,
    #[serde(default)]
    pub contact_email_address: Option<String>,
    #[serde(default)]
    pub account_type: Option<String>,
    #[serde(default)]
    pub account_currency: Option<String>,
    #[serde(default)]
    pub contact_name: Option<String>,
    #[serde(default)]
    pub contact_surname: Option<String>,
    #[serde(default)]
    pub webhook_url: Option<String>,
    #[serde(default)]
    pub authenticated_id_key: Option<String>,
    #[serde(default)]
    pub algorithm_configurations: Option<AlgorithmConfig>,
    #[serde(default)]
    pub churn_info: Option<ChurnInfo>,
}

impl UpdateAccount {
    pub fn from_json(body: &[u8]) -> Result<Self, OctyError> {
        let account: UpdateAccount = serde_json::from_slice(body).map_err(|e| {
            validation_error(vec![field_error(&["body"], &e.to_string(), "value_error.jsondecode")])
        })?;
        if let Some(email) = &account.contact_email_address {
            validate_email(email).map_err(|msg| {
                validation_error(vec![field_error(
                    &["body", "contact_email_address"],
                    &msg,
                    "value_error",
                )])
            })?;
        }
        if let Some(url) = &account.webhook_url {
            validate_http_url(url).map_err(|msg| {
                validation_error(vec![field_error(&["body", "webhook_url"], &msg, "value_error.url")])
            })?;
        }
        Ok(account)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetAccountsInternal {
    pub account_ids: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteAccount {
    pub account_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_body() -> Value {
        json!({
            "contact_email_address": "Jane@ACME.io",
            "account_name": "acme",
            "account_type": "pro",
            "account_currency": "USD",
            "contact_name": "Jane",
            "contact_surname": "Doe",
            "webhook_url": "https://acme.io/hook",
            "permissions": ["rec", "churn"]
        })
    }

    #[test]
    fn accepts_valid_create_account_and_normalizes_email() {
        let body = serde_json::to_vec(&valid_body()).unwrap();
        let account = CreateAccount::from_json(&body).unwrap();
        assert_eq!(account.contact_email_address, "Jane@acme.io");
        assert!(account.connected_platforms.is_empty());
        assert!(account.authenticated_id_key.is_none());
    }

    #[test]
    fn rejects_bad_account_type_permission_url_email() {
        let mut body = valid_body();
        body["account_type"] = json!("mega");
        body["permissions"] = json!(["rec", "nope"]);
        body["webhook_url"] = json!("ftp://acme.io");
        body["contact_email_address"] = json!("not-an-email");
        let err = CreateAccount::from_json(&serde_json::to_vec(&body).unwrap()).unwrap_err();
        assert_eq!(err.code, 422);
        assert_eq!(err.reasons.len(), 4);
    }

    #[test]
    fn rejects_duplicate_store_urls() {
        let mut body = valid_body();
        body["connected_platforms"] = json!([
            {"platform_type": "shopify", "store_url": "https://a.io", "store_name": "A"},
            {"platform_type": "custom", "store_url": "https://a.io ", "store_name": "B"}
        ]);
        let err = CreateAccount::from_json(&serde_json::to_vec(&body).unwrap()).unwrap_err();
        assert_eq!(err.code, 422);
    }

    #[test]
    fn update_account_parses_amqp_payload() {
        let body = json!({
            "account_id": "account_123",
            "churn_info": {
                "churn_percentage": 12.5,
                "churn_indicator": "positive",
                "churn_difference": 0.4
            }
        });
        let update = UpdateAccount::from_json(&serde_json::to_vec(&body).unwrap()).unwrap();
        assert_eq!(update.account_id, "account_123");
        assert_eq!(update.churn_info.unwrap().churn_indicator, "positive");
    }
}
