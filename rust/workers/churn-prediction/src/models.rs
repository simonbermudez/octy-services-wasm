//! Port of `data/models/churn_jobs.py` — AMQP job payload request models
//! (pydantic `BaseModel` → `serde::Deserialize`). Field presence / type
//! mismatches are surfaced as a parse error, matching pydantic's
//! `ValidationError` on missing/invalid fields (the account service maps
//! that to a 400 reject-no-requeue in `amqp.rs`).

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct AccountData {
    pub account_id: String,
    #[serde(default)]
    pub webhook_url: Option<String>,
    pub account_type: String,
    pub account_currency: String,
    pub bucket: String,
    /// `Any` in the Python (int or float) — kept as raw JSON, coerced where used.
    pub churn_percentage: Value,
    pub algorithm_configurations: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChurnCompleteJobData {
    pub hyperparam_tuning_job_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChurnTrainingJob {
    pub account_data: AccountData,
    pub octy_job_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChurnCompleteJob {
    pub account_data: AccountData,
    pub job_data: ChurnCompleteJobData,
    pub octy_job_id: String,
}

/// `AccountData.churn_percentage` is `Any` in Python; the completion job
/// reads it as a number for the churn-diff calculation.
pub fn churn_percentage_f64(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}
