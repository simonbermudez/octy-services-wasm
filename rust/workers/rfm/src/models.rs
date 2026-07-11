//! Port of `data/models/rfm_jobs.py` — AMQP job payload shapes.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AccountData {
    pub account_id: String,
    #[serde(default)]
    pub webhook_url: Option<String>,
    pub account_type: String,
    pub account_currency: String,
    pub bucket: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RfmCompleteJobData {
    pub training_job_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RfmAnalysisJob {
    pub account_data: AccountData,
    pub octy_job_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RfmCompleteJob {
    pub account_data: AccountData,
    pub job_data: RfmCompleteJobData,
    pub octy_job_id: String,
}
