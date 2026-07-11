//! SageMaker control-plane client — port of the boto3 `sagemaker` client
//! calls in `data/repositories/implementation/churn_repository.py`
//! (`create_hyper_parameter_tuning_job`, `describe_hyper_parameter_tuning_job`).
//!
//! Uses SigV4-signed requests from the component
//! (`octy_spin::aws::send_signed`, service name `"sagemaker"`), JSON-1.1
//! protocol: POST https://api.sagemaker.{region}.amazonaws.com/ with
//! `x-amz-target: SageMaker.<Operation>`.

use octy_shared::errors::OctyError;
use octy_spin::aws::AwsCreds;
use octy_spin::ctx::Ctx;
use serde_json::{json, Value};
use spin_sdk::http::Method;

async fn sm_call(creds: &AwsCreds, operation: &str, body: &Value) -> Result<Value, OctyError> {
    let url = format!("https://api.sagemaker.{}.amazonaws.com/", creds.region);
    let target = format!("SageMaker.{operation}");
    let payload = serde_json::to_vec(body).expect("serializable json");
    let (status, response) = octy_spin::aws::send_signed(
        creds,
        "sagemaker",
        Method::Post,
        &url,
        &[
            ("content-type", "application/x-amz-json-1.1"),
            ("x-amz-target", target.as_str()),
        ],
        payload,
    )
    .await?;
    let parsed: Value = serde_json::from_slice(&response).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        let detail = parsed
            .get("message")
            .or_else(|| parsed.get("Message"))
            .and_then(Value::as_str)
            .unwrap_or("SageMaker error");
        return Err(OctyError::internal(format!(
            "SageMaker.{operation} returned status {status}: {detail}"
        )));
    }
    Ok(parsed)
}

/// `start_hparam_tuning_job` — same job configuration as the Python, built
/// from config values.
#[allow(clippy::too_many_arguments)]
pub async fn create_hyper_parameter_tuning_job(
    ctx: &Ctx,
    account_id: &str,
    hyperparam_tuning_job_id: &str,
    parent_hyperparam_tuning_job_id: Option<&str>,
    volume_size: i64,
    training_resources: &[Value],
    bucket_name: &str,
) -> Result<(), OctyError> {
    let creds = AwsCreds::from_ctx(ctx)?;
    let config = &ctx.config;

    let tuning_objective_metric = config.get_str("CHURN_OBJECTIVE_METRIC")?;
    let input_mode = config.get_str("CHURN_SM_INPUT_MODE")?;
    let out_path = config.get_str("CHURN_PRED_MODELS_DIR")?;
    let training_image = config.get_str("CHURN_ALGORITHM_DOCKER_PATH")?;
    let role_arn = config.get_str("AWS_ROLE_ARN")?;
    let max_runtime = config.get_i64("CHURN_TRAINING_MAX_RUN_TIME")?;
    let churn_instance_type = config.get_str("EC2_INSTANCE_TYPE")?;
    let static_hp = config.get("CHURN_TRAINING_STATIC_HYPERPARAMETERS")?;

    let input_data: Vec<Value> = training_resources
        .iter()
        .map(|res| {
            json!({
                "ChannelName": res["channel_name"],
                "DataSource": {
                    "S3DataSource": {
                        "S3DataType": "S3Prefix",
                        "S3Uri": format!(
                            "s3://{bucket_name}/{}",
                            res["training_resource_location"].as_str().unwrap_or_default()
                        ),
                        "S3DataDistributionType": "FullyReplicated"
                    }
                },
                "ContentType": "text/csv",
                "CompressionType": "None",
                "RecordWrapperType": "None",
                "InputMode": input_mode
            })
        })
        .collect();

    let tuning_job_config = json!({
        "Strategy": "Bayesian",
        "HyperParameterTuningJobObjective": {
            "Type": "Maximize",
            "MetricName": tuning_objective_metric
        },
        "ResourceLimits": {
            "MaxNumberOfTrainingJobs": 1,
            "MaxParallelTrainingJobs": 1
        },
        "ParameterRanges": {
            "IntegerParameterRanges": [
                { "Name": "n_estimators", "MinValue": "139", "MaxValue": "140" },
                { "Name": "min_child_weight", "MinValue": "1", "MaxValue": "2" },
                { "Name": "gamma", "MinValue": "0", "MaxValue": "1" },
                { "Name": "seed", "MinValue": "26", "MaxValue": "28" }
            ],
            "ContinuousParameterRanges": [
                { "Name": "learning_rate", "MinValue": "0.1", "MaxValue": "0.2" },
                { "Name": "colsample_bytree", "MinValue": "0.7", "MaxValue": "0.9" }
            ]
        },
        "TrainingJobEarlyStoppingType": "Auto",
        "TuningJobCompletionCriteria": {
            "TargetObjectiveMetricValue": 0.95
        }
    });

    let training_job_definition = json!({
        "StaticHyperParameters": {
            "nthread": static_hp["nthread"],
            "max_depth": static_hp["max_depth"],
            "subsample": static_hp["subsample"],
            "objective": static_hp["objective"],
            "scale_pos_weight": static_hp["scale_pos_weight"],
        },
        "AlgorithmSpecification": {
            "TrainingImage": training_image,
            "TrainingInputMode": input_mode,
            "MetricDefinitions": [
                {
                    "Name": tuning_objective_metric,
                    "Regex": format!("{tuning_objective_metric}=(.*?);"),
                }
            ]
        },
        "RoleArn": role_arn,
        "InputDataConfig": input_data,
        "OutputDataConfig": {
            "S3OutputPath": format!("s3://{bucket_name}/{out_path}")
        },
        "ResourceConfig": {
            "InstanceType": churn_instance_type,
            "InstanceCount": 1,
            "VolumeSizeInGB": volume_size
        },
        "StoppingCondition": {
            "MaxRuntimeInSeconds": max_runtime
        }
    });

    let mut request = json!({
        "HyperParameterTuningJobName": hyperparam_tuning_job_id,
        "HyperParameterTuningJobConfig": tuning_job_config,
        "TrainingJobDefinition": training_job_definition,
        "Tags": [
            { "Key": "octy_account_id", "Value": account_id }
        ]
    });
    if let Some(parent) = parent_hyperparam_tuning_job_id {
        request["WarmStartConfig"] = json!({
            "ParentHyperParameterTuningJobs": [
                { "HyperParameterTuningJobName": parent }
            ],
            "WarmStartType": "IdenticalDataAndAlgorithm"
        });
    }

    sm_call(&creds, "CreateHyperParameterTuningJob", &request).await?;
    Ok(())
}

