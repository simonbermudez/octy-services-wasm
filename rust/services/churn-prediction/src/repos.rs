//! Port of `data/repositories/implementation/churn_repository.py`.
//!
//! The mongoengine schema `tbl_hparam_tuning_jobs` maps to the Mongo
//! collection of the same name; documents travel to/from the data gateway as
//! legacy extended JSON (`{"$date": millis}`, `{"$oid": hex}`).

use octy_shared::ejson;
use octy_shared::errors::OctyError;
use octy_spin::ctx::Ctx;
use serde_json::{json, Value};

const COLLECTION: &str = "tbl_hparam_tuning_jobs";

/// Python: `find(query).sort('updated_at', -1).limit(1)`.
///
/// The gateway `/v1/mongo/{coll}/find` endpoint has no `sort` parameter, so
/// fetch every matching document and pick the newest `updated_at` here —
/// behaviorally identical (documents missing `updated_at` sort last, like
/// Mongo's descending sort). A server-side `sort` option on the gateway find
/// endpoint would let this become `limit(1)` again.
pub async fn get_latest_hp_tuning_job(
    ctx: &Ctx,
    account_id: &Value,
) -> Result<Option<Value>, OctyError> {
    let filter = json!({ "$and": [
        { "account_id": { "$eq": account_id } },
        { "status": { "$eq": "Completed" } },
    ]});
    let docs = ctx.gateway.find(COLLECTION, filter, 0, 0).await?;
    Ok(docs
        .into_iter()
        .max_by_key(|doc| {
            doc.get("updated_at")
                .and_then(ejson::date_millis)
                .unwrap_or(i64::MIN)
        }))
}

/// Python: `delete_many({"$and": [{"account_id": {"$eq": account_id}}]})`,
/// unconditionally returning `True`.
///
/// The gateway only exposes `delete-one`, so emulate `delete_many` by
/// enumerating the matching `_id`s and deleting each — the end state is
/// identical, at the cost of one round trip per document (hp tuning jobs
/// accrue roughly one per account per day). A `/v1/mongo/{coll}/delete-many`
/// gateway endpoint would collapse this to a single call.
pub async fn delete_account_churn_predictions(
    ctx: &Ctx,
    account_id: &str,
) -> Result<bool, OctyError> {
    let filter = json!({ "$and": [
        { "account_id": { "$eq": account_id } },
    ]});
    let docs = ctx.gateway.find(COLLECTION, filter, 0, 0).await?;
    for doc in docs {
        if let Some(id) = doc.get("_id") {
            ctx.gateway
                .delete_one(COLLECTION, json!({ "_id": id }))
                .await?;
        }
    }
    Ok(true)
}
