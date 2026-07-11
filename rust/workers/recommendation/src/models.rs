//! Port of `data/models/rec_jobs.py` (pydantic job payload models).
//!
//! All fields are required, like the pydantic models — a missing field makes
//! the delivery unparseable and it is rejected without requeue.

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Deserialize)]
pub struct AccountData {
    pub account_id: String,
    pub webhook_url: String,
    pub account_type: String,
    pub account_currency: String,
    pub bucket: String,
    pub algorithm_configurations: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecCompleteJobData {
    pub hyperparam_tuning_job_id: String,
}

// ------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct RecTrainingJob {
    pub account_data: AccountData,
    pub octy_job_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecCompleteJob {
    pub account_data: AccountData,
    pub job_data: RecCompleteJobData,
    pub octy_job_id: String,
}

/// Typed accessors over `algorithm_configurations` — the Python indexed the
/// dict directly (`KeyError` → job failure), so missing keys are errors here.
pub struct AlgorithmConfigurations<'a>(pub &'a Map<String, Value>);

impl<'a> AlgorithmConfigurations<'a> {
    fn get(&self, key: &str) -> Result<&'a Value> {
        self.0
            .get(key)
            .ok_or_else(|| anyhow!("missing algorithm configuration key: {key}"))
    }

    pub fn profile_features(&self) -> Result<Vec<String>> {
        Ok(self
            .get("profile_features")?
            .as_array()
            .ok_or_else(|| anyhow!("algorithm_configurations.profile_features is not a list"))?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect())
    }

    pub fn event_type(&self) -> Result<String> {
        Ok(self
            .get("event_type")?
            .as_str()
            .ok_or_else(|| anyhow!("algorithm_configurations.event_type is not a string"))?
            .to_string())
    }

    pub fn rec_item_identifier(&self) -> Result<String> {
        Ok(self
            .get("rec_item_identifier")?
            .as_str()
            .ok_or_else(|| anyhow!("algorithm_configurations.rec_item_identifier is not a string"))?
            .to_string())
    }

    /// `item_id_stop_list` — a list of `{"item_id": …}` objects or `null`.
    pub fn item_id_stop_list(&self) -> Result<Vec<String>> {
        match self.get("item_id_stop_list")? {
            Value::Null => Ok(Vec::new()),
            Value::Array(entries) => Ok(entries
                .iter()
                .filter_map(|entry| entry.get("item_id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()),
            _ => Err(anyhow!("algorithm_configurations.item_id_stop_list is not a list or null")),
        }
    }

    pub fn recommend_interacted_items(&self) -> Result<bool> {
        self.get("recommend_interacted_items")?
            .as_bool()
            .ok_or_else(|| anyhow!("algorithm_configurations.recommend_interacted_items is not a bool"))
    }
}
