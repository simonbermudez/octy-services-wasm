//! Port of the S3 side of `data/repositories/implementation/bucket_repository.py`
//! on top of the data-gateway's `/v1/s3/*` object endpoints
//! (`get-object` / `put-object` / `list-objects` / `delete-object`).
//!
//! The gateway does **not** expose S3 multipart-upload endpoints
//! (`create-multipart-upload` / `upload-part` / `complete-multipart-upload` /
//! `abort-multipart-upload`) — see `training.rs` for how the chunked-upload
//! path in the Python service is collapsed to a single `put-object` call,
//! and the final report for this documented gateway capability gap.

use base64::Engine;
use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::http::Method;

use octy_spin::gateway::http_send;

fn gateway_base() -> String {
    octy_spin::ctx::variable("gateway_url", "GATEWAY_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string())
}

async fn call(path: &str, body: &Value) -> Result<Value, OctyError> {
    let url = format!("{}{}", gateway_base(), path);
    let (status, resp_body) = http_send(
        Method::Post,
        &url,
        &[("content-type", "application/json")],
        Some(serde_json::to_vec(body).expect("serializable json")),
    )
    .await?;
    let parsed: Value = serde_json::from_slice(&resp_body).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(OctyError::internal(format!("gateway {path} returned status {status}: {parsed}")));
    }
    Ok(parsed)
}

/// `download_resource(..., is_compressed=False)` for a single object;
/// returns `None` if the object does not exist (`found: false`).
pub async fn get_object(bucket: &str, key: &str) -> Result<Option<Vec<u8>>, OctyError> {
    let res = call("/v1/s3/get-object", &json!({ "bucket": bucket, "key": key })).await?;
    if !res.get("found").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(None);
    }
    let b64 = res.get("body_base64").and_then(Value::as_str).unwrap_or("");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| OctyError::internal(format!("invalid base64 from gateway get-object: {e}")))?;
    Ok(Some(bytes))
}

/// `single_upload` / the collapsed multipart-upload path.
pub async fn put_object(bucket: &str, key: &str, body: &[u8], content_type: &str) -> Result<(), OctyError> {
    let body_base64 = base64::engine::general_purpose::STANDARD.encode(body);
    let res = call(
        "/v1/s3/put-object",
        &json!({
            "bucket": bucket,
            "key": key,
            "body_base64": body_base64,
            "content_type": content_type,
        }),
    )
    .await?;
    if !res.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Err(OctyError::internal(format!("put-object {bucket}/{key} failed")));
    }
    Ok(())
}

pub async fn list_objects(bucket: &str, prefix: &str) -> Result<Vec<String>, OctyError> {
    let res = call("/v1/s3/list-objects", &json!({ "bucket": bucket, "prefix": prefix })).await?;
    Ok(res
        .get("keys")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default())
}

pub async fn delete_object(bucket: &str, key: &str) -> Result<(), OctyError> {
    call("/v1/s3/delete-object", &json!({ "bucket": bucket, "key": key })).await?;
    Ok(())
}

/// `delete_directory`: list every key under `directory_path` and delete each
/// one. The Python version (`bucket.objects.filter(Prefix=...).delete()`)
/// does not fail the whole operation if an individual delete fails; we mirror
/// that by logging (not propagating) per-object delete errors.
pub async fn delete_directory(bucket: &str, directory_path: &str) -> Result<(), OctyError> {
    let keys = list_objects(bucket, directory_path).await?;
    for key in keys {
        if let Err(err) = delete_object(bucket, &key).await {
            eprintln!("[rfm-worker] delete_directory: failed to delete {bucket}/{key}: {err}");
        }
    }
    Ok(())
}

/// `download_resource(..., is_compressed=True)`: fetch a `.tar.gz` object
/// and return each archive member as `(file_name, file_bytes)`.
pub async fn download_and_extract_targz(bucket: &str, key: &str) -> Result<Vec<(String, Vec<u8>)>, OctyError> {
    let bytes = get_object(bucket, key)
        .await?
        .ok_or_else(|| OctyError::internal(format!("object not found: {bucket}/{key}")))?;

    let gz = flate2::read::GzDecoder::new(std::io::Cursor::new(bytes));
    let mut archive = tar::Archive::new(gz);
    let mut out = Vec::new();
    let entries = archive
        .entries()
        .map_err(|e| OctyError::internal(format!("error occurred when downloading and decompressing file -- file too small: {e}")))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| OctyError::internal(format!("tar entry read failed: {e}")))?;
        let name = entry
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut data)
            .map_err(|e| OctyError::internal(format!("tar entry extract failed: {e}")))?;
        out.push((name, data));
    }
    Ok(out)
}
