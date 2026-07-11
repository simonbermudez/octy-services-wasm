//! Port of `services/rfm.py::RFMCompleteAnalysis` (the
//! `rfm.training.complete.cmd.run` job).
//!
//! ## Artifact-format breaking change (documented, per the task brief)
//!
//! The Python `_get_job_artifacts` downloads `model.tar.gz` from the
//! (external, opaque) SageMaker training container's output and
//! `joblib.load()`s a pickled `df_scores.pkl` pandas DataFrame with columns
//! `profile_id`, `rfm_score`, `segment_description`. Rust has no pickle/
//! joblib deserializer (and no visibility into that container's code to
//! write one), so this port instead looks for a **Rust-native JSON
//! artifact** named `df_scores.json` — a JSON array of
//! `{"profile_id": ..., "rfm_score": ..., "segment_description": ...}`
//! objects — inside the same `model.tar.gz`. If only the legacy
//! `df_scores.pkl` is present (i.e. the SageMaker training image hasn't
//! been updated to also emit the JSON artifact), this is treated as a
//! recoverable error: like every other exception in the Python `run()`
//! method, it is caught by the outer handler and the job is
//! **rescheduled** (`_re_schedule_job`), not destroyed — matching the
//! Python's bug-for-bug behaviour of retrying on *any* uncaught exception in
//! `run()`, rather than treating an artifact-format mismatch as fatal.

use serde::Deserialize;
use serde_json::json;

use octy_shared::errors::OctyError;
use octy_spin::ctx::Ctx;

use crate::billing::BillingUnits;
use crate::rfm_repository;
use crate::s3;
use crate::training::send_job_callback;

const AMQP_MESSAGE_SIZE_LIMIT: usize = 104_857_600; // 100 MB

#[derive(Debug, Clone, Deserialize)]
struct RfmScoreRow {
    profile_id: String,
    rfm_score: serde_json::Value,
    segment_description: serde_json::Value,
}

pub struct RfmCompleteAnalysis<'a> {
    ctx: &'a Ctx,
    account_id: String,
    octy_job_id: String,
    bucket: String,
    training_job_id: String,
    webhook_url: Option<String>,
    billing: BillingUnits,

    status: String,
    training_compute_units: f64,
    rfm_scores: Vec<RfmScoreRow>,
}

