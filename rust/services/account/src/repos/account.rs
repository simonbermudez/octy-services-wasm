//! Port of `data/repositories/implementation/account_repository.py`.
//!
//! MongoDB access goes through the data gateway; the Redis account cache is
//! written directly via Spin's outbound-Redis host capability, using the same
//! `pk:{public_key}` keys and `bson.json_util`-compatible legacy extended
//! JSON encoding the Python services read.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use octy_shared::ejson::now_legacy_date;
use octy_shared::errors::OctyError;
use octy_shared::models::{CreateAccount, UpdateAccount};
use octy_shared::utils::generate_uid;
use serde_json::{json, Value};

use octy_spin::ctx::Ctx;

const COLLECTION: &str = "tbl_accounts";

/// argon2-cffi 20.1.0 `PasswordHasher()` defaults, kept for hash parity with
/// the Python services (params are embedded in the PHC string, so hashes
/// remain mutually verifiable in both directions).
fn password_hasher() -> Argon2<'static> {
    let params = Params::new(102_400, 2, 8, Some(16)).expect("valid argon2 params");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash_secret_key(secret_key: &str) -> Result<String, OctyError> {
    let salt = SaltString::generate(&mut OsRng);
    password_hasher()
        .hash_password(secret_key.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| OctyError::internal(format!("argon2 hashing failed: {e}")))
}

// ---- Redis account cache (`_cache_account_data`) ----

fn redis_conn(ctx: &Ctx) -> Result<spin_sdk::redis::Connection, OctyError> {
    spin_sdk::redis::Connection::open(&ctx.redis_address(2)?)
        .map_err(|e| OctyError::internal(format!("redis connect failed: {e:?}")))
}

pub fn cache_account_data(ctx: &Ctx, pk: &str, account: &Value) -> Result<(), OctyError> {
    let payload = serde_json::to_vec(account).expect("serializable json");
    redis_conn(ctx)?
        .set(&format!("pk:{pk}"), &payload)
        .map_err(|e| OctyError::internal(format!("redis set failed: {e:?}")))
}

