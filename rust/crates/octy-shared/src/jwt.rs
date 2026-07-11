//! Port of `_AuthRepository.generate_authorization_token` — the RS256-signed
//! "fat JWT" carrying account info + authorized resource tags.
//!
//! Implemented with the pure-Rust `rsa`/`sha2` crates so it compiles for
//! `wasm32-wasip1` (no ring / no C dependencies).

use base64::Engine;
use chrono::{Duration, Utc};
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::{SigningKey, VerifyingKey};
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey};
use rsa::sha2::Sha256;
use rsa::signature::{SignatureEncoding, Signer, Verifier};
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde_json::{json, Value};

use crate::errors::OctyError;
use crate::utils::dt_to_int;

const B64URL: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// Parse an RSA private key from PEM (PKCS#8 `PRIVATE KEY` or PKCS#1
/// `RSA PRIVATE KEY`).
pub fn private_key_from_pem(pem: &str) -> Result<RsaPrivateKey, OctyError> {
    if pem.contains("BEGIN RSA PRIVATE KEY") {
        RsaPrivateKey::from_pkcs1_pem(pem)
            .map_err(|e| OctyError::internal(format!("invalid PKCS#1 private key: {e}")))
    } else {
        RsaPrivateKey::from_pkcs8_pem(pem)
            .map_err(|e| OctyError::internal(format!("invalid PKCS#8 private key: {e}")))
    }
}

/// Parse an RSA public key from PEM (SPKI `PUBLIC KEY`, as in
/// `keys/octy-public-key.pub`).
pub fn public_key_from_pem(pem: &str) -> Result<RsaPublicKey, OctyError> {
    RsaPublicKey::from_public_key_pem(pem)
        .map_err(|e| OctyError::internal(format!("invalid public key: {e}")))
}

/// Verify an RS256 JWT signature and return its claims.
///
/// Mirrors `jwt.decode(token, public_key, algorithms='RS256')` on the fat
/// JWT: signature-only — the fat JWT keeps its expiry in the nonstandard
/// `m.exp` (`YYYYMMDDHHMMSS` int) claim, which callers check themselves.
pub fn verify_rs256(token: &str, key: &RsaPublicKey) -> Result<Value, OctyError> {
    let invalid = || OctyError::internal("Invalid JWT token provided!");

    let mut parts = token.split('.');
    let (Some(header_b64), Some(payload_b64), Some(sig_b64), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid());
    };

    let header: Value = serde_json::from_slice(&B64URL.decode(header_b64).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    if header["alg"] != "RS256" {
        return Err(invalid());
    }

    let signature =
        rsa::pkcs1v15::Signature::try_from(B64URL.decode(sig_b64).map_err(|_| invalid())?.as_slice())
            .map_err(|_| invalid())?;
    VerifyingKey::<Sha256>::new(key.clone())
        .verify(format!("{header_b64}.{payload_b64}").as_bytes(), &signature)
        .map_err(|_| invalid())?;

    serde_json::from_slice(&B64URL.decode(payload_b64).map_err(|_| invalid())?)
        .map_err(|_| invalid())
}

/// Sign arbitrary claims as an RS256 JWT.
pub fn sign_rs256(claims: &Value, key: &RsaPrivateKey) -> Result<String, OctyError> {
    let header = json!({"alg": "RS256", "typ": "JWT"});
    let signing_input = format!(
        "{}.{}",
        B64URL.encode(serde_json::to_vec(&header).expect("json")),
        B64URL.encode(serde_json::to_vec(claims).expect("json")),
    );
    let signing_key = SigningKey::<Sha256>::new(key.clone());
    let signature = signing_key.sign(signing_input.as_bytes());
    Ok(format!("{signing_input}.{}", B64URL.encode(signature.to_bytes())))
}

/// Python `str(...)` rendering used by the `li` (limits) claim:
/// `None` → `"None"`, numbers/strings verbatim, booleans `True`/`False`.
fn py_str(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(true)) => "True".to_string(),
        Some(Value::Bool(false)) => "False".to_string(),
        Some(other) => other.to_string(),
    }
}

