//! Port of `data/repositories/implementation/bucket_repository.py`.
//!
//! S3 is reached through the data gateway's object endpoints
//! (`/v1/s3/{put-object,get-object,list-objects,delete-object}`; bodies are
//! base64). The gateway has no multipart-upload bridge, so the Python MPU
//! path (`create_multipart_upload` / `upload_part` /
//! `complete_multipart_upload`) is replaced by validating the chunking
//! exactly like the Python did and then uploading the assembled object with
//! a single put-object (documented divergence; `abort_multipart_upload`
//! becomes a no-op — there is never a dangling MPU to abort).

use anyhow::{anyhow, bail, Result};
use base64::Engine;
use serde_json::{json, Value};

use octy_spin::ctx::Ctx;

use crate::http::gateway_post;
use crate::tar_gz::{extract_tar_gz, TarEntry};
use crate::utils::generate_uid;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// `_generate_file_key` — `{REC_DATA_DIR}/{job_id}/{key}.csv|.json`.
pub fn generate_file_key(
    ctx: &Ctx,
    resource_friendly_name: &str,
    hyperparam_tuning_job_id: &str,
) -> Result<String> {
    let key = generate_uid("key");
    let dir = ctx.config.get_str("REC_DATA_DIR").map_err(|e| anyhow!("{e}"))?;
    let ext = if resource_friendly_name.contains("meta_data") {
        "json"
    } else {
        "csv"
    };
    Ok(format!("{dir}/{hyperparam_tuning_job_id}/{key}.{ext}"))
}

async fn put_object(bucket: &str, key: &str, data: &[u8]) -> Result<()> {
    let res = gateway_post(
        "/v1/s3/put-object",
        &json!({
            "bucket": bucket,
            "key": key,
            "body_base64": B64.encode(data),
        }),
    )
    .await?;
    if !res.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        bail!("s3 put-object failed for {bucket}/{key}");
    }
    Ok(())
}

/// `single_upload` — returns the object key.
pub async fn single_upload(
    ctx: &Ctx,
    file_data: &[u8],
    resource_friendly_name: &str,
    hyperparam_tuning_job_id: &str,
    bucket_name: &str,
) -> Result<String> {
    let key = generate_file_key(ctx, resource_friendly_name, hyperparam_tuning_job_id)?;
    put_object(bucket_name, &key, file_data).await?;
    Ok(key)
}

/// The multipart path: same key generation and part accounting as the Python
/// (the caller still validates chunk counts), but the payload travels to S3
/// as one put-object through the gateway.
pub async fn upload_assembled(
    ctx: &Ctx,
    file_data: &[u8],
    resource_friendly_name: &str,
    hyperparam_tuning_job_id: &str,
    bucket_name: &str,
) -> Result<String> {
    let key = generate_file_key(ctx, resource_friendly_name, hyperparam_tuning_job_id)?;
    put_object(bucket_name, &key, file_data).await?;
    Ok(key)
}

/// `abort_multipart_upload` — the gateway upload path never leaves a
/// dangling MPU, and the Python swallowed every error here anyway.
pub async fn abort_multipart_upload(_key: Option<&str>, _upload_id: Option<&str>, _bucket_name: &str) {}

/// `download_resource(..., is_compressed=True)` — fetch + gunzip + untar.
pub async fn download_resource_compressed(bucket_name: &str, key: &str) -> Result<Vec<TarEntry>> {
    let res = gateway_post(
        "/v1/s3/get-object",
        &json!({ "bucket": bucket_name, "key": key }),
    )
    .await?;
    if !res.get("found").and_then(Value::as_bool).unwrap_or(false) {
        bail!("s3 object not found: {bucket_name}/{key}");
    }
    let body = res
        .get("body_base64")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("s3 get-object returned no body for {bucket_name}/{key}"))?;
    let bytes = B64
        .decode(body)
        .map_err(|e| anyhow!("invalid base64 body from gateway: {e}"))?;
    extract_tar_gz(&bytes)
}

/// `delete_directory` — list by prefix, delete each object.
pub async fn delete_directory(bucket_name: &str, directory_path: &str) -> Result<()> {
    let res = gateway_post(
        "/v1/s3/list-objects",
        &json!({ "bucket": bucket_name, "prefix": directory_path }),
    )
    .await?;
    let keys: Vec<String> = res
        .get("keys")
        .and_then(Value::as_array)
        .map(|keys| {
            keys.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    for key in keys {
        let res = gateway_post(
            "/v1/s3/delete-object",
            &json!({ "bucket": bucket_name, "key": key }),
        )
        .await?;
        if !res.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            bail!("s3 delete-object failed for {bucket_name}/{key}");
        }
    }
    Ok(())
}
