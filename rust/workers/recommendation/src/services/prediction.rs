//! Port of `services/recommendation.py` — `RecommenderCompleteTrainingJob`.
//!
//! Polls the SageMaker tuning-job status; on `Completed` downloads the model
//! artifacts, scores every profile against every seen item with the LightFM
//! dot-product formula, caches the top-N recommendations, and reports back.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

use octy_spin::ctx::Ctx;

use crate::artifacts::parse_job_artifacts;
use crate::billing::BillingUnits;
use crate::http::post_json_with_retry;
use crate::models::AlgorithmConfigurations;
use crate::repos::bucket;
use crate::repos::recommendation as repo;
use crate::repos::recommendation::Prediction;
use crate::utils::now_str;

const NUM_REC: usize = 25;

pub struct RecommenderCompleteTrainingJob<'a> {
    ctx: &'a Ctx,
    account_id: String,
    algorithm_configurations: Map<String, Value>,
    octy_job_id: String,
    hyperparam_tuning_job_id: String,
    bucket: String,
    webhook_url: String,
    billing: BillingUnits,
    status: String,
    data_timeframe: i64,
    base_item_stop_list: Vec<String>,
    training_compute_units: f64,
}

impl<'a> RecommenderCompleteTrainingJob<'a> {
    pub fn new(
        ctx: &'a Ctx,
        account_id: String,
        account_type: String,
        account_currency: String,
        algorithm_configurations: Map<String, Value>,
        octy_job_id: String,
        hyperparam_tuning_job_id: String,
        bucket: String,
        webhook_url: String,
    ) -> Result<Self> {
        let data_timeframe = ctx
            .config
            .get_i64("DATA_SET_TIMEFRAME")
            .map_err(|e| anyhow!("{e}"))?;
        let base_item_stop_list = AlgorithmConfigurations(&algorithm_configurations).item_id_stop_list()?;
        Ok(Self {
            ctx,
            account_id: account_id.clone(),
            algorithm_configurations,
            octy_job_id,
            hyperparam_tuning_job_id,
            bucket,
            webhook_url,
            billing: BillingUnits::new(&account_id, &account_type, &account_currency, "recommender_completion"),
            status: "InProgress".to_string(),
            data_timeframe,
            base_item_stop_list,
            training_compute_units: 0.0,
        })
    }

