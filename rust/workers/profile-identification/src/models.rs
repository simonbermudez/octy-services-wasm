//! Port of `data/models/profile_iden_jobs.py` — the AMQP job payload shape
//! delivered on `profile.identification.cmd.run`.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AccountData {
    pub account_id: String,
    pub webhook_url: String,
    pub account_type: String,
    pub account_currency: String,
    pub authenticated_id_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProfileIdenJob {
    pub account_data: AccountData,
    pub octy_job_id: String,
}
