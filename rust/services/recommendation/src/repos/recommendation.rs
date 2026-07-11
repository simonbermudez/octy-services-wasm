//! Port of `data/repositories/implementation/recommendation_repository.py`.
//!
//! MongoDB access goes through the data gateway; documents travel as
//! `bson.json_util` legacy extended JSON (`{"$date": millis}`, `{"$oid": hex}`).
//!
//! Collections match the mongoengine `Document` classes (lowercased class
//! names): `tbl_hparam_tuning_jobs` and `tbl_recommendations_cache`.

use octy_shared::ejson::date_millis;
use octy_shared::errors::OctyError;
use serde_json::{json, Value};

use octy_spin::ctx::Ctx;

const TBL_HPARAM_TUNING_JOBS: &str = "tbl_hparam_tuning_jobs";
const TBL_RECOMMENDATIONS_CACHE: &str = "tbl_recommendations_cache";

/// `get_latest_hp_tuning_job` — the newest Completed hyperparameter tuning
/// job for the account (Python: `.find(query).sort('updated_at', -1).limit(1)`).
///
/// The gateway `find` endpoint has no sort option yet, so all matching jobs
/// are fetched and sorted client-side by `updated_at` (descending); once the
/// gateway grows a `sort` parameter this should push the sort + limit down.
pub async fn get_latest_hp_tuning_job(ctx: &Ctx, account_id: &str) -> Result<Vec<Value>, OctyError> {
    let filter = json!({ "$and": [
        { "account_id": { "$eq": account_id } },
        { "status": { "$eq": "Completed" } },
    ]});
    let mut docs = ctx.gateway.find(TBL_HPARAM_TUNING_JOBS, filter, 0, 0).await?;
    docs.sort_by_key(|doc| {
        std::cmp::Reverse(date_millis(&doc["updated_at"]).unwrap_or(i64::MIN))
    });
    docs.truncate(1);
    Ok(docs)
}

/// `get_cached_recommendations` — cached item recommendations for the given
/// training job, restricted to the requested profile ids.
pub async fn get_cached_recommendations(
    ctx: &Ctx,
    account_id: &str,
    training_job_id: &str,
    profile_ids: &[String],
) -> Result<Vec<Value>, OctyError> {
    let filter = json!({ "$and": [
        { "account_id": { "$eq": account_id } },
        { "training_job_id": { "$eq": training_job_id } },
        { "profile_id": { "$in": profile_ids } },
    ]});
    ctx.gateway.find(TBL_RECOMMENDATIONS_CACHE, filter, 0, 0).await
}

/// mongoengine `.objects(...).delete()` is a Mongo `delete_many`; the gateway
/// only exposes `delete-one`, so emulate it by enumerating the matching
/// documents and deleting each by `_id`. A native
/// `POST /v1/mongo/{collection}/delete-many` gateway endpoint would replace
/// this loop.
async fn delete_many(ctx: &Ctx, collection: &str, filter: Value) -> Result<(), OctyError> {
    let docs = ctx.gateway.find(collection, filter, 0, 0).await?;
    for doc in docs {
        let Some(id) = doc.get("_id") else { continue };
        ctx.gateway
            .delete_one(collection, json!({ "_id": id }))
            .await?;
    }
    Ok(())
}

/// `delete_cached_recommendations` — AMQP `reccache.cmd.delete` handler body:
/// drop the cached recommendations for the given profiles.
pub async fn delete_cached_recommendations(
    ctx: &Ctx,
    account_id: &str,
    profiles: &[String],
) -> Result<(), OctyError> {
    delete_many(
        ctx,
        TBL_RECOMMENDATIONS_CACHE,
        json!({ "account_id": account_id, "profile_id": { "$in": profiles } }),
    )
    .await
}

/// `delete_all_cached_recommendations` — account-deletion fan-out: drop every
/// cached recommendation and hyperparameter tuning job for the account.
/// The Python swallowed exceptions (Sentry `capture_exception`) and returned
/// `False`; here failures go to stderr.
pub async fn delete_all_cached_recommendations(ctx: &Ctx, account_id: &str) -> bool {
    let result = async {
        delete_many(
            ctx,
            TBL_RECOMMENDATIONS_CACHE,
            json!({ "account_id": account_id }),
        )
        .await?;
        delete_many(
            ctx,
            TBL_HPARAM_TUNING_JOBS,
            json!({ "account_id": account_id }),
        )
        .await?;
        Ok::<(), OctyError>(())
    }
    .await;

    match result {
        Ok(()) => true,
        Err(err) => {
            eprintln!("[recommendation] delete_all_cached_recommendations({account_id}) failed: {err}");
            false
        }
    }
}
