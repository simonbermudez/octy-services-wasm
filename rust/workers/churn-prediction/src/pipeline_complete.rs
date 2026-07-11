//! Port of `services/churn_prediction.py::ChurnPredictionCompleteTrainingJob`.
//!
//! Same synchronous-inside-the-request model as `pipeline_training.rs`.
//! Unlike `ChurnPredictionTraining`, this job's top-level failure path
//! (`_re_schedule_job`) has **no** internal try/except in the Python, so a
//! second failure while rescheduling really does propagate out of `run()` —
//! reproduced here by having `run()` return `Result`, mapped by `amqp.rs` to
//! a reject-without-requeue (matching the Python consumer's
//! `ack_message(payload, False, False)` in its outer `except`).
//!
//! ARTIFACT FORMAT CHANGE: the Python downloaded `model.tar.gz` and read
//! `trained_churn_prediction_model.pkl` / `features.pkl` /
//! `current_churn.pkl` via `joblib.load` (Python pickle — unreadable outside
//! CPython). This port instead expects the SageMaker training image to write
//! `trained_churn_prediction_model.json` (`Booster.save_model(...,
//! format="json")`, scored locally by `xgb.rs`), `features.json` (the same
//! feature-importance list, JSON-encoded) and `current_churn.json` (a bare
//! JSON number). `model_meta_data.json` is unchanged (it was already JSON in
//! the Python). The SageMaker training container is out of scope for this
//! port — see the top-level report for the full list of what changed.

use octy_shared::errors::OctyError;
use octy_spin::ctx::Ctx;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::billing::BillingUnits;
use crate::bucket::S3;
use crate::encode;
use crate::frame::{Cell, Frame};
use crate::models::churn_percentage_f64;
use crate::repos::mongo;
use crate::sagemaker;
use crate::util;
use crate::xgb::XgbModel;

pub struct ChurnPredictionCompleteTrainingJob {
    account_id: String,
    octy_job_id: String,
    bucket_name: String,
    hyperparam_tuning_job_id: String,
    webhook_url: Option<String>,
    previous_churn_percentage: f64,

    b: BillingUnits,
    s3: S3,

    status: String,
    best_training_job: Option<Value>,
    model_meta: Option<Value>,
    trained_model: Option<XgbModel>,
    features: Vec<Value>,
    current_churn: f64,
    cached_df: Option<Frame>,
    x_pred_cols: Vec<String>,
    predictions_df: Option<Frame>,
    training_compute_units: f64,

    amqp_message_size_limit: usize,
}

