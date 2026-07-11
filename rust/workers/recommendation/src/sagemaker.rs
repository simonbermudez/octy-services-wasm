//! SageMaker control-plane client — replaces `boto3.client('sagemaker')`.
//!
//! boto3 method calls map 1:1 onto the AWS JSON-1.1 API:
//!   `create_hyper_parameter_tuning_job(...)` → `SageMaker.CreateHyperParameterTuningJob`
//!   `describe_hyper_parameter_tuning_job(...)` → `SageMaker.DescribeHyperParameterTuningJob`
//! Requests are SigV4-signed (service `"sagemaker"`) and sent straight from
//! the WASM component via `octy_spin::aws::send_signed`.
//!
//! Training runs entirely on SageMaker (custom training image); the worker
//! never invokes a SageMaker endpoint — inference happens in-process from the
//! downloaded model artifacts.

use anyhow::{anyhow, Result};
use serde_json::Value;
use spin_sdk::http::Method;

use octy_spin::aws::{send_signed, AwsCreds};
use octy_spin::ctx::Ctx;

pub async fn sagemaker_call(ctx: &Ctx, operation: &str, payload: &Value) -> Result<Value> {
    let creds = AwsCreds::from_ctx(ctx).map_err(|e| anyhow!("{e}"))?;
    let url = format!("https://api.sagemaker.{}.amazonaws.com/", creds.region);
    let target = format!("SageMaker.{operation}");
    let (status, body) = send_signed(
        &creds,
        "sagemaker",
        Method::Post,
        &url,
        &[
            ("x-amz-target", target.as_str()),
            ("content-type", "application/x-amz-json-1.1"),
        ],
        serde_json::to_vec(payload)?,
    )
    .await
    .map_err(|e| anyhow!("{e}"))?;

    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        let detail = parsed
            .get("message")
            .or_else(|| parsed.get("Message"))
            .and_then(Value::as_str)
            .unwrap_or_else(|| std::str::from_utf8(&body).unwrap_or("unknown error"));
        return Err(anyhow!("SageMaker.{operation} failed ({status}): {detail}"));
    }
    Ok(parsed)
}
