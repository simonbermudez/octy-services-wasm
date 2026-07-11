//! Port of `services/rfm.py::RFMAnalysis` (the `rfm.training.cmd.run` job).
//!
//! `run()` mirrors the Python method exactly in control flow: every stage
//! error is caught, logged, billed (`complete_compute_units`) and disposes
//! of the job (`_dispose_job`) — `run()` itself never returns an error to
//! its caller, matching the Python (whose `except Exception` swallows
//! everything). See `amqp.rs` for how that interacts with AMQP ack/reject.

use serde_json::json;
use std::collections::HashSet;

use octy_shared::errors::OctyError;
use octy_shared::utils::generate_uid;
use octy_spin::ctx::Ctx;

use crate::billing::BillingUnits;
use crate::csv_util::{training_rows_to_csv, TrainingRow};
use crate::rfm_repository;
use crate::s3;
use crate::sagemaker::TrainingResource;
use crate::util::{required_gb, str_to_dt};

pub struct RfmAnalysis<'a> {
    ctx: &'a Ctx,
    account_id: String,
    octy_job_id: String,
    bucket: String,
    billing: BillingUnits,
    training_job_id: String,
    data_timeframe: i64,

    training_rows: Vec<TrainingRow>,
    training_csv: Option<String>,
    training_resources: Vec<TrainingResource>,
    total_bytes: f64,
}

impl<'a> RfmAnalysis<'a> {
    pub fn new(ctx: &'a Ctx, account_id: String, account_type: String, account_currency: String, octy_job_id: String, bucket: String) -> Result<Self, OctyError> {
        let data_timeframe = ctx.config.get_i64("DATA_SET_TIMEFRAME")?;
        Ok(Self {
            ctx,
            billing: BillingUnits::new(&account_id, &account_type, &account_currency, "rfm_analysis"),
            account_id,
            octy_job_id,
            bucket,
            training_job_id: generate_uid("training-job"),
            data_timeframe,
            training_rows: Vec::new(),
            training_csv: None,
            training_resources: Vec::new(),
            total_bytes: 0.0,
        })
    }

    pub async fn run(&mut self) {
        if let Err(e) = self.billing.track_compute_units("hours") {
            eprintln!("[rfm-worker] track_compute_units failed: {e}");
        }

        let result: Result<(), OctyError> = async {
            self.build_training_df().await?;
            self.training_df_validation()?;
            self.create_csv_objects()?;
            self.upload_resources().await?;
            self.start_cloud_training_job().await?;
            self.create_training_job_ref().await?;
            self.complete_job().await?;
            Ok(())
        }
        .await;

        match result {
            Ok(()) => {
                self.billing.complete_compute_units(self.ctx, 0.0).await;
                eprintln!("[rfm-worker] Completed Job!");
            }
            Err(e) => {
                eprintln!("[rfm-worker] SENTRY capture_exception: {e}");
                eprintln!("[rfm-worker] CRITICAL {e}");
                self.billing.complete_compute_units(self.ctx, 0.0).await;
                self.dispose_job(&e.to_string()).await;
            }
        }
    }

    // ---- _build_training_df ----

    async fn build_training_df(&mut self) -> Result<(), OctyError> {
        eprintln!("[rfm-worker] Building RFM analysis dataset...");
        let min_items = self.ctx.config.get_i64("MIN_NUM_ITEMS")?;
        let items = rfm_repository::get_items(self.ctx, &self.account_id).await?;
        if (items.len() as i64) < min_items {
            return Err(OctyError::internal("Not enough items found."));
        }

        let min_profiles = self.ctx.config.get_i64("MIN_NUM_PROFILES")?;
        let profile_ids = rfm_repository::get_profile_ids(self.ctx, &self.account_id).await?;
        if (profile_ids.len() as i64) < min_profiles {
            return Err(OctyError::internal("Not enough active profiles found."));
        }

        let min_events = self.ctx.config.get_i64("MIN_NUM_EVENTS_COLLECTIVE")?;
        let events = rfm_repository::get_events(self.ctx, &self.account_id, &profile_ids, self.data_timeframe, "charged").await?;
        if (events.len() as i64) < min_events {
            return Err(OctyError::internal("Not enough events found to conduct rfm analysis."));
        }

        for event in &events {
            let Some(row) = build_event_item_row(event, &items) else { continue };
            // `training_df.drop(zero_indicies)` — rows with item_price == 0 are dropped.
            if row.item_price != 0.0 {
                self.training_rows.push(row);
            }
        }
        eprintln!("[rfm-worker] Created RFM analysis dataset!");
        Ok(())
    }

    // ---- _training_df_validation ----

