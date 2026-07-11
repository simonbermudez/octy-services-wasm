//! Port of `data/repositories/implementation/recommendation_repository.py`.
//!
//! * Raw training data comes from the internal HTTP APIs of the events /
//!   profiles / items / segmentation services (direct outbound HTTP from the
//!   component, cursor-paginated exactly like the Python).
//! * Hyper-parameter tuning job references live in MongoDB, reached through
//!   the data gateway (legacy extended JSON).
//! * Tuning jobs run on **AWS SageMaker** — `CreateHyperParameterTuningJob` /
//!   `DescribeHyperParameterTuningJob` via SigV4-signed JSON-1.1 calls.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use octy_spin::ctx::Ctx;

use crate::frame::Frame;
use crate::http::{gateway_post, get_with_retry, post_json_with_retry};
use crate::sagemaker::sagemaker_call;

const JOBS_COLLECTION: &str = "tbl_hparam_tuning_jobs";
const CACHE_COLLECTION: &str = "tbl_recommendations_cache";

fn cfg_str(ctx: &Ctx, key: &str) -> Result<String> {
    Ok(ctx.config.get_str(key).map_err(|e| anyhow!("{e}"))?.to_string())
}

fn cfg_i64(ctx: &Ctx, key: &str) -> Result<i64> {
    ctx.config.get_i64(key).map_err(|e| anyhow!("{e}"))
}

// ---------------------------------------------------------------------------
// Raw training data (internal service HTTP APIs)
// ---------------------------------------------------------------------------