impl ChurnPredictionCompleteTrainingJob {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        account_id: String,
        account_type: String,
        account_currency: String,
        octy_job_id: String,
        bucket_name: String,
        hyperparam_tuning_job_id: String,
        previous_churn_percentage: Value,
        webhook_url: Option<String>,
    ) -> Self {
        Self {
            b: BillingUnits::new(&account_id, &account_type, &account_currency, "churn_prediction_completion"),
            s3: S3::new(),
            account_id,
            octy_job_id,
            bucket_name,
            hyperparam_tuning_job_id,
            webhook_url,
            previous_churn_percentage: churn_percentage_f64(&previous_churn_percentage),
            status: String::new(),
            best_training_job: None,
            model_meta: None,
            trained_model: None,
            features: Vec::new(),
            current_churn: 0.0,
            cached_df: None,
            x_pred_cols: Vec::new(),
            predictions_df: None,
            training_compute_units: 0.0,
            amqp_message_size_limit: 104_857_600, // 100 MB AMQP message limit
        }
    }

    pub async fn run(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        let outcome: Result<(), OctyError> = async {
            self.status = sagemaker::get_hparam_tuning_job_status(ctx, &self.hyperparam_tuning_job_id).await?;
            eprintln!("Job ID : {} -- Status : {}", self.hyperparam_tuning_job_id, self.status);

            match self.status.as_str() {
                "InProgress" => self.re_schedule_job(ctx).await,
                "Completed" => {
                    self.b.track_compute_units("hours");
                    self.get_job_artifacts(ctx).await?;
                    self.churn_calculations(ctx).await?;
                    self.get_cached_dataset(ctx).await?;
                    self.predict_churn_scores(ctx).await?;
                    self.assign_churn_scores(ctx).await?;
                    self.destroy_dataset_cache(ctx).await?;
                    self.job_success(ctx).await?;
                    self.b.complete_compute_units(ctx, self.training_compute_units).await;
                    Ok(())
                }
                "Failed" | "Stopping" | "Stopped" => {
                    self.destroy_job(ctx).await;
                    Ok(())
                }
                // No branch matched (unrecognised status): the Python
                // if/elif chain silently falls through with no action.
                _ => Ok(()),
            }
        }
        .await;

        match outcome {
            Ok(()) => {
                eprintln!("Completed Job!");
                Ok(())
            }
            Err(err) => {
                eprintln!("[churn-worker] {err}");
                self.b.complete_compute_units(ctx, self.training_compute_units).await;
                self.re_schedule_job(ctx).await
            }
        }
    }

    async fn send_job_callback(&self, ctx: &Ctx, message: &str, status: &str) -> Result<(), OctyError> {
        let url = format!("{}/v1/internal/jobs/callback", ctx.config.get_str("OCTY_JOB_SERVICE_CLUSTER_IP")?);
        util::http_post_json_with_retry(
            &url,
            &[],
            &json!({
                "account_id": self.account_id,
                "octy_job_id": self.octy_job_id,
                "message": message,
                "status": status,
            }),
        )
        .await?;
        Ok(())
    }

    /// `_send_http_account_webhook_request` — every failure is logged and
    /// swallowed, matching the Python (which never raised from this helper).
    async fn send_webhook(&self, payload: Value) {
        let Some(url) = self.webhook_url.as_deref() else {
            eprintln!("[churn-worker] webhook request skipped: no webhook_url configured");
            return;
        };
        if let Err(err) = util::http_post_json_with_retry(url, &[], &payload).await {
            eprintln!("[churn-worker] webhook request to {url} failed: {err}");
        }
    }

    async fn re_schedule_job(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        eprintln!("Rescheduling job");
        self.send_job_callback(ctx, "Churn-prediction training Job still processing", "failed").await
    }

    #[allow(dead_code)] // see the PYTHON BUG note on `destroy_job`: unreachable in the original.
    async fn job_failed_webhook(&self) {
        self.send_webhook(json!({
            "subject": "Octy training job has failed.",
            "body": {
                "algorithm": "churn-prediction",
                "job_status": "Failed",
                "message": "An issue occurred when attempting to train a churn prediction model. You do have to do anything as our systems will rectify this issue automatically. If this issue repeatedly occurs, please contact the Octy support team: support@octy.ai"
            },
            "date_time": util::py_now_str(),
        }))
        .await;
    }

    async fn job_success(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        let best_job_name = self
            .best_training_job
            .as_ref()
            .and_then(|j| j.get("training_job_name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        mongo::update_hparam_tuning_job_ref(
            ctx,
            &self.account_id,
            &self.hyperparam_tuning_job_id,
            &best_job_name,
            &self.status,
            self.model_meta.as_ref(),
        )
        .await?;
        self.send_webhook(json!({
            "subject": "Octy training job has successfully completed.",
            "body": {
                "algorithm": "churn-prediction",
                "job_status": "Completed",
                "message": "This means a new, up to date churn prediction analysis report is avilable and updated churn predictions have been applied to each profile. Octy support team."
            },
            "date_time": util::py_now_str(),
        }))
        .await;
        self.send_job_callback(ctx, "Churn prediction training Job successfully completed", "success").await
    }

    /// `_destroy_job`. PYTHON BUG (preserved): `update_hparam_tuning_job_ref`
    /// is called without `model_meta`, which the method requires — that call
    /// crashes with `TypeError` in the original service, so the
    /// `octy.job.cmd.delete` publish and `_job_failed_webhook()` that follow
    /// it never run; only the preceding `delete_directory` takes effect.
    async fn destroy_job(&mut self, ctx: &Ctx) {
        eprintln!("Destroying job due to error");
        let result: Result<(), OctyError> = async {
            let models_dir = ctx.config.get_str("CHURN_PRED_MODELS_DIR")?;
            self.s3
                .delete_directory(&self.bucket_name, &format!("{models_dir}/{}", self.hyperparam_tuning_job_id))
                .await?;
            Err(mongo::destroy_job_update_status_call_would_crash())
        }
        .await;

        if let Err(err) = result {
            // Python's except here calls complete_compute_units() with no
            // argument (additional_unit_hours defaults to 0), not the
            // in-flight training_compute_units.
            self.b.complete_compute_units(ctx, 0.0).await;
            eprintln!("[churn-worker] Error occurred when attempting to destroy job. {err}");
        }
        // `job_failed_webhook()` and the `octy.job.cmd.delete` publish that
        // follow `update_hparam_tuning_job_ref` in the Python are
        // unreachable here too — see the PYTHON BUG note above.
    }

    async fn get_job_artifacts(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        eprintln!("Getting job artifacts");
        mongo::get_hparam_tuning_job_ref(ctx, &self.account_id, &self.hyperparam_tuning_job_id, "in_progress")
            .await?;

        let (best_job, compute_units) = sagemaker::get_best_training_job(ctx, &self.hyperparam_tuning_job_id).await?;
        self.training_compute_units = compute_units;
        let job_name = best_job
            .get("training_job_name")
            .and_then(Value::as_str)
            .ok_or_else(|| OctyError::internal("BestTrainingJob missing training_job_name"))?
            .to_string();
        self.best_training_job = Some(best_job);

        let models_dir = ctx.config.get_str("CHURN_PRED_MODELS_DIR")?;
        let model_location = format!("{models_dir}/{job_name}/output/model.tar.gz");
        let files = self.s3.download_resource(&self.bucket_name, &model_location, true).await?;

        for (name, bytes) in files {
            match name.as_str() {
                "model_meta_data.json" => {
                    self.model_meta = Some(
                        serde_json::from_slice(&bytes)
                            .map_err(|e| OctyError::internal(format!("invalid model_meta_data.json: {e}")))?,
                    );
                }
                "trained_churn_prediction_model.json" => {
                    self.trained_model = Some(XgbModel::parse(&bytes).map_err(OctyError::internal)?);
                }
                "features.json" => {
                    let parsed: Value = serde_json::from_slice(&bytes)
                        .map_err(|e| OctyError::internal(format!("invalid features.json: {e}")))?;
                    self.features = parsed.as_array().cloned().unwrap_or_default();
                }
                "current_churn.json" => {
                    let parsed: Value = serde_json::from_slice(&bytes)
                        .map_err(|e| OctyError::internal(format!("invalid current_churn.json: {e}")))?;
                    self.current_churn = parsed.as_f64().unwrap_or(0.0);
                }
                _ => {}
            }
        }
        Ok(())
    }

    async fn churn_calculations(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        let churn_diff = round1(self.previous_churn_percentage - self.current_churn);
        let churn_indicator = if churn_diff > 0.0 {
            "positive"
        } else if churn_diff < 0.0 {
            "negative"
        } else {
            "stalled"
        };

        let feature_vals: Vec<Value> = self
            .features
            .iter()
            .map(|f| f.get("feature_importance").cloned())
            .collect::<Option<Vec<Value>>>()
            .ok_or_else(|| OctyError::internal("'feature_importance'"))?;

        // NOTE: Destroy job if features contains NaN values, this signifies
        // the model is overfitted. `destroy_job` never raises, and the
        // Python does not `return` here — execution always continues on to
        // publish `churn.info.cmd.update` below regardless.
        let has_nan = feature_vals.iter().any(|v| {
            v.as_str() == Some("NaN") || v.as_f64().map(f64::is_nan).unwrap_or(false)
        });
        if has_nan {
            self.destroy_job(ctx).await;
        }

        // NOTE: Destroy job if any feature value is >70% dominant — also
        // does not halt execution.
        let mut counts: HashMap<String, usize> = HashMap::new();
        for v in &feature_vals {
            *counts.entry(v.to_string()).or_insert(0) += 1;
        }
        let total = feature_vals.len().max(1) as f64;
        for count in counts.values() {
            if (*count as f64 * 100.0) / total > 70.0 {
                self.destroy_job(ctx).await;
            }
        }

        ctx.gateway
            .amqp_publish(
                "churn.info.cmd.update",
                &json!({
                    "account_id": self.account_id,
                    "churn_info": {
                        "churn_percentage": self.current_churn,
                        "churn_indicator": churn_indicator,
                        "churn_difference": churn_diff,
                        "features": self.features,
                    }
                }),
            )
            .await?;
        Ok(())
    }

    async fn get_cached_dataset(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        eprintln!("Loading cached dataset...");
        let rows = mongo::get_cached_dataset(ctx, &self.account_id, &self.hyperparam_tuning_job_id).await?;
        self.cached_df = Some(Frame::from_records(&rows).map_err(OctyError::internal)?);
        eprintln!("Cached dataset loaded!");
        Ok(())
    }

    async fn destroy_dataset_cache(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        mongo::delete_cached_dataset(ctx, &self.account_id, &self.hyperparam_tuning_job_id).await?;
        eprintln!("Cached dataset Deleted!");
        Ok(())
    }

    async fn predict_churn_scores(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        eprintln!("Generating churn prediction scores");
        let cached = self.cached_df.as_mut().expect("cached_df loaded");
        let x_pred_cols: Vec<String> = cached
            .cols
            .iter()
            .filter(|c| c.as_str() != "churn" && c.as_str() != "profile_id")
            .cloned()
            .collect();
        self.x_pred_cols = x_pred_cols.clone();

        let model = self
            .trained_model
            .as_ref()
            .ok_or_else(|| OctyError::internal("missing trained_churn_prediction_model.json artifact"))?;

        let mut churn_prob = Vec::with_capacity(cached.len());
        for i in 0..cached.len() {
            let x = cached.row_as_f64(i, &x_pred_cols).map_err(OctyError::internal)?;
            churn_prob.push(Cell::Float(model.predict_proba(&x)));
        }
        cached.add_col("churn_prob", churn_prob).map_err(OctyError::internal)?;

        let mut predictions = cached.select(&["profile_id", "churn_prob"]).map_err(OctyError::internal)?;

        if predictions.nunique("churn_prob").map_err(OctyError::internal)? < 5 {
            // NOTE: does not halt execution — the Python continues to
            // cluster-encode and (later) assign scores even after this.
            self.destroy_job(ctx).await;
        }

        encode::numerical_cluster_encoding(&mut predictions, "churn_prob", true).map_err(OctyError::internal)?;
        self.predictions_df = Some(predictions);
        Ok(())
    }

    /// `_assign_churn_scores`. PYTHON BUG (preserved), two of them:
    /// 1. When `predict_churn_scores` skipped clustering (fewer than 30
    ///    predictions), `predictions_df` still has a `churn_prob` column, not
    ///    `churn_prob_cluster` — the Python's `pd['churn_prob_cluster']`
    ///    raises `KeyError`, propagating out to `run()`'s reschedule path.
    ///    Reproduced by requiring the `churn_prob_cluster` column here.
    /// 2. `amqp_batch_profiles` only gets the final (possibly partial)
    ///    `profile_updates` batch appended when it is still empty — once the
    ///    100 MB size guard has split off *any* earlier batch, the trailing
    ///    remainder is silently dropped and never published. Reproduced
    ///    as-is below.
    async fn assign_churn_scores(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        eprintln!("Assigning churn prediction scores to profiles");
        let predictions = self.predictions_df.as_ref().expect("predictions computed");
        let cluster_idx = predictions
            .col_index("churn_prob_cluster")
            .ok_or_else(|| OctyError::internal("'churn_prob_cluster'"))?;
        let profile_idx = predictions
            .col_index("profile_id")
            .ok_or_else(|| OctyError::internal("'profile_id'"))?;

        let mut amqp_batch_profiles: Vec<Vec<Value>> = Vec::new();
        let mut profile_updates: Vec<Value> = Vec::new();
        let mut running_size: usize = 64; // base list overhead, approximated

        for row in &predictions.rows {
            let entry = json!({
                "profile_id": row[profile_idx].to_json(),
                "churn_probability": row[cluster_idx].to_json(),
            });
            running_size += entry.to_string().len() + 8;
            profile_updates.push(entry);

            if running_size > self.amqp_message_size_limit {
                amqp_batch_profiles.push(std::mem::take(&mut profile_updates));
                running_size = 64;
            }
        }
        if amqp_batch_profiles.is_empty() {
            amqp_batch_profiles.push(profile_updates);
        }

        for batch in amqp_batch_profiles {
            ctx.gateway
                .amqp_publish(
                    "profiles.cmd.update",
                    &json!({ "account_id": self.account_id, "profiles": batch }),
                )
                .await?;
        }
        Ok(())
    }
}

/// Python `round(x, 1)` — round-half-to-even at one decimal place.
fn round1(x: f64) -> f64 {
    let scaled = x * 10.0;
    let floor = scaled.floor();
    let diff = scaled - floor;
    let rounded = if (diff - 0.5).abs() < 1e-9 {
        if (floor as i64) % 2 == 0 {
            floor
        } else {
            floor + 1.0
        }
    } else {
        scaled.round()
    };
    rounded / 10.0
}