    fn training_df_validation(&self) -> Result<(), OctyError> {
        self.validate_recency_unique()?;
        self.validate_frequency_unique()?;
        self.validate_monetary_unique()?;
        Ok(())
    }

    fn validate_recency_unique(&self) -> Result<(), OctyError> {
        use std::collections::HashMap;
        let mut max_by_profile: HashMap<&str, chrono::DateTime<chrono::Utc>> = HashMap::new();
        for row in &self.training_rows {
            max_by_profile
                .entry(row.profile_id.as_str())
                .and_modify(|v| *v = (*v).max(row.created_at))
                .or_insert(row.created_at);
        }
        let global_max = max_by_profile.values().cloned().max();
        let Some(global_max) = global_max else {
            return Err(OctyError::internal("Number of required unique recency days not met. >10 required."));
        };
        let recency_days: HashSet<i64> = max_by_profile.values().map(|v| (global_max - *v).num_days()).collect();
        if recency_days.len() < 10 {
            return Err(OctyError::internal("Number of required unique recency days not met. >10 required."));
        }
        Ok(())
    }

    fn validate_frequency_unique(&self) -> Result<(), OctyError> {
        use std::collections::HashMap;
        let mut counts: HashMap<&str, i64> = HashMap::new();
        for row in &self.training_rows {
            *counts.entry(row.profile_id.as_str()).or_insert(0) += 1;
        }
        let unique: HashSet<i64> = counts.values().cloned().collect();
        if unique.len() < 10 {
            return Err(OctyError::internal("Number of required unique frequent events not met. >10 required."));
        }
        Ok(())
    }

    fn validate_monetary_unique(&self) -> Result<(), OctyError> {
        use std::collections::HashMap;
        let mut sums: HashMap<&str, f64> = HashMap::new();
        for row in &self.training_rows {
            *sums.entry(row.profile_id.as_str()).or_insert(0.0) += row.item_price;
        }
        // pandas' `Series.unique()` compares by exact bit pattern; mirror
        // that instead of coercing to a lossy rounded key.
        let unique: HashSet<u64> = sums.values().map(|v| v.to_bits()).collect();
        if unique.len() < 10 {
            return Err(OctyError::internal("Number of required unique monetary amounts not met. >10 required."));
        }
        Ok(())
    }

    // ---- _create_csv_objects ----

    fn create_csv_objects(&mut self) -> Result<(), OctyError> {
        let csv = training_rows_to_csv(&self.training_rows).map_err(|e| OctyError::internal(format!("csv encode failed: {e}")))?;
        self.billing.track_data_units(&csv);
        self.training_csv = Some(csv);
        Ok(())
    }

    // ---- _upload_resources ----
    //
    // NOTE (gateway capability gap): the Python service branches into a
    // chunked S3 multipart upload for files between `MIN_FILE_SIZE` (15MB)
    // and `MAX_FILE_SIZE` (50GB). The data gateway only exposes whole-object
    // `put-object` / `get-object` (no `create-multipart-upload` /
    // `upload-part` / `complete-multipart-upload`), so this port always
    // performs a single `put-object` call for any file under
    // `MAX_FILE_SIZE`, and still raises the same "Invalid file size" error
    // above that threshold. See the final report for the suggested gateway
    // endpoints if true multipart support is needed for very large datasets.
    async fn upload_resources(&mut self) -> Result<(), OctyError> {
        eprintln!("[rfm-worker] Uploading training job resources");
        let csv = self
            .training_csv
            .clone()
            .ok_or_else(|| OctyError::internal("no training csv built"))?;
        let file_size = csv.len() as f64;
        let max_file_size = self.ctx.config.get_i64("MAX_FILE_SIZE")? as f64;
        if file_size >= max_file_size {
            return Err(OctyError::internal(format!(
                "Invalid file size. File size exceeds maximum. Account ID: {} File type : training",
                self.account_id
            )));
        }

        let key = generate_file_key(self.ctx, "training", &self.training_job_id)?;
        s3::put_object(&self.bucket, &key, csv.as_bytes(), "text/csv").await?;

        self.training_resources.push(TrainingResource {
            channel_name: "training".to_string(),
            training_resource_location: key,
        });
        self.total_bytes += file_size;

        self.billing.complete_data_units(self.ctx, "MB").await?;
        eprintln!("[rfm-worker] Uploaded training job resources!");
        Ok(())
    }

    // ---- _create_training_job_ref ----

    async fn create_training_job_ref(&self) -> Result<(), OctyError> {
        rfm_repository::create_training_job_ref(self.ctx, &self.training_job_id, &self.account_id).await
    }

    // ---- _start_cloud_training_job ----