pub async fn describe_hyper_parameter_tuning_job(
    ctx: &Ctx,
    hyperparam_tuning_job_id: &str,
) -> Result<Value, OctyError> {
    let creds = AwsCreds::from_ctx(ctx)?;
    sm_call(
        &creds,
        "DescribeHyperParameterTuningJob",
        &json!({ "HyperParameterTuningJobName": hyperparam_tuning_job_id }),
    )
    .await
}

/// `get_hparam_tuning_job_status` — the tuning-job status while in progress,
/// otherwise the best training job's status.
pub async fn get_hparam_tuning_job_status(
    ctx: &Ctx,
    hyperparam_tuning_job_id: &str,
) -> Result<String, OctyError> {
    let job = describe_hyper_parameter_tuning_job(ctx, hyperparam_tuning_job_id).await?;
    let tuning_status = job
        .get("HyperParameterTuningJobStatus")
        .and_then(Value::as_str)
        .ok_or_else(|| OctyError::internal("describe: missing HyperParameterTuningJobStatus"))?;
    if tuning_status == "InProgress" {
        return Ok(tuning_status.to_string());
    }
    job.pointer("/BestTrainingJob/TrainingJobStatus")
        .and_then(Value::as_str)
        .map(String::from)
        // Python raised KeyError here when no BestTrainingJob exists.
        .ok_or_else(|| OctyError::internal("describe: missing BestTrainingJob.TrainingJobStatus"))
}

/// `get_best_training_job` — `(best job summary, compute unit hours)`.
pub async fn get_best_training_job(
    ctx: &Ctx,
    hyperparam_tuning_job_id: &str,
) -> Result<(Value, f64), OctyError> {
    let hp_job = describe_hyper_parameter_tuning_job(ctx, hyperparam_tuning_job_id).await?;
    // JSON-1.1 timestamps are epoch seconds.
    let end = hp_job
        .get("HyperParameterTuningEndTime")
        .and_then(Value::as_f64)
        .ok_or_else(|| OctyError::internal("describe: missing HyperParameterTuningEndTime"))?;
    let start = hp_job
        .get("CreationTime")
        .and_then(Value::as_f64)
        .ok_or_else(|| OctyError::internal("describe: missing CreationTime"))?;
    let mut training_compute_units = (end - start) / 3600.0;
    if training_compute_units < 1.0 {
        training_compute_units = 1.0;
    }

    let job = hp_job
        .get("BestTrainingJob")
        .ok_or_else(|| OctyError::internal("describe: missing BestTrainingJob"))?;
    let field = |k: &str| -> Result<Value, OctyError> {
        job.get(k)
            .cloned()
            .ok_or_else(|| OctyError::internal(format!("BestTrainingJob missing {k}")))
    };
    let summary = json!({
        "training_job_name": field("TrainingJobName")?,
        "training_job_arn": field("TrainingJobArn")?,
        "creation_time": field("CreationTime")?,
        "training_start_time": field("TrainingStartTime")?,
        "training_end_time": field("TrainingEndTime")?,
        "training_job_status": field("TrainingJobStatus")?,
        "tuned_hyper_parameters": field("TunedHyperParameters")?,
        "final_hyper_parameter_tuning_job_objective_metric":
            field("FinalHyperParameterTuningJobObjectiveMetric")?,
        "objective_status": field("ObjectiveStatus")?,
    });
    Ok((summary, training_compute_units))
}
