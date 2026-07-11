//! Port of `services/recommendation.py` — `RecommenderTraining`.
//!
//! Builds the profiles/events/items training datasets, converts them to CSV,
//! uploads them to S3 (through the gateway), starts a SageMaker
//! hyper-parameter tuning job, and writes the job reference to MongoDB.
//!
//! Chunked-upload semantics: the Python computed `chunk_count` and rejected
//! files needing >`MAX_NUM_PARTS` parts, then only actually issued a
//! multipart upload when `chunk_count >= 2`. That validation is preserved
//! bug-for-bug; the assembled bytes are then PUT once through the gateway
//! (see `repos::bucket` for why — no MPU bridge on the gateway).

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};

use octy_spin::ctx::Ctx;

use crate::billing::BillingUnits;
use crate::frame::Frame;
use crate::http::post_json_with_retry;
use crate::models::AlgorithmConfigurations;
use crate::repos::bucket;
use crate::repos::recommendation as repo;
use crate::repos::recommendation::TrainingResource;
use crate::utils::{py_sizeof_csv_object, required_gb};

pub struct RecommenderTraining<'a> {
    ctx: &'a Ctx,
    account_id: String,
    octy_job_id: String,
    bucket: String,
    algorithm_configurations: Map<String, Value>,
    billing: BillingUnits,
    hyperparam_tuning_job_id: String,
    data_timeframe: i64,
    profiles_ids: Vec<String>,
    features: Value,
    profiles_df: Option<Frame>,
    events_df: Option<Frame>,
    items_df: Option<Frame>,
    csv_objects: Vec<(String, String)>, // (type, data)
    training_resources: Vec<TrainingResource>,
    total_bytes: i64,
    key: Option<String>,
}

impl<'a> RecommenderTraining<'a> {
    pub fn new(
        ctx: &'a Ctx,
        account_id: String,
        account_type: String,
        account_currency: String,
        octy_job_id: String,
        bucket: String,
        algorithm_configurations: Map<String, Value>,
    ) -> Result<Self> {
        let data_timeframe = ctx
            .config
            .get_i64("DATA_SET_TIMEFRAME")
            .map_err(|e| anyhow!("{e}"))?;
        let item_feature_cols = ctx
            .config
            .get_array("ITEM_FEATURE_COLS")
            .map_err(|e| anyhow!("{e}"))?
            .clone();
        let hyperparam_tuning_job_id = crate::utils::generate_uid("hp-t-job");

        Ok(Self {
            ctx,
            account_id: account_id.clone(),
            octy_job_id,
            bucket,
            algorithm_configurations,
            billing: BillingUnits::new(&account_id, &account_type, &account_currency, "recommender_training"),
            hyperparam_tuning_job_id,
            data_timeframe,
            profiles_ids: Vec::new(),
            features: json!([
                { "item_feature_list": item_feature_cols },
                { "profile_feature_list": ["rfm_score", "has_charged"] },
            ]),
            profiles_df: None,
            events_df: None,
            items_df: None,
            csv_objects: Vec::new(),
            training_resources: Vec::new(),
            total_bytes: 0,
            key: None,
        })
    }