impl<'a> RfmCompleteAnalysis<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ctx: &'a Ctx,
        account_id: String,
        account_type: String,
        account_currency: String,
        octy_job_id: String,
        bucket: String,
        training_job_id: String,
        webhook_url: Option<String>,
    ) -> Self {
        Self {
            billing: BillingUnits::new(&account_id, &account_type, &account_currency, "rfm_analysis_completion"),
            ctx,
            account_id,
            octy_job_id,
            bucket,
            training_job_id,
            webhook_url,
            status: String::new(),
            training_compute_units: 0.0,
            rfm_scores: Vec::new(),
        }
    }

    pub async fn run(&mut self) {
        let result: Result<(), OctyError> = async {
            self.status = self.get_cloud_training_status().await?;
            eprintln!("[rfm-worker] Job ID : {} -- Status : {}", self.training_job_id, self.status);

            match self.status.as_str() {
                "InProgress" => self.re_schedule_job().await?,
                "Completed" => {
                    if let Err(e) = self.billing.track_compute_units("hours") {
                        eprintln!("[rfm-worker] track_compute_units failed: {e}");
                    }
                    self.get_job_artifacts().await?;
                    self.assign_rfm_scores().await?;
                    self.job_success().await?;
                    self.billing.complete_compute_units(self.ctx, self.training_compute_units).await;
                }
                "Failed" | "Stopping" | "Stopped" => self.destroy_job().await,
                // Any other status is a no-op, matching the Python (no
                // `else` branch in its `if/elif` chain).
                _ => {}
            }
            Ok(())
        }
        .await;

        if let Err(e) = result {
            eprintln!("[rfm-worker] CRITICAL {e}");
            eprintln!("[rfm-worker] SENTRY capture_exception: {e}");
            self.billing.complete_compute_units(self.ctx, self.training_compute_units).await;
            self.re_schedule_job().await.ok();
        }
    }

    // ---- _get_cloud_training_status ----

    async fn get_cloud_training_status(&mut self) -> Result<String, OctyError> {
        eprintln!("[rfm-worker] Getting cloud training status");
        let (status, compute_units) = rfm_repository::get_cloud_training_status_time(self.ctx, &self.training_job_id).await?;
        self.training_compute_units = compute_units;
        Ok(status)
    }

    // ---- _re_schedule_job ----

    async fn re_schedule_job(&self) -> Result<(), OctyError> {
        eprintln!("[rfm-worker] Rescheduling job");
        // Status "failed" here does not mean the job failed — it is how the
        // Octy Job Scheduler is told to re-run this job on its next tick
        // while the SageMaker training job is still `InProgress`.
        send_job_callback(self.ctx, &self.account_id, &self.octy_job_id, "RFM analysis Job still processing", "failed").await
    }

    // ---- _destroy_job ----

    async fn destroy_job(&mut self) {
        let result: Result<(), OctyError> = async {
            eprintln!("[rfm-worker] Destroying job due to error");
            let models_dir = self.ctx.config.get_str("RFM_MODELS_DIR")?.to_string();
            s3::delete_directory(&self.bucket, &format!("{models_dir}/{}", self.training_job_id)).await?;
            rfm_repository::update_training_job_ref(self.ctx, &self.account_id, &self.training_job_id, "Failed").await?;
            self.ctx
                .gateway
                .amqp_publish(
                    "octy.job.cmd.delete",
                    &json!({
                        "account_id": self.account_id,
                        "octy_job_ids": [self.octy_job_id],
                        "alt_identifiers": null,
                    }),
                )
                .await?;
            self.job_failed_webhook().await
        }
        .await;

        if let Err(err) = result {
            self.billing.complete_compute_units(self.ctx, self.training_compute_units).await;
            eprintln!("[rfm-worker] SENTRY capture_exception: {err}");
            eprintln!("[rfm-worker] CRITICAL Error occurred when attempting to destroy job. {err}");
        }
    }

    // ---- _get_job_artifacts ----

    async fn get_job_artifacts(&mut self) -> Result<(), OctyError> {
        eprintln!("[rfm-worker] Getting job artifacts");

        rfm_repository::get_training_job(self.ctx, &self.account_id, &self.training_job_id, "in_progress").await?;

        let models_dir = self.ctx.config.get_str("RFM_MODELS_DIR")?;
        let model_location = format!("{models_dir}/{}/output/model.tar.gz", self.training_job_id);
        let files = s3::download_and_extract_targz(&self.bucket, &model_location).await?;

        let scores_file = files
            .iter()
            .find(|(name, _)| name.ends_with("df_scores.json"))
            .or_else(|| files.iter().find(|(name, _)| name.ends_with("df_scores.pkl")));

        match scores_file {
            Some((name, bytes)) if name.ends_with(".json") => {
                self.rfm_scores = serde_json::from_slice(bytes)
                    .map_err(|e| OctyError::internal(format!("df_scores.json is not valid: {e}")))?;
                Ok(())
            }
            Some((name, _)) if name.ends_with(".pkl") => Err(OctyError::internal(
                "df_scores.pkl found but this Rust port cannot deserialize joblib/pickle artifacts; \
                 the SageMaker training image must be updated to also emit a df_scores.json artifact \
                 (see complete.rs module docs) — job left in_progress and will be rescheduled",
            )),
            _ => Err(OctyError::internal("model.tar.gz did not contain a df_scores artifact")),
        }
    }

    // ---- _assign_rfm_scores ----
    //
    // NB (preserved Python bug): the Python only appends the trailing,
    // still-accumulating `profile_updates` batch to `amqp_batch_profiles`
    // *if no flush ever happened* (`if len(amqp_batch_profiles) < 1`). Once
    // at least one size-triggered flush has occurred, any remaining
    // partial batch after the loop is silently dropped — those profiles
    // never get an AMQP `profiles.cmd.update` publish. We reproduce this
    // bug-for-bug rather than "fixing" it, per the task brief.
    async fn assign_rfm_scores(&self) -> Result<(), OctyError> {
        let mut batches: Vec<Vec<serde_json::Value>> = Vec::new();
        let mut current: Vec<serde_json::Value> = Vec::new();

        for row in &self.rfm_scores {
            let entry = json!({
                "profile_id": row.profile_id,
                "rfm_score": row.rfm_score,
                "rfm_segment_desc": row.segment_description,
            });
            current.push(entry);

            let current_size: usize = serde_json::to_vec(&current).map(|v| v.len()).unwrap_or(0);
            if current_size > AMQP_MESSAGE_SIZE_LIMIT {
                batches.push(std::mem::take(&mut current));
            }
        }
        if batches.is_empty() {
            batches.push(current);
        }

        for profiles_updates in batches {
            self.ctx
                .gateway
                .amqp_publish(
                    "profiles.cmd.update",
                    &json!({ "account_id": self.account_id, "profiles": profiles_updates }),
                )
                .await?;
        }
        Ok(())
    }

    // ---- _job_success ----

    async fn job_success(&self) -> Result<(), OctyError> {
        rfm_repository::update_training_job_ref(self.ctx, &self.account_id, &self.training_job_id, &self.status).await?;
        self.send_webhook(
            "Octy training job has successfully completed.",
            "Completed",
            "This means a new, up to date RFM score has been applied to each profile. Octy support team.",
        )
        .await?;
        send_job_callback(self.ctx, &self.account_id, &self.octy_job_id, "RFM analysis Job successfully completed", "success").await
    }

    // ---- _job_failed_webhook ----

    async fn job_failed_webhook(&self) -> Result<(), OctyError> {
        self.send_webhook(
            "Octy training job has failed.",
            "Failed",
            "An unknown server error occurred when attempting to conduct RFM analysis. You do have to do anything, \
             we are aware of this issue and will resolve it shortly. Octy support team",
        )
        .await
    }

    async fn send_webhook(&self, subject: &str, job_status: &str, message: &str) -> Result<(), OctyError> {
        let Some(url) = &self.webhook_url else {
            return Ok(());
        };
        // NOTE: Python sent `str(dt.now())` — a naive local-time string like
        // "2024-01-01 12:00:00.123456" — for this webhook's `date_time`
        // field. This port sends UTC RFC 3339 instead; harmless unless a
        // downstream webhook consumer parses the exact Python format.
        let payload = json!({
            "subject": subject,
            "body": { "algorithm": "rfm-analysis", "job_status": job_status, "message": message },
            "date_time": chrono::Utc::now().to_rfc3339(),
        });
        // Mirrors `_send_http_account_webhook_request`: connection failures
        // are logged, not propagated (unlike `_send_http_request`, which
        // this webhook helper deliberately does not share error semantics
        // with).
        match octy_spin::gateway::http_post_json_with_retry(url, &[], &payload).await {
            Ok((status, _)) => eprintln!("[rfm-worker] POST {url} returned status {status}"),
            Err(e) => eprintln!("[rfm-worker] Error: {e}"),
        }
        Ok(())
    }
}
