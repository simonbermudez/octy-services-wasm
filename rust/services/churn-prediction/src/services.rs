//! Port of `services/churn_prediction.py` (`ChurnPredictionService`).

use chrono::{DateTime, Utc};
use octy_shared::ejson;
use octy_shared::errors::{ErrorReason, OctyError};
use octy_spin::auth::AuthAccount;
use octy_spin::ctx::Ctx;
use serde_json::{json, Value};

use crate::repos;

/// `dict['key']` equivalent — a missing key raised `KeyError` in Python and
/// surfaced as the generic 500 envelope, which `OctyError::internal` mirrors.
fn key<'v>(value: &'v Value, name: &str) -> Result<&'v Value, OctyError> {
    value
        .get(name)
        .ok_or_else(|| OctyError::internal(format!("KeyError: '{name}'")))
}

/// Python `round(x, 1)`: ints pass through unchanged; floats are rounded to
/// one decimal (correctly-rounded decimal conversion, ties-to-even — the same
/// algorithm Rust's `{:.1}` float formatting uses). Non-numbers raised
/// `TypeError` → generic 500, mirrored by `OctyError::internal`.
fn round1(value: &Value) -> Result<Value, OctyError> {
    if value.is_i64() || value.is_u64() {
        return Ok(value.clone());
    }
    if let Some(f) = value.as_f64() {
        let rounded: f64 = format!("{f:.1}").parse().unwrap_or(f);
        return Ok(json!(rounded));
    }
    Err(OctyError::internal(format!(
        "TypeError: type {value} doesn't define __round__ method"
    )))
}

/// Port of `int_to_dt(dt_int, as_str=True)`:
/// `datetime.fromtimestamp(ms / 1e3).strftime('%a, %d %b %Y %H:%M:%S GMT')`.
/// Python used the process-local timezone (UTC in the deployment containers);
/// this port always formats in UTC.
fn int_to_dt_str(millis: i64) -> Result<String, OctyError> {
    let dt: DateTime<Utc> = DateTime::from_timestamp_millis(millis)
        .ok_or_else(|| OctyError::internal("invalid updated_at timestamp"))?;
    Ok(dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
}

/// GET /v1/retention/churn_prediction/report — build the churn report from
/// the latest completed hyper-parameter tuning job plus the `churn_info`
/// claims (`b.c_i`) carried by the auth JWT.
pub async fn generate_churn_report(
    ctx: &Ctx,
    account: &AuthAccount,
) -> Result<Value, OctyError> {
    let hp_tuning_job = repos::get_latest_hp_tuning_job(ctx, &account.account_id).await?;

    let Some(job) = hp_tuning_job else {
        return Err(OctyError::new(
            400,
            "An error occurred when generating this churn prediction report.",
            vec![ErrorReason::new(
                "No churn prediction training jobs have been completed. Churn prediction training jobs are automatically run every 24 hours",
                ctx.config.get_str("CHURN_PREDICTION_EXTENDED_HELP")?,
            )],
        ));
    };

    let meta = key(&job, "best_model_meta_data")?.clone();
    let churn_rates = &account.churn_info;

    let updated_at_millis = ejson::date_millis(key(&job, "updated_at")?)
        .ok_or_else(|| OctyError::internal("TypeError: updated_at is not a legacy $date"))?;

    let mut churn_report = json!({
        "training_job_data": {
            "training_job_id": key(&job, "best_model_training_job_id")?,
            "model_accuracy": key(&meta, "eval_score")?,
            "training_job_date": int_to_dt_str(updated_at_millis)?,
        },
        "churn_data": {
            "current_churn_percentage": round1(key(churn_rates, "churn_percentage")?)?,
            "churn_direction_indication": key(churn_rates, "churn_indicator")?,
            "churn_percentage_difference": 0.0,
            "features_of_importance": [],
        }
    });

    // `if churn_rates['churn_difference'] != 0.0` — numeric comparison, so an
    // integer 0 also keeps the 0.0 default; non-numbers fall through to
    // `round(...)` which raised TypeError → 500 in Python (round1 mirrors it).
    let churn_difference = key(churn_rates, "churn_difference")?;
    if churn_difference.as_f64() != Some(0.0) {
        churn_report["churn_data"]["churn_percentage_difference"] = round1(churn_difference)?;
    }

    // `if len(churn_rates['features']) > 0` — non-list raised TypeError → 500.
    let features = key(churn_rates, "features")?;
    let features_list = features
        .as_array()
        .ok_or_else(|| OctyError::internal("TypeError: object of type has no len()"))?;
    if !features_list.is_empty() {
        churn_report["churn_data"]["features_of_importance"] = features.clone();
    }

    Ok(churn_report)
}

/// POST /v1/internal/churn_prediction/delete — remove every churn-prediction
/// document tied to an account (called by the account-deletion fan-out).
pub async fn delete_account_churn_predictions_internal(
    ctx: &Ctx,
    account_id: &str,
) -> Result<bool, OctyError> {
    repos::delete_account_churn_predictions(ctx, account_id).await
}
