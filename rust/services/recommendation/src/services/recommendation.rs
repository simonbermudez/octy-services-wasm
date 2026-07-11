//! Port of `services/recommendation.py::RecommendationsService`.

use octy_shared::ejson::date_millis;
use octy_shared::errors::{ErrorReason, OctyError};
use octy_shared::utils::int_to_dt;
use serde_json::{json, Value};

use octy_spin::ctx::Ctx;

use crate::repos::recommendation as repo;

const NO_RECOMMENDATIONS_ERROR: &str = "Profile does not exist or Insufficient number of items available to make recommendations for this profile, if 'recommend_interacted_items' set to 'false' in your recommendations algorithm configurations, try setting it to 'true' if this frequently occurs.";

/// `get_recommendations` — returns `(recommendations, training_job_meta)`.
///
/// `training_job_meta` is the tuning job's `best_model_meta_data` with
/// `training_job_id` and `model_created_at` merged in, exactly as the Python
/// mutated the dict before handing it to `GetRecommendationsDTO`.
pub async fn get_recommendations(
    ctx: &Ctx,
    account_id: &str,
    profile_ids: &[String],
) -> Result<(Vec<Value>, Value), OctyError> {
    let hp_tuning_job = repo::get_latest_hp_tuning_job(ctx, account_id).await?;
    let Some(job) = hp_tuning_job.first() else {
        return Err(OctyError::new(
            400,
            "An error occurred when getting item recommendations",
            vec![ErrorReason::new(
                "No recommendations training jobs have been completed. Recommendations training jobs are automatically run every 24 hours",
                ctx.config.opt_str("RECOMENDATIONS_EXTENDED_HELP").unwrap_or(""),
            )],
        ));
    };

    // Missing keys on the job document were Python `KeyError`s → generic 500.
    let training_job_id = job["best_model_training_job_id"]
        .as_str()
        .ok_or_else(|| OctyError::internal("hp tuning job missing best_model_training_job_id"))?
        .to_string();

    let recommendations_cache =
        repo::get_cached_recommendations(ctx, account_id, &training_job_id, profile_ids).await?;

    let mut recommendations: Vec<Value> = Vec::with_capacity(profile_ids.len());
    for profile_id in profile_ids {
        // `_filter_recommendations` — first cached document for this profile.
        let matched = recommendations_cache
            .iter()
            .find(|doc| doc["profile_id"].as_str() == Some(profile_id.as_str()));

        let Some(doc) = matched else {
            recommendations.push(json!({
                "profile_id": profile_id,
                "recommendations": [],
                "error": NO_RECOMMENDATIONS_ERROR,
            }));
            continue;
        };

        let items = doc
            .get("recommendations")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                OctyError::internal("cached recommendation document missing 'recommendations'")
            })?;
        let top_ten: Vec<Value> = items.iter().take(10).cloned().collect();
        recommendations.push(json!({
            "profile_id": profile_id,
            "recommendations": top_ten,
            "error": Value::Null,
        }));
    }

    let mut meta = match job.get("best_model_meta_data") {
        Some(Value::Object(map)) => map.clone(),
        _ => {
            return Err(OctyError::internal(
                "hp tuning job best_model_meta_data is not an object",
            ))
        }
    };
    meta.insert("training_job_id".to_string(), json!(training_job_id));

    // Python: `int_to_dt(job['updated_at']['$date'], as_str=True)` →
    // '%a, %d %b %Y %H:%M:%S GMT'. (`dt.fromtimestamp` used the container's
    // local timezone — UTC in deployment — so this formats in UTC.)
    let updated_at_millis = date_millis(&job["updated_at"])
        .ok_or_else(|| OctyError::internal("hp tuning job missing updated_at"))?;
    let model_created_at = int_to_dt(updated_at_millis)
        .ok_or_else(|| OctyError::internal("hp tuning job updated_at out of range"))?
        .format("%a, %d %b %Y %H:%M:%S GMT")
        .to_string();
    meta.insert("model_created_at".to_string(), json!(model_created_at));

    Ok((recommendations, Value::Object(meta)))
}

/// `delete_account_recommendations` — account-deletion fan-out; `true` when
/// every cached recommendation and tuning job was deleted.
pub async fn delete_account_recommendations(ctx: &Ctx, account_id: &str) -> bool {
    repo::delete_all_cached_recommendations(ctx, account_id).await
}