/// Build the fat-JWT claims from a cached account document (legacy extended
/// JSON, as stored in Redis) — field-for-field port of the Python payload.
pub fn build_auth_claims(account: &Value) -> Result<Value, OctyError> {
    let cfg = &account["account_configurations"];
    let limit_keys = [
        "MAX_TOTAL_PROFILES",
        "MAX_TOTAL_ITEMS",
        "MAX_TOTAL_CUSTOM_EVENT_TYPES",
        "MAX_TOTAL_EVENTS",
        "MAX_TOTAL_SEGMENT_DEFINITIONS",
        "MAX_TOTAL_MESSAGE_TEMPLATES",
    ];
    let first_limit = cfg
        .get("limits")
        .and_then(|l| l.get(0))
        .ok_or_else(|| OctyError::internal("account has no limits configured"))?;
    let li = limit_keys
        .iter()
        .map(|k| py_str(first_limit.get(*k)))
        .collect::<Vec<_>>()
        .join("*");

    let now = Utc::now();
    Ok(json!({
        "m": {
            "iss": "octy-auth-service",
            "iat": dt_to_int(now),
            "exp": dt_to_int(now + Duration::hours(1)),
        },
        "b": {
            "a_id": account["_id"],
            "a_n": account["account_name"],
            "b": account["bucket"],
            "pe": account["permissions"],
            "a_cf": {
                "a_t": cfg["account_type"],
                "a_c": cfg["account_currency"],
                "c_n": cfg["contact_name"],
                "c_s": cfg["contact_surname"],
                "c_e": cfg["contact_email_address"],
                "we": cfg["webhook_url"],
                "ak": cfg.get("authenticated_id_key").cloned().unwrap_or(Value::Null),
                "li": li,
            },
            "al_cf": account["algorithm_configurations"],
            "c_i": account["churn_info"],
            "c_at": account["created_at"],
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs1v15::VerifyingKey;
    use rsa::signature::Verifier;
    use rsa::RsaPublicKey;

    fn test_key() -> RsaPrivateKey {
        let mut rng = rand::thread_rng();
        RsaPrivateKey::new(&mut rng, 2048).unwrap()
    }

    #[test]
    fn signs_verifiable_rs256() {
        let key = test_key();
        let claims = json!({"m": {"iss": "octy-auth-service"}});
        let token = sign_rs256(&claims, &key).unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);

        let header: Value =
            serde_json::from_slice(&B64URL.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["alg"], "RS256");

        let verifying = VerifyingKey::<Sha256>::new(RsaPublicKey::from(&key));
        let sig = rsa::pkcs1v15::Signature::try_from(
            B64URL.decode(parts[2]).unwrap().as_slice(),
        )
        .unwrap();
        verifying
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
            .unwrap();
    }

    #[test]
    fn verify_rs256_roundtrip_and_tamper_detection() {
        let key = test_key();
        let claims = json!({"m": {"iss": "octy-auth-service", "exp": 20991231235959i64}, "b": {"a_n": "acme"}});
        let token = sign_rs256(&claims, &key).unwrap();

        let public = RsaPublicKey::from(&key);
        let decoded = verify_rs256(&token, &public).unwrap();
        assert_eq!(decoded, claims);

        // tampered payload must fail
        let mut parts: Vec<String> = token.split('.').map(String::from).collect();
        parts[1] = B64URL.encode(br#"{"m":{"iss":"evil"}}"#);
        assert!(verify_rs256(&parts.join("."), &public).is_err());

        // wrong key must fail
        let other = RsaPublicKey::from(&test_key());
        assert!(verify_rs256(&token, &other).is_err());
    }

    #[test]
    fn claims_match_python_shape() {
        let account = json!({
            "_id": {"$oid": "64b7f3a1c2d4e5f6a7b8c9d0"},
            "account_name": "acme",
            "bucket": "bucket-abc",
            "permissions": ["rec", "churn"],
            "account_configurations": {
                "account_type": "pro",
                "account_currency": "USD",
                "contact_name": "Jane",
                "contact_surname": "Doe",
                "contact_email_address": "jane@acme.io",
                "webhook_url": "https://acme.io/hook",
                "limits": [{
                    "MAX_TOTAL_PROFILES": 1000,
                    "MAX_TOTAL_ITEMS": 500,
                    "MAX_TOTAL_CUSTOM_EVENT_TYPES": 10,
                    "MAX_TOTAL_EVENTS": 100000,
                    "MAX_TOTAL_SEGMENT_DEFINITIONS": 25,
                    "MAX_TOTAL_MESSAGE_TEMPLATES": 50
                }]
            },
            "algorithm_configurations": [],
            "churn_info": {},
            "created_at": {"$date": 1750000000000i64}
        });
        let claims = build_auth_claims(&account).unwrap();
        assert_eq!(claims["m"]["iss"], "octy-auth-service");
        assert_eq!(claims["b"]["a_id"]["$oid"], "64b7f3a1c2d4e5f6a7b8c9d0");
        assert_eq!(claims["b"]["a_cf"]["li"], "1000*500*10*100000*25*50");
        // authenticated_id_key missing -> null ("ak": None in Python)
        assert_eq!(claims["b"]["a_cf"]["ak"], Value::Null);
        // iat/exp use the YYYYMMDDHHMMSS integer encoding
        assert!(claims["m"]["iat"].as_i64().unwrap() > 20260101000000);
    }
}