    fn algo(&self) -> AlgorithmConfigurations<'_> {
        AlgorithmConfigurations(&self.algorithm_configurations)
    }

    async fn send_job_callback(&self, message: &str, status: &str) -> Result<()> {
        let url = format!(
            "{}/v1/internal/jobs/callback",
            self.ctx
                .config
                .get_str("OCTY_JOB_SERVICE_CLUSTER_IP")
                .map_err(|e| anyhow!("{e}"))?
        );
        let payload = json!({
            "account_id": self.account_id,
            "octy_job_id": self.octy_job_id,
            "message": message,
            "status": status,
        });
        post_json_with_retry(&url, &[], &payload).await?;
        Ok(())
    }

    /// `_send_http_account_webhook_request` — errors are logged, never raised.
    async fn send_webhook(&self, subject: &str, algorithm: &str, job_status: &str, message: &str) {
        let payload = json!({
            "subject": subject,
            "body": {
                "algorithm": algorithm,
                "job_status": job_status,
                "message": message,
            },
            "date_time": now_str(),
        });
        if let Err(e) = post_json_with_retry(&self.webhook_url, &[], &payload).await {
            eprintln!("[recommendation-worker] webhook request failed: {e}");
        }
    }

    async fn job_failed_webhook(&self) {
        self.send_webhook(
            "Octy training job has failed.",
            "recommendations",
            "Failed",
            "An unknown server error occurred when attempting to train a model using this algorithm. You do have to do anything, we are aware of this issue and will resolve it shortly. Octy support team",
        )
        .await;
    }

    async fn job_success(&self, best_training_job: &Value, model_meta: &Value) -> Result<()> {
        let training_job_name = best_training_job
            .get("training_job_name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("best_training_job missing training_job_name"))?;
        repo::update_hparam_tuning_job_ref(
            self.ctx,
            &self.account_id,
            &self.hyperparam_tuning_job_id,
            training_job_name,
            &self.status,
            Some(model_meta),
        )
        .await?;
        self.send_webhook(
            "Octy training job has successfully completed.",
            "recommendations",
            "Completed",
            "This means a new, up to date recommendations model is available for predictions. Octy support team.",
        )
        .await;
        self.send_job_callback("Recommender training Job successfully completed", "success")
            .await?;
        Ok(())
    }

    async fn re_schedule_job(&self) -> Result<()> {
        self.send_job_callback(
            "Recommender training Job and or Recommender hyper parameter job still processing",
            "failed",
        )
        .await
    }

    async fn destroy_job(&mut self) {
        let result: Result<()> = async {
            let rec_data_dir = self
                .ctx
                .config
                .get_str("REC_DATA_DIR")
                .map_err(|e| anyhow!("{e}"))?;
            bucket::delete_directory(
                &self.bucket,
                &format!("{rec_data_dir}/{}", self.hyperparam_tuning_job_id),
            )
            .await?;
            repo::update_hparam_tuning_job_ref(
                self.ctx,
                &self.account_id,
                &self.hyperparam_tuning_job_id,
                "--",
                "Failed",
                None,
            )
            .await?;
            if let Err(e) = self
                .ctx
                .gateway
                .amqp_publish(
                    "octy.job.cmd.delete",
                    &json!({
                        "account_id": self.account_id,
                        "octy_job_ids": [self.octy_job_id],
                        "alt_identifiers": Value::Null,
                    }),
                )
                .await
            {
                eprintln!("[recommendation-worker] failed to publish octy.job.cmd.delete: {e}");
            }
            self.job_failed_webhook().await;
            Ok(())
        }
        .await;

        if let Err(err) = result {
            let _ = self.billing.complete_compute_units(self.ctx, self.training_compute_units).await;
            eprintln!("[recommendation-worker] Error occurred when attempting to destroy job. {err}");
        }
    }

    fn filter_lfm_idx_mappings<'m>(
        mappings: &'m [Value],
        type_: Option<&str>,
        id_: Option<&str>,
    ) -> Vec<&'m Value> {
        mappings
            .iter()
            .filter(|entry| match (type_, id_) {
                (_, Some(id_)) => entry.get("res_id").and_then(Value::as_str) == Some(id_),
                (Some(t), None) => entry.get("type_").and_then(Value::as_str) == Some(t),
                (None, None) => false,
            })
            .collect()
    }

    async fn make_predictions(&mut self) -> Result<()> {
        self.billing.track_compute_units("hours")?;

        // ---- get_job_artifacts ----
        let hp_tuning_job = repo::get_hparam_tuning_job_ref(
            self.ctx,
            &self.account_id,
            &self.hyperparam_tuning_job_id,
            "in_progress",
        )
        .await?;
        let (best_training_job, training_compute_units) =
            repo::get_best_training_job(self.ctx, &self.hyperparam_tuning_job_id).await?;
        self.training_compute_units = training_compute_units;

        let training_job_name = best_training_job
            .get("training_job_name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("best_training_job missing training_job_name"))?;
        let rec_models_dir = self
            .ctx
            .config
            .get_str("REC_MODELS_DIR")
            .map_err(|e| anyhow!("{e}"))?;
        let model_location = format!("{rec_models_dir}/{training_job_name}/output/model.tar.gz");
        let files = bucket::download_resource_compressed(&self.bucket, &model_location).await?;
        let artifacts = parse_job_artifacts(&files)?;

        // ---- get_lfm_idx_mappings ----
        let lfm_idx_mappings: Vec<Value> = hp_tuning_job
            .get("lfm_idxs")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| anyhow!("hyper-parameter tuning job ref missing lfm_idxs"))?;

        // ---- get_items ----
        let all_items_raw = repo::get_items(self.ctx, &self.account_id, true, "active").await?;
        if all_items_raw.is_empty() {
            bail!("There are currently no active items associated with this account.");
        }
        let all_items: HashSet<String> = all_items_raw.iter().map(repo::value_as_string).collect();
        let seen_items: Vec<(String, usize)> = Self::filter_lfm_idx_mappings(&lfm_idx_mappings, Some("items"), None)
            .into_iter()
            .filter_map(|entry| {
                let res_id = entry.get("res_id").and_then(Value::as_str)?.to_string();
                let lfm_idx = entry.get("lfm_idx").and_then(Value::as_i64)? as usize;
                Some((res_id, lfm_idx))
            })
            .collect();

        // ---- get_profiles ----
        let profiles_raw = repo::get_profiles(self.ctx, &self.account_id, true).await?;
        if profiles_raw.is_empty() {
            bail!("There are currently no active profiles associated with this account.");
        }
        let profile_ids: Vec<String> = profiles_raw.iter().map(repo::value_as_string).collect();

        // ---- apply_profile_lfm_idx_map ----
        let mut mapped: Vec<(String, usize)> = Vec::new();
        for profile_id in &profile_ids {
            let matches = Self::filter_lfm_idx_mappings(&lfm_idx_mappings, None, Some(profile_id));
            if let Some(entry) = matches.first() {
                if let Some(idx) = entry.get("lfm_idx").and_then(Value::as_i64) {
                    mapped.push((profile_id.clone(), idx as usize));
                }
            }
        }
        if mapped.is_empty() {
            bail!("Error occurred when attempting to make item recommendations. No active profiles found.");
        }

        // ---- get_profile_events (only when !recommend_interacted_items) ----
        let recommend_interacted_items = self.algo().recommend_interacted_items()?;
        let mut events_by_profile: HashMap<String, Vec<Value>> = HashMap::new();
        if !recommend_interacted_items {
            let events = repo::get_events(self.ctx, &self.account_id, &profile_ids, self.data_timeframe, "charged")
                .await?;
            if events.is_empty() {
                bail!("There are currently no events associated with the provided profile ids.");
            }
            for event in events {
                if let Some(pid) = event.get("profile_id").and_then(Value::as_str) {
                    events_by_profile.entry(pid.to_string()).or_default().push(event);
                }
            }
        }

        // ---- apply_prediction_scores ----
        let rec_item_identifier = self.algo().rec_item_identifier()?;
        let predictions: Vec<Prediction> = mapped
            .iter()
            .map(|(profile_id, profile_lfm_idx)| {
                let profile_stop_list = self.build_stop_list(
                    recommend_interacted_items,
                    &rec_item_identifier,
                    events_by_profile.get(profile_id),
                );

                let user = artifacts
                    .model
                    .users_embeddings
                    .get(*profile_lfm_idx)
                    .cloned()
                    .unwrap_or_default();
                let user_bias = artifacts.model.users_biases.get(*profile_lfm_idx).copied().unwrap_or(0.0);

                let mut item_scores: Vec<(String, f64)> = seen_items
                    .iter()
                    .map(|(item_id, item_idx)| {
                        let item_e = artifacts.model.items_embeddings.get(*item_idx).cloned().unwrap_or_default();
                        let item_bias = artifacts.model.items_biases.get(*item_idx).copied().unwrap_or(0.0);
                        let dot: f64 = user.iter().zip(item_e.iter()).map(|(a, b)| a * b).sum();
                        (item_id.clone(), dot + user_bias + item_bias)
                    })
                    .collect();

                item_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                let filtered: Vec<(String, f64)> = item_scores
                    .into_iter()
                    .filter(|(item_id, _)| !profile_stop_list.contains(item_id) && all_items.contains(item_id))
                    .take(NUM_REC)
                    .collect();

                Prediction {
                    profile_id: profile_id.clone(),
                    item_scores: filtered,
                }
            })
            .collect();

        // ---- cache + finish ----
        repo::cache_item_recommendations(self.ctx, &self.account_id, training_job_name, &predictions).await?;
        self.job_success(&best_training_job, &artifacts.model_meta).await?;
        self.billing
            .complete_compute_units(self.ctx, self.training_compute_units)
            .await?;
        Ok(())
    }

    fn build_stop_list(
        &self,
        recommend_interacted_items: bool,
        rec_item_identifier: &str,
        events: Option<&Vec<Value>>,
    ) -> HashSet<String> {
        let mut stop_list: HashSet<String> = self.base_item_stop_list.iter().cloned().collect();
        if recommend_interacted_items {
            return stop_list;
        }
        let Some(events) = events else { return stop_list };
        for event in events {
            let props = event.get("event_properties").cloned().unwrap_or(Value::Null);
            let skip = matches!(&props, Value::Null)
                || matches!(&props, Value::String(s) if s.is_empty() || s == "\"\"");
            if skip {
                continue;
            }
            let Some(props_obj) = props.as_object() else { continue };
            for (k, v) in props_obj {
                if k == rec_item_identifier {
                    if let Some(item_id) = v.as_str() {
                        stop_list.insert(item_id.to_string());
                    }
                }
            }
        }
        stop_list
    }

    pub async fn run(mut self) {
        let result: Result<()> = async {
            self.status = repo::get_hparam_tuning_job_status(self.ctx, &self.hyperparam_tuning_job_id).await?;

            match self.status.as_str() {
                "InProgress" => self.re_schedule_job().await?,
                "Completed" => self.make_predictions().await?,
                "Failed" | "Stopping" | "Stopped" => self.destroy_job().await,
                other => {
                    eprintln!("[recommendation-worker] unrecognized tuning job status: {other}");
                }
            }
            Ok(())
        }
        .await;

        if let Err(e) = result {
            eprintln!("[recommendation-worker] {e}");
            let _ = self
                .billing
                .complete_compute_units(self.ctx, self.training_compute_units)
                .await;
            let _ = self.re_schedule_job().await;
        }
    }
}
