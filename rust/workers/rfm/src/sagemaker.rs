//! Port of the SageMaker control-plane calls in
//! `data/repositories/implementation/rfm_repository.py` (`self.s3_client`,
//! a `boto3.client('sagemaker', ...)` despite the misleading attribute
//! name in the Python source).
//!
//! Sent as SigV4-signed `application/x-amz-json-1.1` requests straight from
//! the WASM component (`octy_spin::aws::send_signed`), matching the AWS
//! JSON-RPC protocol boto3 uses under the hood — no native AWS SDK required.

use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::http::Method;

use octy_spin::aws::{send_signed, AwsCreds};
use octy_spin::ctx::Ctx;

pub struct TrainingResource {
    pub channel_name: String,
    pub training_resource_location: String,
}

fn endpoint(region: &str) -> String {
    format!("https://api.sagemaker.{region}.amazonaws.com/")
}

async fn call(ctx: &Ctx, operation: &str, body: &Value) -> Result<Value, OctyError> {
    let creds = AwsCreds::from_ctx(ctx)?;
    let url = endpoint(&creds.region);
    let payload = serde_json::to_vec(body).expect("serializable json");
    let (status, resp_body) = send_signed(
        &creds,
        "sagemaker",
        Method::Post,
        &url,
        &[
            ("x-amz-target", &format!("SageMaker.{operation}")),
            ("content-type", "application/x-amz-json-1.1"),
        ],
        payload,
    )
    .await?;

    let parsed: Value = serde_json::from_slice(&resp_body).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(OctyError::internal(format!(
            "SageMaker.{operation} returned status {status}: {parsed}"
        )));
    }
    Ok(parsed)
}

/// `start_cloud_training` → `CreateTrainingJob`.
pub async fn create_training_job(
    ctx: &Ctx,
    account_id: &str,
    training_job_id: &str,
    volume_size: i64,
    training_resources: &[TrainingResource],
    bucket_name: &str,
) -> Result<(), OctyError> {
    let input_mode = ctx.config.get_str("RFM_SM_INPUT_MODE")?;
    let out_path = ctx.config.get_str("RFM_MODELS_DIR")?;
    let training_image = ctx.config.get_str("RFM_ALGORITHM_DOCKER_PATH")?;
    let role_arn = ctx.config.get_str("AWS_ROLE_ARN")?;
    let instance_type = ctx.config.get_str("EC2_INSTANCE_TYPE")?;
    let max_runtime = ctx.config.get_i64("TRAINING_MAX_RUN_TIME")?;

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

    let body = json!({
        "TrainingJobName": training_job_id,
        "HyperParameters": {},
        "AlgorithmSpecification": {
            "TrainingImage": training_image,
            "TrainingInputMode": input_mode,
        },
        "RoleArn": role_arn,
        "InputDataConfig": input_data,
        "OutputDataConfig": { "S3OutputPath": format!("s3://{bucket_name}/{out_path}") },
        "ResourceConfig": {
            "InstanceType": instance_type,
            "InstanceCount": 1,
            "VolumeSizeInGB": volume_size,
        },
        "StoppingCondition": { "MaxRuntimeInSeconds": max_runtime },
        "Tags": [ { "Key": "octy_account_id", "Value": account_id } ],
    });

    call(ctx, "CreateTrainingJob", &body).await?;
    Ok(())
}

/// `get_cloud_training_status_time` → `DescribeTrainingJob`. Returns
/// `(TrainingJobStatus, TrainingTimeInSeconds / 3600)`.
pub async fn describe_training_job(ctx: &Ctx, training_job_id: &str) -> Result<(String, f64), OctyError> {
    let body = json!({ "TrainingJobName": training_job_id });
    let res = call(ctx, "DescribeTrainingJob", &body).await?;
    let status = res
        .get("TrainingJobStatus")
        .and_then(Value::as_str)
        .ok_or_else(|| OctyError::internal("DescribeTrainingJob response missing TrainingJobStatus"))?
        .to_string();
    let seconds = res.get("TrainingTimeInSeconds").and_then(Value::as_f64).unwrap_or(0.0);
    Ok((status, seconds / 3600.0))
}