    async fn start_cloud_training_job(&self) -> Result<(), OctyError> {
        eprintln!("[rfm-worker] Starting cloud training");
        let volume_size = required_gb(self.total_bytes);
        rfm_repository::start_cloud_training(
            self.ctx,
            &self.account_id,
            &self.training_job_id,
            volume_size,
            &self.training_resources,
            &self.bucket,
        )
        .await
    }

    // ---- _complete_job ----

    async fn complete_job(&self) -> Result<(), OctyError> {
        eprintln!("[rfm-worker] Training job complete");
        send_job_callback(self.ctx, &self.account_id, &self.octy_job_id, "RFM analysis Job suceeded", "success").await?;

        self.ctx
            .gateway
            .amqp_publish(
                "octy.job.cmd.create",
                &json!({
                    "account_id": self.account_id,
                    "job_meta": {
                        "job_type": "rfm",
                        "amqp_routing_key": "rfm.training.complete.cmd.run",
                        "required_permissions": ["rfm"],
                        "required_configurations": {
                            "account_attributes": [
                                "account_configurations.webhook_url",
                                "account_configurations.account_type",
                                "account_configurations.account_currency",
                                "bucket"
                            ],
                            "algorithm_configuration_idxs": []
                        },
                        "desired_runs": 1,
                        "time_interval": 30,
                        "fail_threshold": 3
                    },
                    "job_data": { "training_job_id": self.training_job_id }
                }),
            )
            .await
    }

    // ---- _dispose_job ----

    async fn dispose_job(&mut self, ex: &str) {
        let result: Result<(), OctyError> = async {
            rfm_repository::delete_training_job_ref(self.ctx, &self.account_id, &self.training_job_id).await?;
            // `abort_multipart_upload`: a no-op in this port since uploads
            // never use S3 multipart (see `upload_resources`); the Python
            // version silently swallows abort failures for single uploads
            // (no MPU id) anyway, so the net effect is identical.
            send_job_callback(
                self.ctx,
                &self.account_id,
                &self.octy_job_id,
                &format!("RFM analysis Job failed. EX :: {ex}"),
                "failed",
            )
            .await
        }
        .await;

        if let Err(err) = result {
            self.billing.complete_compute_units(self.ctx, 0.0).await;
            eprintln!("[rfm-worker] SENTRY capture_exception: {err}");
            eprintln!("[rfm-worker] CRITICAL Error occurred when attempting to dispose of job. {err}");
        }
    }
}

fn build_event_item_row(event: &serde_json::Value, items: &[serde_json::Value]) -> Option<TrainingRow> {
    let profile_id = event.get("profile_id")?.as_str()?.to_string();
    let created_at_raw = event.get("created_at")?.as_str()?;
    let created_at = str_to_dt(created_at_raw)?;

    let mut item_price = 0.0;
    let props = event.get("event_properties");
    let has_props = match props {
        None => false,
        Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::String(s)) => !(s.is_empty() || s == "\"\""),
        Some(serde_json::Value::Object(map)) => !map.is_empty(),
        _ => true,
    };
    if has_props {
        if let Some(serde_json::Value::Object(map)) = props {
            if let Some(item_id) = map.get("item_id") {
                if let Some(item) = items.iter().find(|i| i.get("item_id") == Some(item_id)) {
                    item_price = item.get("item_price").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
                }
            }
        }
    }

    Some(TrainingRow { profile_id, item_price, created_at })
}

/// `_generate_file_key`. Extension is picked by a `"meta_data"` substring
/// match on the resource name (else `.csv`) — inherited from Python; this
/// port only ever names the resource `"training"`, so it always lands on `.csv`.
fn generate_file_key(ctx: &Ctx, resource_friendly_name: &str, training_job_id: &str) -> Result<String, OctyError> {
    let data_dir = ctx.config.get_str("RFM_DATA_DIR")?;
    let key = generate_uid("key");
    let ext = if resource_friendly_name.contains("meta_data") { "json" } else { "csv" };
    Ok(format!("{data_dir}/{training_job_id}/{key}.{ext}"))
}

/// `_send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', ...)`.
pub async fn send_job_callback(ctx: &Ctx, account_id: &str, octy_job_id: &str, message: &str, status: &str) -> Result<(), OctyError> {
    let url = format!("{}/v1/internal/jobs/callback", ctx.config.get_str("OCTY_JOB_SERVICE_CLUSTER_IP")?);
    let payload = json!({
        "account_id": account_id,
        "octy_job_id": octy_job_id,
        "message": message,
        "status": status,
    });
    let (status_code, _) = octy_spin::gateway::http_post_json_with_retry(&url, &[], &payload).await?;
    eprintln!("[rfm-worker] POST {url} returned status {status_code}");
    Ok(())
}