    fn algo(&self) -> AlgorithmConfigurations<'_> {
        AlgorithmConfigurations(&self.algorithm_configurations)
    }

    async fn build_profiles_dataset(&mut self) -> Result<()> {
        let features_list = self.algo().profile_features()?;
        let segments = repo::get_segments(self.ctx, &self.account_id).await?;
        let segment_names: Vec<String> = segments
            .iter()
            .filter_map(|s| s.get("segment_name").and_then(Value::as_str))
            .map(str::to_string)
            .collect();

        let profiles = repo::get_profiles(self.ctx, &self.account_id, false).await?;
        let min_profiles = self
            .ctx
            .config
            .get_i64("MIN_NUM_PROFILES")
            .map_err(|e| anyhow!("{e}"))?;
        if (profiles.len() as i64) < min_profiles {
            bail!("Not enough profiles found to conduct model training.");
        }

        let mut profiles_list: Vec<Map<String, Value>> = Vec::with_capacity(profiles.len());
        for p in &profiles {
            let rfm_raw = p.get("rfm_score").cloned().unwrap_or(Value::Null);
            // Unset RFM score defaults to 111 — the lowest possible RFM score.
            let rfm_score = match &rfm_raw {
                Value::Null => json!(111),
                Value::String(s) if s.is_empty() => json!(111),
                other => other.clone(),
            };

            let mut profile_dict = Map::new();
            profile_dict.insert(
                "profile_id".to_string(),
                p.get("profile_id").cloned().unwrap_or(Value::Null),
            );
            profile_dict.insert(
                "has_charged".to_string(),
                p.get("has_charged").cloned().unwrap_or(Value::Null),
            );
            profile_dict.insert("rfm_score".to_string(), rfm_score);
            let segment_tags = p
                .get("segment_tags")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            // merge platform_info + profile_data, keep keys in features_list
            let platform_info = p
                .get("platform_info")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let profile_data = p
                .get("profile_data")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let mut merged = platform_info;
            merged.extend(profile_data);
            for (k, v) in merged {
                if features_list.iter().any(|f| f == &k) {
                    profile_dict.insert(k, v);
                }
            }

            // one-hot encode segment tags
            for seg in &segment_names {
                let key = format!("{seg}__SEGMENT");
                let mut hit = 0;
                for tag in &segment_tags {
                    if tag.get("segment_tag").and_then(Value::as_str) == Some(seg.as_str()) {
                        hit = 1;
                    }
                }
                profile_dict.insert(key, json!(hit));
            }

            profiles_list.push(profile_dict);
        }

        let mut profiles_df = Frame::from_records(&profiles_list);
        profiles_df.dropna();
        profiles_df.drop_columns(&["segment_tags"])?;
        // profile_LFM_IDX must be a sequential index matching each row's position —
        // SageMaker/LightFM assigns user embeddings by that same row order.
        profiles_df.insert_range_index("profile_LFM_IDX");

        self.profiles_ids = profiles_df
            .column_values("profile_id")?
            .into_iter()
            .map(|v| repo::value_as_string(&v))
            .collect();
        self.profiles_df = Some(profiles_df);
        Ok(())
    }

    async fn build_events_dataset(&mut self) -> Result<()> {
        let event_type = self.algo().event_type()?;
        let events = repo::get_events(
            self.ctx,
            &self.account_id,
            &self.profiles_ids,
            self.data_timeframe,
            &event_type,
        )
        .await?;
        let min_events = self
            .ctx
            .config
            .get_i64("MIN_NUM_EVENTS_COLLECTIVE")
            .map_err(|e| anyhow!("{e}"))?;
        if (events.len() as i64) < min_events {
            bail!("Not enough events found to conduct model training.");
        }

        let rec_item_identifier = self.algo().rec_item_identifier()?;
        let mut events_list: Vec<Map<String, Value>> = Vec::new();
        for event in &events {
            let props = event.get("event_properties").cloned().unwrap_or(Value::Null);
            let skip = matches!(&props, Value::Null)
                || matches!(&props, Value::String(s) if s.is_empty() || s == "\"\"");
            if skip {
                continue;
            }
            let Some(props_obj) = props.as_object() else { continue };
            let mut variable_value = Value::Null;
            for (k, v) in props_obj {
                if k == &rec_item_identifier {
                    variable_value = v.clone();
                }
            }
            let mut row = Map::new();
            row.insert(
                "profile_id".to_string(),
                event.get("profile_id").cloned().unwrap_or(Value::Null),
            );
            row.insert("variable_value".to_string(), variable_value);
            events_list.push(row);
        }

        let cols = self
            .ctx
            .config
            .get_array("EVENTS_DATAFRAME_COLS")
            .map_err(|e| anyhow!("{e}"))?;
        let columns: Vec<String> = cols
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        self.events_df = Some(Frame::from_records_with_columns(&events_list, &columns));
        Ok(())
    }

    async fn build_items_dataset(&mut self) -> Result<()> {
        let items = repo::get_items(self.ctx, &self.account_id, false, "all").await?;
        let min_items = self
            .ctx
            .config
            .get_i64("MIN_NUM_ITEMS")
            .map_err(|e| anyhow!("{e}"))?;
        if (items.len() as i64) < min_items {
            bail!("Not enough items found to conduct model training.");
        }
        let records: Vec<Map<String, Value>> = items
            .iter()
            .filter_map(|item| item.as_object().cloned())
            .collect();
        let mut items_df = Frame::from_records(&records);
        items_df.drop_columns(&["status", "created_at", "updated_at"])?;

        let renamed = self
            .ctx
            .config
            .get_array("ITEMS_DATAFRAME_COLS")
            .map_err(|e| anyhow!("{e}"))?;
        let renamed: Vec<String> = renamed
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        items_df.set_columns(&renamed)?;
        // Same rationale as profile_LFM_IDX: row position becomes the item embedding index.
        items_df.insert_range_index("item_LFM_IDX");
        self.items_df = Some(items_df);
        Ok(())
    }

    async fn feature_engineering(&mut self) -> Result<()> {
        let profiles_df = self.profiles_df.as_ref().ok_or_else(|| anyhow!("profiles_df not built"))?;
        let events_df = self.events_df.as_ref().ok_or_else(|| anyhow!("events_df not built"))?;
        let items_df = self.items_df.as_ref().ok_or_else(|| anyhow!("items_df not built"))?;

        let system_cols = ["profile_LFM_IDX", "profile_id", "rfm_score", "has_charged"];
        if let Some(profile_feature_list) = self.features[1]["profile_feature_list"].as_array_mut() {
            for c in &profiles_df.columns {
                if !system_cols.contains(&c.as_str()) {
                    profile_feature_list.push(json!(c));
                }
            }
        }

        let merged = events_df.merge_left(profiles_df, "profile_id", "profile_id")?;
        let merged = merged.merge_left(items_df, "variable_value", "item_id")?;

        let mut merged = merged;
        merged.dropna();
        self.events_df = Some(merged);
        Ok(())
    }

    async fn create_csv_objects(&mut self) -> Result<()> {
        let profiles_csv = self
            .profiles_df
            .as_ref()
            .ok_or_else(|| anyhow!("profiles_df not built"))?
            .to_csv()?;
        let events_csv = self
            .events_df
            .as_ref()
            .ok_or_else(|| anyhow!("events_df not built"))?
            .to_csv()?;
        let items_csv = self
            .items_df
            .as_ref()
            .ok_or_else(|| anyhow!("items_df not built"))?
            .to_csv()?;
        self.csv_objects.push(("profiles".to_string(), profiles_csv));
        self.csv_objects.push(("events".to_string(), events_csv));
        self.csv_objects.push(("items".to_string(), items_csv));
        let meta = json!({ "features": self.features });
        self.csv_objects.push(("meta_data".to_string(), meta.to_string()));
        Ok(())
    }

    /// `_chunk_file_` — returns `Some(chunk_count)` when >=2 chunks are
    /// required, `None` when the file fits in a single chunk.
    fn plan_chunks(&self, file_size: i64) -> Result<Option<i64>> {
        let min_chunk = self
            .ctx
            .config
            .get_i64("MIN_CHUNK_SIZE")
            .map_err(|e| anyhow!("{e}"))?;
        let max_parts = self
            .ctx
            .config
            .get_i64("MAX_NUM_PARTS")
            .map_err(|e| anyhow!("{e}"))?;
        let chunk_count = (file_size as f64 / min_chunk as f64).ceil() as i64;
        if chunk_count > max_parts {
            bail!("Maximum number of chunk parts exceeded");
        }
        if chunk_count < 2 {
            return Ok(None);
        }
        Ok(Some(chunk_count))
    }

    async fn upload_resources(&mut self) -> Result<()> {
        let min_file_size = self
            .ctx
            .config
            .get_i64("MIN_FILE_SIZE")
            .map_err(|e| anyhow!("{e}"))?;
        let max_file_size = self
            .ctx
            .config
            .get_i64("MAX_FILE_SIZE")
            .map_err(|e| anyhow!("{e}"))?;

        let csv_objects = self.csv_objects.clone();
        for (type_, data) in &csv_objects {
            self.billing.track_data_units(py_sizeof_csv_object(data, type_));
            let file_size = crate::utils::py_sizeof_str(data);

            let key = if file_size < min_file_size {
                bucket::single_upload(self.ctx, data.as_bytes(), type_, &self.hyperparam_tuning_job_id, &self.bucket)
                    .await?
            } else if file_size > min_file_size && file_size < max_file_size {
                match self.plan_chunks(file_size)? {
                    None => bail!("Could not chunk file! Less thank 2 chunks."),
                    Some(_chunk_count) => {
                        bucket::upload_assembled(
                            self.ctx,
                            data.as_bytes(),
                            type_,
                            &self.hyperparam_tuning_job_id,
                            &self.bucket,
                        )
                        .await?
                    }
                }
            } else {
                bail!(
                    "Invalid file size. File size exceeds maximum. Account ID: {} File type : {type_}",
                    self.account_id
                );
            };

            self.training_resources.push(TrainingResource {
                channel_name: type_.clone(),
                training_resource_location: key.clone(),
            });
            self.key = Some(key);
            self.total_bytes += file_size;
        }

        self.billing.complete_data_units(self.ctx, "MB").await?;
        Ok(())
    }

    async fn create_hparam_tuning_job_ref(&self) -> Result<()> {
        let meta_data = json!({
            "event_type": self.algo().event_type()?,
            "features": self.features,
        });
        repo::create_hparam_tuning_job_ref(
            self.ctx,
            self.items_df.as_ref().ok_or_else(|| anyhow!("items_df not built"))?,
            self.profiles_df.as_ref().ok_or_else(|| anyhow!("profiles_df not built"))?,
            &self.hyperparam_tuning_job_id,
            &self.account_id,
            &meta_data,
        )
        .await
    }

    async fn start_cloud_hparam_tuning_job(&self) -> Result<()> {
        let parent_job = repo::get_parent_hparam_tuning_job_ref(self.ctx, &self.account_id).await;
        let parent_job_id = parent_job
            .as_ref()
            .and_then(|job| job.get("_id"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let volume_size = required_gb(self.total_bytes)?;
        repo::start_hparam_tuning_job(
            self.ctx,
            &self.account_id,
            &self.hyperparam_tuning_job_id,
            parent_job_id.as_deref(),
            volume_size,
            &self.training_resources,
            &self.bucket,
        )
        .await
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
        // NOTE: the Python training job callback sent a `cursor: 0` header here while
        // the completion job's equivalent callback (prediction.rs) sent none — an
        // inconsistency in the original, not a typo here. Preserved for parity.
        post_json_with_retry(&url, &[("cursor", "0")], &payload).await?;
        Ok(())
    }

    async fn dispose_job(&mut self, ex: &str) {
        let result: Result<()> = async {
            repo::delete_hparam_tuning_job_ref(self.ctx, &self.account_id, &self.hyperparam_tuning_job_id).await?;
            bucket::abort_multipart_upload(self.key.as_deref(), None, &self.bucket).await;
            self.send_job_callback(
                &format!("Recommender hyper parameter tuning Job failed. EX :: {ex}"),
                "failed",
            )
            .await?;
            Ok(())
        }
        .await;

        if let Err(err) = result {
            let _ = self.billing.complete_compute_units(self.ctx, 0.0).await;
            eprintln!("[recommendation-worker] Error occurred when attempting to dispose of job. {err}");
        }
    }

    async fn complete_job(&self) -> Result<()> {
        self.send_job_callback("Recommender hyper parameter tuning Job successfully initated", "success")
            .await?;

        let payload = json!({
            "account_id": self.account_id,
            "job_meta": {
                "job_type": "rec",
                "amqp_routing_key": "rec.training.complete.cmd.run",
                "required_permissions": ["rec"],
                "required_configurations": {
                    "account_attributes": [
                        "account_configurations.webhook_url",
                        "account_configurations.account_type",
                        "account_configurations.account_currency",
                        "bucket",
                    ],
                    "algorithm_configuration_idxs": [0],
                },
                "desired_runs": 1,
                "time_interval": 60,
                "fail_threshold": 3,
            },
            "job_data": {
                "hyperparam_tuning_job_id": self.hyperparam_tuning_job_id,
            }
        });
        if let Err(e) = self.ctx.gateway.amqp_publish("octy.job.cmd.create", &payload).await {
            eprintln!("[recommendation-worker] failed to publish octy.job.cmd.create: {e}");
        }
        Ok(())
    }

    pub async fn run(mut self) {
        let result: Result<()> = async {
            self.billing.track_compute_units("hours")?;
            self.build_profiles_dataset().await?;
            self.build_items_dataset().await?;
            self.build_events_dataset().await?;
            self.feature_engineering().await?;
            self.create_csv_objects().await?;
            self.upload_resources().await?;
            self.start_cloud_hparam_tuning_job().await?;
            self.create_hparam_tuning_job_ref().await?;
            self.complete_job().await?;
            self.billing.complete_compute_units(self.ctx, 0.0).await?;
            Ok(())
        }
        .await;

        if let Err(e) = result {
            eprintln!("[recommendation-worker] {e}");
            let _ = self.billing.complete_compute_units(self.ctx, 0.0).await;
            self.dispose_job(&e.to_string()).await;
        }
    }
}
