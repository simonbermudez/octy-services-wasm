//! Port of `data/repositories/implementation/profiles_iden_repository.py`'s
//! `get_profile_key_types` (Redis) and `create_merged_profiles_ref` (Mongo).

use octy_shared::ejson::now_legacy_date;
use octy_shared::errors::OctyError;
use serde_json::{json, Value};

use octy_spin::ctx::Ctx;

const MERGED_PROFILES_COLLECTION: &str = "tbl_merged_profiles";

/// Port of `get_profile_key_types` — `SMEMBERS {account_id}_profile_key_types`,
/// each member a JSON string `{"key": "...", "type_": "<class '...'>"}`.
/// The Python worker used Redis DB index `1` (`db_redis_connect`'s `db=1`).
pub fn get_profile_key_types(ctx: &Ctx, account_id: &str) -> Result<Vec<(String, String)>, OctyError> {
    let conn = spin_sdk::redis::Connection::open(&ctx.redis_address(1)?)
        .map_err(|e| OctyError::internal(format!("redis connect failed: {e:?}")))?;
    let members = conn
        .smembers(&format!("{account_id}_profile_key_types"))
        .map_err(|e| OctyError::internal(format!("redis smembers failed: {e:?}")))?;

    Ok(members
        .iter()
        .filter_map(|m| serde_json::from_str::<Value>(m).ok())
        .filter_map(|v| {
            let key = v.get("key")?.as_str()?.to_string();
            let type_ = v.get("type_")?.as_str()?.to_string();
            Some((key, type_))
        })
        .collect())
}

/// Port of `create_merged_profiles_ref`'s bulk insert.
///
/// GAP: the gateway only exposes `insert-one` — the Python used
/// `initialize_unordered_bulk_op()` (a single round trip, partial-failure
/// tolerant). This loops `insert_one` per record instead, which is neither
/// atomic nor a single round trip. A `POST /v1/mongo/{collection}/insert-many`
/// gateway endpoint (accepting a JSON array of legacy-extended-JSON
/// documents, unordered semantics to match `initialize_unordered_bulk_op`)
/// would close this gap; not added here per the brief (gateway is out of
/// scope for this port).
pub async fn create_merged_profiles_ref(ctx: &Ctx, merged_profiles: &[Value]) -> Result<(), OctyError> {
    for profile in merged_profiles {
        let doc = json!({
            "account_id": profile.get("account_id").cloned().unwrap_or(Value::Null),
            "merged_profiles": profile.get("merged_profiles").cloned().unwrap_or(json!([])),
            "parent_profile_id": profile.get("parent_profile_id").cloned().unwrap_or(Value::Null),
            "parent_customer_id": profile.get("parent_customer_id").cloned().unwrap_or(Value::Null),
            "authenticated_id_key": profile.get("authenticated_id_key").cloned().unwrap_or(Value::Null),
            "authenticated_id_value": profile.get("authenticated_id_value").cloned().unwrap_or(Value::Null),
            "created_at": now_legacy_date(),
        });
        ctx.gateway.insert_one(MERGED_PROFILES_COLLECTION, doc).await?;
    }
    Ok(())
}
