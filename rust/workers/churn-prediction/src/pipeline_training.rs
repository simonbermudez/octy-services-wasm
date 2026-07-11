//! Port of `services/churn_prediction.py::ChurnPredictionTraining`.
//!
//! Runs synchronously inside the `/internal/amqp/consume` request (see
//! `amqp.rs`); every internal failure is caught and routed through
//! `_dispose_job` exactly like the Python `try/except` in `run()` — the AMQP
//! delivery is always acked (2xx), never rejected, matching the original
//! (the Python's broad `except Exception` meant `run()` itself never raised
//! back to the consumer).

use octy_shared::errors::OctyError;
use octy_spin::ctx::Ctx;
use serde_json::{json, Value};

use crate::billing::BillingUnits;
use crate::bucket::S3;
use crate::encode;
use crate::frame::{Cell, Frame, How};
use crate::repos::{external, mongo};
use crate::sagemaker;
use crate::util::{self, generate_uid};

fn algo_profile_features(v: &Value) -> Vec<String> {
    v.get("profile_features")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

fn algo_event_type(v: &Value) -> String {
    v.get("event_type").and_then(Value::as_str).unwrap_or_default().to_string()
}

pub struct ChurnPredictionTraining {
    account_id: String,
    octy_job_id: String,
    bucket_name: String,
    algorithm_configurations: Value,
    b: BillingUnits,
    s3: S3,

    hyperparam_tuning_job_id: String,
    data_timeframe: i64,

    /// `self.features` — `[{'item_feature_list': [...]}, {'profile_feature_list': [...]}]`.
    item_feature_list: Vec<String>,
    profile_feature_list: Vec<String>,

    /// `stop_list_tuple` — starts at `('has_charged',)`, grows with segment tags.
    stop_list: Vec<String>,

    items_df: Option<Frame>,
    profiles_df: Option<Frame>,
    charged_events_df: Option<Frame>,
    complaints_events_df: Option<Frame>,
    training_df: Option<Frame>,

    training_resources: Vec<Value>,
    total_bytes: i64,
    key: Option<String>,
    mpu_upload_id: Option<String>,
}

impl ChurnPredictionTraining {
    pub fn new(
        account_id: String,
        account_type: String,
        account_currency: String,
        octy_job_id: String,
        bucket_name: String,
        algorithm_configurations: Value,
        data_timeframe: i64,
        item_feature_cols: Vec<String>,
    ) -> Self {
        Self {
            b: BillingUnits::new(&account_id, &account_type, &account_currency, "churn_prediction_training"),
            s3: S3::new(),
            account_id,
            octy_job_id,
            bucket_name,
            algorithm_configurations,
            hyperparam_tuning_job_id: generate_uid("hp-t-job"),
            data_timeframe,
            item_feature_list: item_feature_cols,
            profile_feature_list: vec!["rfm_score".to_string(), "has_charged".to_string()],
            stop_list: vec!["has_charged".to_string()],
            items_df: None,
            profiles_df: None,
            charged_events_df: None,
            complaints_events_df: None,
            training_df: None,
            training_resources: Vec::new(),
            total_bytes: 0,
            key: None,
            mpu_upload_id: None,
        }
    }

    pub async fn run(&mut self, ctx: &Ctx) {
        self.b.track_compute_units("hours");

        let result: Result<(), OctyError> = async {
            self.build_training_dataset(ctx).await?;
            self.upload_resources(ctx).await?;
            self.start_cloud_hparam_tuning_job(ctx).await?;
            self.complete_job(ctx).await?;
            Ok(())
        }
        .await;

        match result {
            Ok(()) => {
                self.b.complete_compute_units(ctx, 0.0).await;
            }
            Err(err) => {
                eprintln!("[churn-worker] churn prediction training failed: {err}");
                self.b.complete_compute_units(ctx, 0.0).await;
                self.dispose_job(ctx, &err.to_string()).await;
            }
        }
    }

    // ---- Data aggregation ----

    async fn get_items_data(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        let items = external::get_items(ctx, &self.account_id, "false").await?;
        let mut df = Frame::from_records(&items).map_err(OctyError::internal)?;
        df.drop_cols(&["item_category", "item_name", "item_description", "status", "created_at", "updated_at"])
            .map_err(OctyError::internal)?;
        if (df.len() as i64) < ctx.config.get_i64("MIN_NUM_ITEMS")? {
            return Err(OctyError::internal("Not enough items found to conduct model training."));
        }
        self.items_df = Some(df);
        Ok(())
    }

    async fn get_profiles_data(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        let mut records = external::get_profiles(ctx, &self.account_id, "active", "false").await?;
        records.extend(external::get_profiles(ctx, &self.account_id, "churned", "false").await?);
        let mut df = Frame::from_records(&records).map_err(OctyError::internal)?;

        let status_idx = df
            .col_index("status")
            .ok_or_else(|| OctyError::internal("profiles missing 'status' field"))?;
        let churn: Vec<Cell> = df
            .rows
            .iter()
            .map(|row| Cell::Bool(matches!(&row[status_idx], Cell::Str(s) if s == "churned")))
            .collect();
        df.add_col("churn", churn).map_err(OctyError::internal)?;

        df.drop_cols(&[
            "customer_id",
            "rfm_score",
            "rfm_segment_desc",
            "churn_probability",
            "status",
            "created_at",
            "updated_at",
        ])
        .map_err(OctyError::internal)?;

        if (df.len() as i64) < ctx.config.get_i64("MIN_NUM_PROFILES")? {
            return Err(OctyError::internal("Not enough profiles found to conduct model training."));
        }
        self.profiles_df = Some(df);
        Ok(())
    }

    fn apply_profile_dict_features(&mut self) -> Result<(), OctyError> {
        let algo_features = algo_profile_features(&self.algorithm_configurations);
        let df = self.profiles_df.as_mut().expect("profiles_df built");
        for col in ["platform_info", "profile_data"] {
            let keys = df.dict_keys_union_sorted(col).map_err(OctyError::internal)?;
            for key in keys {
                if algo_features.iter().any(|f| f == &key) {
                    let values = df.dict_value_col(col, &key).map_err(OctyError::internal)?;
                    df.add_col(&key, values).map_err(OctyError::internal)?;
                }
            }
        }
        Ok(())
    }

    async fn dynamic_null_drop(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        let allowed = ctx.config.get_i64("ALLOWED_COL_NULL_COUNT")? as f64;
        let algo_features = algo_profile_features(&self.algorithm_configurations);
        let mut required_cols = self.profile_feature_list.clone();
        required_cols.extend(["segment_tags".to_string(), "profile_id".to_string(), "churn".to_string()]);

        let df = self.profiles_df.as_mut().expect("profiles_df built");
        let current_cols = df.cols.clone();
        let mut cols_to_drop: Vec<String> = Vec::new();

        for col in &current_cols {
            let null_count = df.null_count(col).map_err(OctyError::internal)?;
            let total = df.len().max(1);
            let null_percent = (null_count as f64 / total as f64) * 100.0;
            if null_percent > allowed {
                cols_to_drop.push(col.clone());
            }
        }
        for col in &current_cols {
            if !algo_features.contains(col) && !required_cols.contains(col) {
                cols_to_drop.push(col.clone());
            }
        }

        eprintln!(
            "Dropping 'profiles_df' columns: {cols_to_drop:?} due to null values exceeding {allowed}% \
             of the each columns cells values. OR columns not required for training data set."
        );
        let refs: Vec<&str> = cols_to_drop.iter().map(String::as_str).collect();
        df.drop_cols(&refs).map_err(OctyError::internal)?;
        df.dropna_any();
        Ok(())
    }

    fn apply_segment_tags(&mut self) -> Result<(), OctyError> {
        let df = self.profiles_df.as_mut().expect("profiles_df built");
        let idx = df
            .col_index("segment_tags")
            .ok_or_else(|| OctyError::internal("profiles missing 'segment_tags' field"))?;

        let mut tags: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for row in &df.rows {
            if let Cell::Json(Value::Array(arr)) = &row[idx] {
                for item in arr {
                    if let Some(t) = item.get("segment_tag").and_then(Value::as_str) {
                        tags.insert(t.to_string());
                    }
                }
            }
        }

        for tag in tags {
            let seg_key = format!("{tag}__SEGMENT");
            self.stop_list.push(seg_key.clone());
            let col: Vec<Cell> = df
                .rows
                .iter()
                .map(|row| {
                    let has = if let Cell::Json(Value::Array(arr)) = &row[idx] {
                        arr.iter()
                            .any(|item| item.get("segment_tag").and_then(Value::as_str) == Some(tag.as_str()))
                    } else {
                        false
                    };
                    Cell::Int(has as i64)
                })
                .collect();
            df.add_col(&seg_key, col).map_err(OctyError::internal)?;
        }
        df.remove_col("segment_tags").map_err(OctyError::internal)?;
        Ok(())
    }

    async fn get_events_data(&mut self, ctx: &Ctx, profile_ids: &[String]) -> Result<(), OctyError> {
        let charged = external::get_events(ctx, &self.account_id, profile_ids, self.data_timeframe, "charged").await?;
        let complaints =
            external::get_events(ctx, &self.account_id, profile_ids, self.data_timeframe, "complaint").await?;

        if (charged.len() as i64) < ctx.config.get_i64("MIN_NUM_CHARGED_EVENTS")? {
            return Err(OctyError::internal(
                "Not enough charged event instances found to conduct model training.",
            ));
        }
        if (complaints.len() as i64) < ctx.config.get_i64("MIN_NUM_COMPLAINTS")? {
            return Err(OctyError::internal(
                "Not enough complaint event instances found to conduct model training.",
            ));
        }

        let mut charged_df = Frame::from_records(&charged).map_err(OctyError::internal)?;
        let mut complaints_df = Frame::from_records(&complaints).map_err(OctyError::internal)?;
        complaints_df
            .drop_cols(&["account_id", "event_type_id", "created_at", "event_id"])
            .map_err(OctyError::internal)?;
        charged_df
            .drop_cols(&["account_id", "event_type_id", "created_at", "event_id"])
            .map_err(OctyError::internal)?;

        self.charged_events_df = Some(charged_df);
        self.complaints_events_df = Some(complaints_df);
        Ok(())
    }

    fn expand_event_properties(df: &mut Frame) -> Result<(), OctyError> {
        let keys = df.dict_keys_union_sorted("event_properties").map_err(OctyError::internal)?;
        for key in keys {
            let values = df.dict_value_col("event_properties", &key).map_err(OctyError::internal)?;
            df.add_col(&key, values).map_err(OctyError::internal)?;
        }
        df.remove_col("event_properties").map_err(OctyError::internal)?;
        Ok(())
    }

    async fn apply_total_purchase_value(&mut self) -> Result<(), OctyError> {
        let charged = self.charged_events_df.as_ref().expect("charged events built");
        let items = self.items_df.as_ref().expect("items built");
        let purchased = charged.merge(items, "item_id", How::Inner).map_err(OctyError::internal)?;
        let mut total = purchased.groupby_sum("profile_id", "item_price").map_err(OctyError::internal)?;
        total.rename_col("item_price", "total_purchase_value");

        let training = self.training_df.as_ref().expect("training_df built");
        let mut merged = training.merge(&total, "profile_id", How::Outer).map_err(OctyError::internal)?;
        merged.fillna("total_purchase_value", Cell::Float(0.0)).map_err(OctyError::internal)?;
        self.training_df = Some(merged);
        Ok(())
    }

    async fn get_profile_most_frequent(
        &mut self,
        events_df: &Frame,
        drop_columns: &[&str],
        keep_col: &str,
    ) -> Result<(), OctyError> {
        let mut most_frequent = events_df.groupby_most_frequent("profile_id").map_err(OctyError::internal)?;
        most_frequent.drop_cols(drop_columns).map_err(OctyError::internal)?;

        let training = self.training_df.as_ref().expect("training_df built");
        let mut merged = training.merge(&most_frequent, "profile_id", How::Outer).map_err(OctyError::internal)?;
        merged
            .fillna(keep_col, Cell::Str("not_specified".to_string()))
            .map_err(OctyError::internal)?;
        self.training_df = Some(merged);
        Ok(())
    }

    async fn get_profile_event_count(
        &mut self,
        events_df: &Frame,
        drop_columns: &[&str],
        new_columns: &[&str],
    ) -> Result<(), OctyError> {
        let mut counted = events_df.groupby_count("profile_id").map_err(OctyError::internal)?;
        counted.drop_cols(drop_columns).map_err(OctyError::internal)?;
        let names: Vec<String> = new_columns.iter().map(|s| s.to_string()).collect();
        counted.rename_all(&names).map_err(OctyError::internal)?;

        let training = self.training_df.as_ref().expect("training_df built");
        let mut merged = training.merge(&counted, "profile_id", How::Outer).map_err(OctyError::internal)?;
        merged.fillna(new_columns[1], Cell::Int(0)).map_err(OctyError::internal)?;
        merged.cast_col_int(new_columns[1]).map_err(OctyError::internal)?;
        self.training_df = Some(merged);
        Ok(())
    }

    /// `_identify_drop_invalid_numerical_columns`. PYTHON BUG (preserved):
    /// the "drop" for columns with <2 unique values calls
    /// `self.training_df.drop([n_col], axis=1)` without reassigning /
    /// `inplace=True`, so the column is *not* actually removed — it is only
    /// excluded from both the cluster and bin encoding lists and survives
    /// untouched into the final training set.
    fn identify_drop_invalid_numerical_columns(&mut self) -> Result<(Vec<String>, Vec<String>), OctyError> {
        let df = self.training_df.as_ref().expect("training_df built");
        let mut num_cluster_cols = Vec::new();
        let mut num_bin_cols = Vec::new();
        let num_cols: Vec<String> = df
            .numeric_cols()
            .into_iter()
            .filter(|c| !self.stop_list.contains(c))
            .collect();

        for n_col in num_cols {
            let unique = df.unique_len_with_null(&n_col).map_err(OctyError::internal)?;
            if unique < 2 {
                eprintln!("Dropping numerical column: {n_col} due to insufficient number of unique values");
                continue;
            }
            if unique < 10 {
                num_bin_cols.push(n_col);
            } else {
                num_cluster_cols.push(n_col);
            }
        }
        Ok((num_cluster_cols, num_bin_cols))
    }

    async fn build_training_dataset(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        eprintln!("Building training dataset ...");

        eprintln!("Building items dataframe ...");
        self.get_items_data(ctx).await?;
        eprintln!("Built items dataframe");

        eprintln!("Building profiles dataframe ...");
        self.get_profiles_data(ctx).await?;
        self.apply_profile_dict_features()?;
        self.dynamic_null_drop(ctx).await?;
        self.apply_segment_tags()?;

        let profile_ids: Vec<String> = self
            .profiles_df
            .as_ref()
            .unwrap()
            .col_cells("profile_id")
            .map_err(OctyError::internal)?
            .iter()
            .map(|c| c.py_str())
            .collect();
        eprintln!("Built profiles dataframe");

        eprintln!("Building training dataframe ...");
        self.training_df = Some(self.profiles_df.as_ref().unwrap().clone());

        self.get_events_data(ctx, &profile_ids).await?;
        {
            let charged = self.charged_events_df.as_mut().unwrap();
            Self::expand_event_properties(charged)?;
        }
        {
            let complaints = self.complaints_events_df.as_mut().unwrap();
            Self::expand_event_properties(complaints)?;
        }

        self.apply_total_purchase_value().await?;

        let charged = self.charged_events_df.as_ref().unwrap().clone();
        let complaints = self.complaints_events_df.as_ref().unwrap().clone();
        self.get_profile_most_frequent(&charged, &["item_id", "event_type"], "payment_method").await?;
        self.get_profile_most_frequent(&complaints, &["event_type"], "channel").await?;
        self.get_profile_event_count(&charged, &["item_id", "payment_method"], &["profile_id", "number_charges"])
            .await?;
        self.get_profile_event_count(&complaints, &["channel"], &["profile_id", "number_complaints"]).await?;

        let (num_cluster_cols, num_bin_cols) = self.identify_drop_invalid_numerical_columns()?;
        for col in num_cluster_cols {
            let df = self.training_df.as_mut().unwrap();
            encode::numerical_cluster_encoding(df, &col, true).map_err(OctyError::internal)?;
        }
        for col in num_bin_cols {
            let df = self.training_df.as_mut().unwrap();
            encode::numerical_bin_encoding(df, &col, 3).map_err(OctyError::internal)?;
        }

        {
            let df = self.training_df.as_mut().unwrap();
            encode::categorical_encoding(df, &["profile_id"]).map_err(OctyError::internal)?;
        }

        let training_len = self.training_df.as_ref().unwrap().len();
        if (training_len as i64) < ctx.config.get_i64("MIN_NUM_ROWS_COLLECTIVE")? {
            return Err(OctyError::internal("Not enough valid data to conduct model training."));
        }

        {
            let df = self.training_df.as_mut().unwrap();
            encode::format_column_names(df);
        }

        let feature_cols: Vec<String> = self
            .training_df
            .as_ref()
            .unwrap()
            .cols
            .iter()
            .filter(|c| c.as_str() != "churn" && c.as_str() != "profile_id")
            .cloned()
            .collect();
        self.profile_feature_list = feature_cols;

        Ok(())
    }

    // ---- Dataset file upload ----

    async fn upload_resources(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        eprintln!("Uploading training job resources");
        let csv_data = self.training_df.as_ref().unwrap().to_csv();
        let resource_type = "training";

        self.b.track_data_units(csv_data.len(), resource_type.len());
        let file_size = util::py_str_sizeof(csv_data.len());

        let min_file_size = ctx.config.get_i64("MIN_FILE_SIZE")?;
        let max_file_size = ctx.config.get_i64("MAX_FILE_SIZE")?;
        let min_chunk_size = ctx.config.get_i64("MIN_CHUNK_SIZE")?;
        let max_num_parts = ctx.config.get_i64("MAX_NUM_PARTS")?;
        let churn_data_dir = ctx.config.get_str("CHURN_DATA_DIR")?.to_string();

        if file_size < min_file_size {
            let key = self
                .s3
                .single_upload(
                    &churn_data_dir,
                    csv_data.as_bytes(),
                    resource_type,
                    &self.hyperparam_tuning_job_id,
                    &self.bucket_name,
                )
                .await?;
            self.key = Some(key);
        } else if file_size > min_file_size && file_size < max_file_size {
            // Gateway capability gap: no multipart-upload endpoints exist
            // (`/v1/s3/{create,upload-part,complete}-multipart-upload`).
            // The chunk-count validation is preserved; the object itself is
            // uploaded whole via `put-object`.
            let chunk_count = (file_size as f64 / min_chunk_size as f64).ceil() as i64;
            eprintln!("Number of upload parts: {chunk_count}");
            if chunk_count > max_num_parts {
                return Err(OctyError::internal("Maximum number of chunk parts exceeded"));
            }
            if chunk_count < 2 {
                return Err(OctyError::internal("Could not chunk file! Less thank 2 chunks."));
            }
            let key = self
                .s3
                .upload_whole(
                    &churn_data_dir,
                    csv_data.as_bytes(),
                    resource_type,
                    &self.hyperparam_tuning_job_id,
                    &self.bucket_name,
                )
                .await?;
            self.key = Some(key);
        } else {
            return Err(OctyError::internal(format!(
                "Invalid file size. File size exceeds maximum. Account ID: {} File type : {resource_type}",
                self.account_id
            )));
        }

        self.training_resources.push(json!({
            "channel_name": resource_type,
            "training_resource_location": self.key,
        }));
        self.total_bytes += file_size;

        self.b.complete_data_units(ctx, "MB").await;
        eprintln!("Uploaded training job resources!");
        Ok(())
    }

    // ---- Training job ----

    async fn start_cloud_hparam_tuning_job(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        eprintln!("Starting cloud hyper parameter tuning job...");
        let parent_job = mongo::get_parent_hparam_tuning_job_ref(ctx, &self.account_id).await;
        let parent_job_id = parent_job
            .as_ref()
            .and_then(|j| j.get("hyperparam_tuning_job_id"))
            .and_then(Value::as_str)
            .map(String::from);

        let volume_size = util::required_gb(self.total_bytes as f64)?;
        sagemaker::create_hyper_parameter_tuning_job(
            ctx,
            &self.account_id,
            &self.hyperparam_tuning_job_id,
            parent_job_id.as_deref(),
            volume_size,
            &self.training_resources,
            &self.bucket_name,
        )
        .await?;
        eprintln!("Cloud hyper parameter tuning job started!");

        let meta_data = json!({
            "event_type": algo_event_type(&self.algorithm_configurations),
            "features": [
                { "item_feature_list": self.item_feature_list },
                { "profile_feature_list": self.profile_feature_list },
            ]
        });
        mongo::create_hparam_tuning_job_ref(ctx, &self.hyperparam_tuning_job_id, &self.account_id, &meta_data).await?;

        let dataset_json = self.training_df.as_ref().unwrap().to_json_index();
        mongo::cache_dataset(ctx, &self.account_id, &self.hyperparam_tuning_job_id, &dataset_json).await?;
        Ok(())
    }

    async fn complete_job(&mut self, ctx: &Ctx) -> Result<(), OctyError> {
        eprintln!("Training job complete");
        let job_url = format!("{}/v1/internal/jobs/callback", ctx.config.get_str("OCTY_JOB_SERVICE_CLUSTER_IP")?);
        util::http_post_json_with_retry(
            &job_url,
            &[],
            &json!({
                "account_id": self.account_id,
                "octy_job_id": self.octy_job_id,
                "message": "Churn prediction training Job suceeded",
                "status": "success",
            }),
        )
        .await?;

        ctx.gateway
            .amqp_publish(
                "octy.job.cmd.create",
                &json!({
                    "account_id": self.account_id,
                    "job_meta": {
                        "job_type": "churn",
                        "amqp_routing_key": "churn.training.complete.cmd.run",
                        "required_permissions": ["churn"],
                        "required_configurations": {
                            "account_attributes": [
                                "account_configurations.webhook_url",
                                "account_configurations.account_type",
                                "account_configurations.account_currency",
                                "bucket",
                                "churn_info.churn_percentage"
                            ],
                            "algorithm_configuration_idxs": [1]
                        },
                        "desired_runs": 1,
                        "time_interval": 60,
                        "fail_threshold": 3
                    },
                    "job_data": {
                        "hyperparam_tuning_job_id": self.hyperparam_tuning_job_id,
                    }
                }),
            )
            .await?;
        Ok(())
    }

    // ---- Failure path ----

    async fn dispose_job(&mut self, ctx: &Ctx, ex: &str) {
        let result: Result<(), OctyError> = async {
            mongo::delete_hparam_tuning_job_ref(ctx, &self.account_id, &self.hyperparam_tuning_job_id).await?;
            self.s3
                .abort_multipart_upload(self.key.as_deref(), self.mpu_upload_id.as_deref(), &self.bucket_name)
                .await;
            let job_url = format!("{}/v1/internal/jobs/callback", ctx.config.get_str("OCTY_JOB_SERVICE_CLUSTER_IP")?);
            util::http_post_json_with_retry(
                &job_url,
                &[],
                &json!({
                    "account_id": self.account_id,
                    "octy_job_id": self.octy_job_id,
                    "message": format!("Churn prediction training Job failed. EX :: {ex}"),
                    "status": "failed",
                }),
            )
            .await?;
            Ok(())
        }
        .await;

        if let Err(err) = result {
            self.b.complete_compute_units(ctx, 0.0).await;
            eprintln!("[churn-worker] Error occurred when attempting to dispose of job. {err}");
        }
    }
}