/// Shared cursor-pagination loop: request pages until a non-200 response
/// (that is how the Python detected exhaustion). A zero-count 200 page also
/// stops the loop — the Python would have spun forever on one.
async fn paginate(
    url: &str,
    payload: Option<&Value>,
    page_key: &str,
) -> Result<Vec<Value>> {
    let mut results: Vec<Value> = Vec::new();
    let mut cursor: i64 = 0;
    loop {
        let cursor_header = cursor.to_string();
        let headers = [("cursor", cursor_header.as_str())];
        let (status, body) = match payload {
            Some(payload) => post_json_with_retry(url, &headers, payload).await?,
            None => get_with_retry(url, &headers).await?,
        };
        if status != 200 {
            break;
        }
        let body: Value = serde_json::from_slice(&body)
            .map_err(|e| anyhow!("invalid JSON from {url}: {e}"))?;
        let page = body
            .get(page_key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        results.extend(page);
        let count = body
            .pointer("/request_meta/count")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if count == 0 {
            break;
        }
        cursor += count;
    }
    Ok(results)
}

pub async fn get_events(
    ctx: &Ctx,
    account_id: &str,
    profile_ids: &[String],
    timeframe: i64,
    event_type: &str,
) -> Result<Vec<Value>> {
    let url = format!("{}/v1/internal/events", cfg_str(ctx, "EVENT_SERVICE_CLUSTER_IP")?);
    let payload = json!({
        "timeframe": timeframe,
        "account_id": account_id,
        "profile_ids": profile_ids,
        "event_type": event_type,
    });
    paginate(&url, Some(&payload), "events").await
}

/// `ids='true'` collapses each profile to its `profile_id` string.
pub async fn get_profiles(ctx: &Ctx, account_id: &str, ids: bool) -> Result<Vec<Value>> {
    let url = format!(
        "{}/v1/internal/profiles?ids={}",
        cfg_str(ctx, "PROFILE_SERVICE_CLUSTER_IP")?,
        if ids { "true" } else { "false" }
    );
    let payload = json!({
        "account_id": account_id,
        "profiles": [],
        "get_all": true,
    });
    let profiles = paginate(&url, Some(&payload), "profiles").await?;
    if ids {
        Ok(profiles
            .into_iter()
            .filter_map(|p| p.get("profile_id").cloned())
            .collect())
    } else {
        Ok(profiles)
    }
}

pub async fn get_items(ctx: &Ctx, account_id: &str, ids: bool, status: &str) -> Result<Vec<Value>> {
    let url = format!(
        "{}/v1/internal/items?account_id={}&ids={}&status={}",
        cfg_str(ctx, "ITEM_SERVICE_CLUSTER_IP")?,
        account_id,
        if ids { "true" } else { "false" },
        status
    );
    let items = paginate(&url, None, "items").await?;
    if ids {
        Ok(items
            .into_iter()
            .filter_map(|item| item.get("item_id").cloned())
            .collect())
    } else {
        Ok(items)
    }
}

pub async fn get_segments(ctx: &Ctx, account_id: &str) -> Result<Vec<Value>> {
    let url = format!(
        "{}/v1/internal/segments?account_id={}&segment_type=all&status=active",
        cfg_str(ctx, "SEGMENTATION_SERVICE_CLUSTER_IP")?,
        account_id
    );
    let (status, body) = get_with_retry(&url, &[]).await?;
    // Python parsed the body unconditionally, then only used it when < 202.
    let body: Value =
        serde_json::from_slice(&body).map_err(|e| anyhow!("invalid JSON from {url}: {e}"))?;
    if status < 202 {
        Ok(body
            .get("segments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    } else {
        Ok(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// Hyper-parameter tuning job references (MongoDB via the data gateway)
// ---------------------------------------------------------------------------

/// `hyperparam_tuning_job_id` is the mongoengine primary key → `_id`.
pub async fn create_hparam_tuning_job_ref(
    ctx: &Ctx,
    items_df: &Frame,
    profiles_df: &Frame,
    hyperparam_tuning_job_id: &str,
    account_id: &str,
    meta_data: &Value,
) -> Result<()> {
    let mut lfm_idx_map: Vec<Value> = Vec::new();
    // Each row's position in items_df/profiles_df is its LFM index, so the
    // enumerate() index here is what prediction.rs later looks up embeddings by.
    let mut build_input = |ids: Vec<Value>, type_: &str| {
        for (i, id_) in ids.into_iter().enumerate() {
            lfm_idx_map.push(json!({
                "lfm_idx": i as i64,
                "type_": type_,
                "res_id": id_,
            }));
        }
    };
    build_input(profiles_df.column_values("profile_id")?, "profiles");
    build_input(items_df.column_values("item_id")?, "items");

    let now = octy_shared::ejson::now_legacy_date();
    let document = json!({
        "_id": hyperparam_tuning_job_id,
        "account_id": account_id,
        "meta_data": meta_data,
        "lfm_idxs": lfm_idx_map,
        "status": "in_progress",
        "created_at": now,
        "updated_at": now,
    });
    ctx.gateway
        .insert_one(JOBS_COLLECTION, document)
        .await
        .map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

/// mongoengine `.get(...)` raised `DoesNotExist` for a missing document.
pub async fn get_hparam_tuning_job_ref(
    ctx: &Ctx,
    account_id: &str,
    hyperparam_tuning_job_id: &str,
    status: &str,
) -> Result<Value> {
    ctx.gateway
        .find_one(
            JOBS_COLLECTION,
            json!({
                "account_id": account_id,
                "_id": hyperparam_tuning_job_id,
                "status": status,
            }),
        )
        .await
        .map_err(|e| anyhow!("{e}"))?
        .ok_or_else(|| anyhow!("tbl_hparam_tuning_jobs matching query does not exist"))
}

/// Latest 'Completed' job for the account (or None). The gateway `find` has
/// no sort parameter, so the sort by `updated_at` desc happens client-side.
/// The Python swallowed every error here (`except: return None`) — kept.
pub async fn get_parent_hparam_tuning_job_ref(ctx: &Ctx, account_id: &str) -> Option<Value> {
    let docs = ctx
        .gateway
        .find(
            JOBS_COLLECTION,
            json!({ "account_id": account_id, "status": "Completed" }),
            0,
            0,
        )
        .await
        .ok()?;
    docs.into_iter().max_by_key(|doc| {
        doc.get("updated_at")
            .and_then(octy_shared::ejson::date_millis)
            .unwrap_or(i64::MIN)
    })
}

/// Bug-for-bug: without `model_meta` the Python only updated `status` and
/// `updated_at` — `best_model_training_job_id` was silently dropped.
pub async fn update_hparam_tuning_job_ref(
    ctx: &Ctx,
    account_id: &str,
    hyperparam_tuning_job_id: &str,
    best_model_training_job_id: &str,
    status: &str,
    model_meta: Option<&Value>,
) -> Result<()> {
    let filter = json!({
        "account_id": account_id,
        "_id": hyperparam_tuning_job_id,
    });
    let set = match model_meta {
        Some(meta) => json!({
            "status": status,
            "best_model_training_job_id": best_model_training_job_id,
            "best_model_meta_data": meta,
            "updated_at": octy_shared::ejson::now_legacy_date(),
        }),
        None => json!({
            "status": status,
            "updated_at": octy_shared::ejson::now_legacy_date(),
        }),
    };
    ctx.gateway
        .update_one(JOBS_COLLECTION, filter, json!({ "$set": set }))
        .await
        .map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

pub async fn delete_hparam_tuning_job_ref(
    ctx: &Ctx,
    account_id: &str,
    hyperparam_tuning_job_id: &str,
) -> Result<()> {
    ctx.gateway
        .delete_one(
            JOBS_COLLECTION,
            json!({ "account_id": account_id, "_id": hyperparam_tuning_job_id }),
        )
        .await
        .map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Cloud hyper-parameter tuning jobs (SageMaker)
// ---------------------------------------------------------------------------

pub struct TrainingResource {
    pub channel_name: String,
    pub training_resource_location: String,
}

pub async fn start_hparam_tuning_job(
    ctx: &Ctx,
    account_id: &str,
    hyperparam_tuning_job_id: &str,
    parent_hyperparam_tuning_job_id: Option<&str>,
    volume_size: i64,
    training_resources: &[TrainingResource],
    bucket_name: &str,
) -> Result<()> {
    let tuning_objective_metric = cfg_str(ctx, "RECOMMENDATION_OBJECTIVE_METRIC")?;
    let input_mode = cfg_str(ctx, "RECOMMENDATION_SM_INPUT_MODE")?;
    let out_path = cfg_str(ctx, "REC_MODELS_DIR")?;
    let training_image = cfg_str(ctx, "RECOMMENDATION_ALGORITHM_DOCKER_PATH")?;
    let role_arn = cfg_str(ctx, "AWS_ROLE_ARN")?;
    let max_runtime = cfg_i64(ctx, "REC_TRAINING_MAX_RUN_TIME")?;
    let rec_instance_type = cfg_str(ctx, "EC2_INSTANCE_TYPE")?;
    let static_hyperparameters = ctx
        .config
        .get("REC_TRAINING_STATIC_HYPERPARAMETERS")
        .map_err(|e| anyhow!("{e}"))?;

    let input_data: Vec<Value> = training_resources
        .iter()
        .map(|res| {
            json!({
                "ChannelName": res.channel_name,
                "DataSource": {
                    "S3DataSource": {
                        "S3DataType": "S3Prefix",
                        "S3Uri": format!("s3://{bucket_name}/{}", res.training_resource_location),
                        "S3DataDistributionType": "FullyReplicated",
                    }
                },
                "ContentType": "text/csv",
                "CompressionType": "None",
                "RecordWrapperType": "None",
                "InputMode": input_mode,
            })
        })
        .collect();

    let hyper_parameter_tuning_job_config = json!({
        "Strategy": "Bayesian",
        "HyperParameterTuningJobObjective": {
            "Type": "Maximize",
            "MetricName": tuning_objective_metric,
        },
        "ResourceLimits": {
            "MaxNumberOfTrainingJobs": 4,
            "MaxParallelTrainingJobs": 4,
        },
        "ParameterRanges": {
            "IntegerParameterRanges": [
                { "Name": "no_components", "MinValue": "100", "MaxValue": "200" },
                { "Name": "random_state", "MinValue": "1000", "MaxValue": "3000" },
            ],
            "ContinuousParameterRanges": [
                { "Name": "learning_rate", "MinValue": "0.01", "MaxValue": "0.10" },
            ],
            "CategoricalParameterRanges": [
                { "Name": "learning_schedule", "Values": ["adagrad", "adadelta"] },
                { "Name": "loss", "Values": ["logistic", "bpr", "warp"] },
            ]
        },
        "TrainingJobEarlyStoppingType": "Auto",
        "TuningJobCompletionCriteria": {
            "TargetObjectiveMetricValue": 1.0,
        }
    });

    let training_job_definition = json!({
        "StaticHyperParameters": {
            "epochs": static_hyperparameters.get("epochs").cloned().unwrap_or(Value::Null),
            "num_threads": static_hyperparameters.get("num_threads").cloned().unwrap_or(Value::Null),
        },
        "AlgorithmSpecification": {
            "TrainingImage": training_image,
            "TrainingInputMode": input_mode,
            "MetricDefinitions": [
                {
                    "Name": tuning_objective_metric,
                    "Regex": format!("{tuning_objective_metric}=(.*?);"),
                },
            ]
        },
        "RoleArn": role_arn,
        "InputDataConfig": input_data,
        "OutputDataConfig": {
            "S3OutputPath": format!("s3://{bucket_name}/{out_path}"),
        },
        "ResourceConfig": {
            "InstanceType": rec_instance_type,
            "InstanceCount": 1,
            "VolumeSizeInGB": volume_size,
        },
        "StoppingCondition": {
            "MaxRuntimeInSeconds": max_runtime,
        }
    });

    let mut request = json!({
        "HyperParameterTuningJobName": hyperparam_tuning_job_id,
        "HyperParameterTuningJobConfig": hyper_parameter_tuning_job_config,
        "TrainingJobDefinition": training_job_definition,
        "Tags": [
            { "Key": "octy_account_id", "Value": account_id },
        ],
    });
    if let Some(parent_id) = parent_hyperparam_tuning_job_id {
        request["WarmStartConfig"] = json!({
            "ParentHyperParameterTuningJobs": [
                { "HyperParameterTuningJobName": parent_id },
            ],
            "WarmStartType": "IdenticalDataAndAlgorithm",
        });
    }

    sagemaker_call(ctx, "CreateHyperParameterTuningJob", &request).await?;
    Ok(())
}

async fn describe_tuning_job(ctx: &Ctx, hyperparam_tuning_job_id: &str) -> Result<Value> {
    sagemaker_call(
        ctx,
        "DescribeHyperParameterTuningJob",
        &json!({ "HyperParameterTuningJobName": hyperparam_tuning_job_id }),
    )
    .await
}

/// Tuning-job status, falling through to the best training job's status once
/// the tuning job itself is no longer 'InProgress'. A finished tuning job
/// with no `BestTrainingJob` raised `KeyError` in Python — an error here.
pub async fn get_hparam_tuning_job_status(
    ctx: &Ctx,
    hyperparam_tuning_job_id: &str,
) -> Result<String> {
    let hpt_job = describe_tuning_job(ctx, hyperparam_tuning_job_id).await?;
    let job_status = hpt_job
        .get("HyperParameterTuningJobStatus")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("DescribeHyperParameterTuningJob returned no status"))?;
    if job_status == "InProgress" {
        return Ok(job_status.to_string());
    }
    hpt_job
        .pointer("/BestTrainingJob/TrainingJobStatus")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("'BestTrainingJob'"))
}

/// Returns `(best_training_job, training_compute_units)` — units are the
/// tuning job's wall-clock hours, floored at 1 (like the Python ternary).
/// JSON-1.1 timestamps arrive as epoch seconds.
pub async fn get_best_training_job(
    ctx: &Ctx,
    hyperparam_tuning_job_id: &str,
) -> Result<(Value, f64)> {
    let hp_job = describe_tuning_job(ctx, hyperparam_tuning_job_id).await?;
    let end = hp_job
        .get("HyperParameterTuningEndTime")
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow!("'HyperParameterTuningEndTime'"))?;
    let creation = hp_job
        .get("CreationTime")
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow!("'CreationTime'"))?;
    let training_compute_units = (end - creation) / 3600.0;
    let job = hp_job
        .get("BestTrainingJob")
        .ok_or_else(|| anyhow!("'BestTrainingJob'"))?;

    let get = |key: &str| -> Result<Value> {
        job.get(key)
            .cloned()
            .ok_or_else(|| anyhow!("'{key}'"))
    };
    let best = json!({
        "training_job_name": get("TrainingJobName")?,
        "training_job_arn": get("TrainingJobArn")?,
        "creation_time": get("CreationTime")?,
        "training_start_time": get("TrainingStartTime")?,
        "training_end_time": get("TrainingEndTime")?,
        "training_job_status": get("TrainingJobStatus")?,
        "tuned_hyper_parameters": get("TunedHyperParameters")?,
        "final_hyper_parameter_tuning_job_objective_metric": get("FinalHyperParameterTuningJobObjectiveMetric")?,
        "objective_status": get("ObjectiveStatus")?,
    });
    Ok((
        best,
        if training_compute_units >= 1.0 {
            training_compute_units
        } else {
            1.0
        },
    ))
}

// ---------------------------------------------------------------------------
// Recommendations predictions cache
// ---------------------------------------------------------------------------

pub struct Prediction {
    pub profile_id: String,
    pub item_scores: Vec<(String, f64)>, // (item_id, score)
}

/// Deletes any cached recommendations for the account + training job, then
/// bulk-inserts the new predictions. The gateway exposes only
/// delete-one/insert-one, so both run as bounded loops (capability gap:
/// delete-many / insert-many).
pub async fn cache_item_recommendations(
    ctx: &Ctx,
    account_id: &str,
    training_job_id: &str,
    predictions: &[Prediction],
) -> Result<()> {
    let filter = json!({ "account_id": account_id, "training_job_id": training_job_id });

    // `.objects(...).delete()` — delete all matching documents.
    let mut guard = 0u64;
    loop {
        let res = gateway_post(
            &format!("/v1/mongo/{CACHE_COLLECTION}/delete-one"),
            &json!({ "filter": filter }),
        )
        .await?;
        let deleted = res.get("deleted").and_then(Value::as_i64).unwrap_or(0);
        if deleted == 0 {
            break;
        }
        guard += 1;
        if guard > 5_000_000 {
            bail!("delete loop exceeded bound while clearing recommendations cache");
        }
    }

    for prediction in predictions {
        let recommendations: Vec<Value> = prediction
            .item_scores
            .iter()
            .map(|(item_id, score)| json!({ "score": score, "item_id": item_id }))
            .collect();
        let document = json!({
            "account_id": account_id,
            "training_job_id": training_job_id,
            "profile_id": prediction.profile_id,
            "recommendations": recommendations,
            "created_at": octy_shared::ejson::now_legacy_date(),
        });
        if let Err(e) = ctx.gateway.insert_one(CACHE_COLLECTION, document).await {
            eprintln!("[recommendation-worker] bulk insert failed: {e}");
            bail!("Error occurred when attempting to cache recommendations");
        }
    }
    Ok(())
}

/// Helper: extract the identifier value from a raw JSON document that may be
/// either a plain string (ids='true' responses) or an object.
pub fn value_as_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