pub fn get_cached_account(ctx: &Ctx, pk: &str) -> Result<Option<Value>, OctyError> {
    let raw = redis_conn(ctx)?
        .get(&format!("pk:{pk}"))
        .map_err(|e| OctyError::internal(format!("redis get failed: {e:?}")))?;
    match raw {
        None => Ok(None),
        Some(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
    }
}

pub fn delete_cached_account(ctx: &Ctx, pk: &str) -> Result<(), OctyError> {
    redis_conn(ctx)?
        .del(&[format!("pk:{pk}")])
        .map_err(|e| OctyError::internal(format!("redis del failed: {e:?}")))?;
    Ok(())
}

// ---- Repository methods ----

pub async fn get_account_by_account_id(ctx: &Ctx, account_id: &str) -> Result<Option<Value>, OctyError> {
    ctx.gateway
        .find_one(COLLECTION, json!({ "account_id": account_id }))
        .await
}

/// `create_account` — returns `(new_account_document, plaintext_secret_key)`.
pub async fn create_account(
    ctx: &Ctx,
    account: &CreateAccount,
    bucket: &str,
) -> Result<(Value, String), OctyError> {
    let secret_key = generate_uid("sk");
    let pk = generate_uid("pk");

    let keys = json!({
        "public_key": pk,
        "secret_key": hash_secret_key(&secret_key)?,
    });

    let resource_limits = ctx
        .config
        .get("RESOURCE_LIMITS")?
        .get(&account.account_type)
        .cloned()
        .ok_or_else(|| {
            OctyError::internal(format!("no RESOURCE_LIMITS for {}", account.account_type))
        })?;

    let account_configurations = json!({
        "account_type": account.account_type,
        "account_currency": account.account_currency,
        "contact_name": account.contact_name,
        "contact_surname": account.contact_surname,
        "contact_email_address": account.contact_email_address,
        "webhook_url": account.webhook_url,
        "authenticated_id_key": account.authenticated_id_key,
        "limits": [resource_limits],
    });

    let account_id = generate_uid("account");

    let mut new_account = json!({
        "account_id": account_id,
        "account_name": account.account_name,
        "bucket": bucket,
        "permissions": account.permissions,
        "keys": keys,
        "account_configurations": account_configurations,
        "algorithm_configurations": [
            { "algorithm_name": "rec", "config_json": {} },
            { "algorithm_name": "churn", "config_json": {} },
        ],
        "churn_info": {},
        "last_updated_action": "Account created",
        "connected_platforms": account.connected_platforms,
        "created_at": now_legacy_date(),
        "updated_at": now_legacy_date(),
        "active": true,
    });

    // insert_one raises 'Duplicate entry' (mapped in the gateway client)
    let inserted_id = ctx.gateway.insert_one(COLLECTION, new_account.clone()).await?;
    new_account["_id"] = inserted_id;

    new_account["api_usage"] = json!([{ "month": 0, "request_count": 0 }]);

    if let Err(err) = cache_account_data(ctx, &pk, &new_account) {
        // cache write failed -> roll the insert back, exactly like the Python
        let _ = ctx
            .gateway
            .delete_one(COLLECTION, json!({ "account_id": new_account["account_id"] }))
            .await;
        return Err(err);
    }

    Ok((new_account, secret_key))
}

pub async fn get_accounts(
    ctx: &Ctx,
    account_ids: &[Value],
    cursor: i64,
) -> Result<(Vec<Value>, i64), OctyError> {
    let filter = json!({ "account_id": { "$in": account_ids }, "active": true });
    let mut accounts = ctx.gateway.find(COLLECTION, filter.clone(), cursor, 100).await?;
    let total = ctx.gateway.count(COLLECTION, filter).await?;
    for doc in &mut accounts {
        if let Some(obj) = doc.as_object_mut() {
            obj.remove("keys");
        }
    }
    Ok((accounts, total))
}

/// `update_account(account, action)` — consumed from AMQP.
pub async fn update_account(ctx: &Ctx, account: &UpdateAccount, action: &str) -> Result<(), OctyError> {
    fn opt(value: &Option<String>) -> Value {
        value.as_ref().map(|s| json!(s)).unwrap_or(Value::Null)
    }

    let update_fields = match action {
        "account-config" => json!({
            "account_configurations.contact_name": opt(&account.contact_name),
            "account_configurations.contact_surname": opt(&account.contact_surname),
            "account_configurations.contact_email_address": opt(&account.contact_email_address),
            "account_configurations.webhook_url": opt(&account.webhook_url),
            "account_configurations.authenticated_id_key": opt(&account.authenticated_id_key),
            "last_updated_action": "updated account configurations",
            "updated_at": now_legacy_date(),
        }),
        "algorithm-config" => {
            let algo = account
                .algorithm_configurations
                .as_ref()
                .ok_or_else(|| OctyError::internal("[toxic]:: missing algorithm_configurations"))?;
            let index = if algo.algorithm_name == "rec" { 0 } else { 1 };
            json!({
                (format!("algorithm_configurations.{index}.algorithm_name")): algo.algorithm_name,
                (format!("algorithm_configurations.{index}.config_json")): algo.config_json,
                "last_updated_action": "updated algorithm configurations",
                "updated_at": now_legacy_date(),
            })
        }
        "churn-info" => {
            let churn = account
                .churn_info
                .as_ref()
                .ok_or_else(|| OctyError::internal("[toxic]:: missing churn_info"))?;
            json!({
                "churn_info.churn_percentage": churn.churn_percentage,
                "churn_info.churn_indicator": churn.churn_indicator,
                "churn_info.churn_difference": churn.churn_difference,
                "churn_info.features": churn.features,
                "last_updated_action": "updated churn info",
                "updated_at": now_legacy_date(),
            })
        }
        other => return Err(OctyError::internal(format!("[toxic]:: unknown action {other}"))),
    };

    ctx.gateway
        .update_one(
            COLLECTION,
            json!({ "account_id": account.account_id }),
            json!({ "$set": update_fields }),
        )
        .await?;

    let acc = ctx
        .gateway
        .find_one(COLLECTION, json!({ "account_id": account.account_id }))
        .await?
        .ok_or_else(|| OctyError::internal("Account not found"))?;

    let pk = acc["keys"]["public_key"]
        .as_str()
        .ok_or_else(|| OctyError::internal("account missing public key"))?
        .to_string();

    let cached = get_cached_account(ctx, &pk)?
        .ok_or_else(|| OctyError::internal("Account not found in DB cache"))?;

    let mut acc = acc;
    acc["api_usage"] = cached.get("api_usage").cloned().unwrap_or_else(|| json!([]));
    cache_account_data(ctx, &pk, &acc)
}

pub async fn delete_account(ctx: &Ctx, account_id: &str) -> Result<(), OctyError> {
    let Some(acc) = ctx
        .gateway
        .find_one(COLLECTION, json!({ "account_id": account_id }))
        .await?
    else {
        return Ok(());
    };
    if let Some(pk) = acc["keys"]["public_key"].as_str() {
        delete_cached_account(ctx, pk)?;
    }
    ctx.gateway
        .delete_one(COLLECTION, json!({ "account_id": account_id }))
        .await
}

/// `refresh_account_data_cache` — re-prime the Redis cache from MongoDB.
pub async fn refresh_account_data_cache(ctx: &Ctx, pk: &str) -> Result<(), OctyError> {
    if let Some(mut acc) = ctx
        .gateway
        .find_one(COLLECTION, json!({ "keys.public_key": pk }))
        .await?
    {
        // json_util.dumps(acc) in Python has no api_usage on the fresh doc;
        // downstream code re-merges it from the old cache when updating. Keep
        // whatever the cache already tracks to avoid losing usage counters.
        if acc.get("api_usage").is_none() {
            if let Ok(Some(cached)) = get_cached_account(ctx, pk) {
                if let Some(usage) = cached.get("api_usage") {
                    acc["api_usage"] = usage.clone();
                }
            }
        }
        cache_account_data(ctx, pk, &acc)?;
    }
    Ok(())
}
