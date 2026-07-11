//! Port of `data/repositories/implementation/auth_repository.py`.

use argon2::password_hash::PasswordHash;
use argon2::{Argon2, PasswordVerifier};
use chrono::{Duration, Utc};
use octy_shared::ejson::legacy_date;
use octy_shared::errors::OctyError;
use octy_shared::jwt::{build_auth_claims, private_key_from_pem, sign_rs256};
use serde_json::{json, Value};

use octy_spin::ctx::Ctx;
use crate::repos::account::get_cached_account;

const COLLECTION: &str = "tbl_failed_auth_attempts";

/// `verify_account_keys` — returns `(pk_valid, sk_valid, account)`.
/// The account is read from the Redis cache; the secret key is verified
/// against the stored Argon2 PHC hash (params come from the hash itself, so
/// hashes minted by either the Python or Rust services verify identically).
pub fn verify_account_keys(
    ctx: &Ctx,
    pk: &str,
    sk: &str,
) -> Result<(bool, bool, Option<Value>), OctyError> {
    let Some(account) = get_cached_account(ctx, pk)? else {
        return Ok((false, false, None));
    };

    if !account.get("active").and_then(Value::as_bool).unwrap_or(false) {
        return Ok((false, false, None));
    }

    let stored_hash = account["keys"]["secret_key"]
        .as_str()
        .ok_or_else(|| OctyError::internal("cached account missing secret key hash"))?;
    let parsed = PasswordHash::new(stored_hash)
        .map_err(|e| OctyError::internal(format!("invalid stored secret key hash: {e}")))?;

    if Argon2::default().verify_password(sk.as_bytes(), &parsed).is_err() {
        return Ok((true, false, None));
    }

    Ok((true, true, Some(account)))
}

/// RSA private key PEM for RS256 signing (base64-encoded variable/env with
/// raw-PEM fallback, exactly like the Python).
fn octy_private_key_pem() -> Result<String, OctyError> {
    let raw = octy_spin::ctx::variable("octy_private_key", "OCTY_PRIVATE_KEY")?;
    Ok(octy_shared::utils::base64_decode_str(&raw))
}

/// `generate_authorization_token` — the RS256 fat JWT.
pub fn generate_authorization_token(_ctx: &Ctx, account: &Value) -> Result<String, OctyError> {
    let pem = octy_private_key_pem()?;
    let key = private_key_from_pem(&pem)?;
    let claims = build_auth_claims(account)?;
    sign_rs256(&claims, &key)
}

/// `log_failed_auth` — records the attempt and returns all attempts against
/// the same public key in the last 30 minutes. Failures are swallowed (the
/// Python wrapped this in try/except with sentry capture).
pub async fn log_failed_auth(ctx: &Ctx, failed_attempt: &Value) -> Vec<Value> {
    let insert = ctx
        .gateway
        .insert_one(
            COLLECTION,
            json!({
                "public_key": failed_attempt["public_key"],
                "user_agent": failed_attempt["user_agent"],
                "server_name": failed_attempt["server_name"],
                "server_port": failed_attempt["server_port"],
                "request_type": failed_attempt["request_type"],
                "created_at": legacy_date(Utc::now()),
            }),
        )
        .await;
    if insert.is_err() {
        return Vec::new();
    }

    let backdate = Utc::now() - Duration::minutes(30);
    ctx.gateway
        .find(
            COLLECTION,
            json!({
                "public_key": failed_attempt["public_key"],
                "created_at": { "$gt": legacy_date(backdate) },
            }),
            0,
            0,
        )
        .await
        .unwrap_or_default()
}
