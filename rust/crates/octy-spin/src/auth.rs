//! Port of the `decode_account_jwt` dependency shared by all downstream
//! services: verify the `X-AUTH-JWT` fat token (RS256), check its
//! `m.exp` (YYYYMMDDHHMMSS int), enforce `REQUIRED_PERMISSIONS` from config,
//! and hand back the account claims.

use chrono::Utc;
use octy_shared::errors::{ErrorReason, OctyError};
use octy_shared::jwt::{public_key_from_pem, verify_rs256};
use octy_shared::utils::{base64_decode_str, dt_to_int};
use serde_json::Value;
use spin_sdk::http::Request;

use crate::ctx::{variable, Ctx};

/// The `Account` request-model every service builds from the decoded token.
#[derive(Debug, Clone)]
pub struct AuthAccount {
    /// `b.a_id` — the Mongo `_id` (legacy extended JSON `{"$oid": …}`).
    pub account_id: Value,
    pub account_name: String,
    pub bucket: String,
    pub permissions: Vec<String>,
    pub account_configurations: Value,
    pub algorithm_configurations: Value,
    pub churn_info: Value,
    pub created_at: Value,
    /// Full decoded claims, for anything service-specific.
    pub claims: Value,
}

fn load_public_key_pem() -> Result<String, OctyError> {
    // Preferred: the octy_public_key variable (base64 or raw PEM).
    if let Ok(raw) = variable("octy_public_key", "OCTY_PUBLIC_KEY") {
        return Ok(base64_decode_str(&raw));
    }
    // Fallback: the packaged key file (spin.toml `files`), matching the
    // Python `open('keys/octy-public-key.pub')`.
    std::fs::read_to_string("keys/octy-public-key.pub")
        .map_err(|e| OctyError::internal(format!("cannot load octy public key: {e}")))
}

/// The Python raised a plain `Exception` for invalid tokens, surfacing as the
/// generic 500 envelope; expired/invalid tokens therefore return 500 here too.
fn invalid_token() -> OctyError {
    OctyError::internal("Authentication failed becuase of a server error. Invalid JWT token provided!")
}

pub fn decode_account_jwt(ctx: &Ctx, req: &Request) -> Result<AuthAccount, OctyError> {
    let Some(token) = req.header("x-auth-jwt").and_then(|v| v.as_str()) else {
        return Err(OctyError::new(
            400,
            "Missing header",
            vec![ErrorReason::new(
                "[X-AUTH-JWT] : auth-token header must be provided in request headers.",
                ctx.config.opt_str("ERRORS_OVERVIEW_EXTENDED_HELP").unwrap_or(""),
            )],
        ));
    };

    let key = public_key_from_pem(&load_public_key_pem()?)?;
    let claims = verify_rs256(token, &key).map_err(|_| invalid_token())?;

    if claims["m"]["exp"].as_i64().unwrap_or(0) < dt_to_int(Utc::now()) {
        return Err(invalid_token());
    }

    // REQUIRED_PERMISSIONS is optional config (KeyError → pass in Python).
    let token_permissions: Vec<String> = claims["b"]["pe"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    if let Ok(required) = ctx.config.get_array("REQUIRED_PERMISSIONS") {
        for permission in required.iter().filter_map(Value::as_str) {
            if !token_permissions.iter().any(|p| p == permission) {
                return Err(OctyError::new(
                    401,
                    "Insufficient resource permissions",
                    vec![ErrorReason::new(
                        format!(
                            "This account is not permitted to access this resource. If you wish to access this resource, please contact our support team. {}",
                            ctx.config.opt_str("SUPPORT_EMAIL").unwrap_or("")
                        ),
                        ctx.config.opt_str("AUTH_EXTENDED_HELP").unwrap_or(""),
                    )],
                ));
            }
        }
    }

    let b = &claims["b"];
    Ok(AuthAccount {
        account_id: b["a_id"].clone(),
        account_name: b["a_n"].as_str().unwrap_or_default().to_string(),
        bucket: b["b"].as_str().unwrap_or_default().to_string(),
        permissions: token_permissions,
        account_configurations: b["a_cf"].clone(),
        algorithm_configurations: b["al_cf"].clone(),
        churn_info: b["c_i"].clone(),
        created_at: b["c_at"].clone(),
        claims: claims.clone(),
    })
}

impl AuthAccount {
    /// The account's Mongo `_id` hex (most repositories key on it).
    pub fn account_oid(&self) -> Option<&str> {
        octy_shared::ejson::oid_hex(&self.account_id)
            .or_else(|| self.account_id.as_str())
    }
}
