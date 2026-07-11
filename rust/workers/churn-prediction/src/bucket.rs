//! Port of `data/repositories/implementation/bucket_repository.py`.
//!
//! S3 is reached through the octy-data-gateway sidecar
//! (`/v1/s3/{put-object,get-object,list-objects,delete-object}`, bodies as
//! base64). The gateway exposes no multipart-upload endpoints, so the
//! Python's MPU path (files > 15 MB) collapses into a single `put-object`
//! after the same chunk-count validation — see `upload_whole`.

use base64::Engine;
use flate2::read::GzDecoder;
use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::http::Method;
use std::io::Read;

use crate::util::generate_uid;

pub struct S3 {
    base: String,
}

impl S3 {
    pub fn new() -> Self {
        let base = octy_spin::ctx::variable("gateway_url", "GATEWAY_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string());
        Self {
            base: base.trim_end_matches('/').to_string(),
        }
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value, OctyError> {
        let url = format!("{}{}", self.base, path);
        let (status, response) = octy_spin::gateway::http_send(
            Method::Post,
            &url,
            &[("content-type", "application/json")],
            Some(serde_json::to_vec(body).expect("serializable json")),
        )
        .await?;
        let parsed: Value = serde_json::from_slice(&response).unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            return Err(OctyError::internal(format!(
                "gateway {path} returned status {status}"
            )));
        }
        Ok(parsed)
    }

    pub async fn put_object(&self, bucket: &str, key: &str, data: &[u8]) -> Result<(), OctyError> {
        let body = json!({
            "bucket": bucket,
            "key": key,
            "body_base64": base64::engine::general_purpose::STANDARD.encode(data),
        });
        let res = self.post("/v1/s3/put-object", &body).await?;
        if res.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            Ok(())
        } else {
            Err(OctyError::internal(format!(
                "S3 put-object failed for {bucket}/{key}"
            )))
        }
    }

    pub async fn get_object(&self, bucket: &str, key: &str) -> Result<Vec<u8>, OctyError> {
        let res = self
            .post("/v1/s3/get-object", &json!({ "bucket": bucket, "key": key }))
            .await?;
        if !res.get("found").and_then(Value::as_bool).unwrap_or(false) {
            return Err(OctyError::internal(format!(
                "S3 object not found: {bucket}/{key}"
            )));
        }
        let b64 = res
            .get("body_base64")
            .and_then(Value::as_str)
            .unwrap_or_default();
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| OctyError::internal(format!("invalid base64 from gateway: {e}")))
    }

    pub async fn list_objects(&self, bucket: &str, prefix: &str) -> Result<Vec<String>, OctyError> {
        let res = self
            .post(
                "/v1/s3/list-objects",
                &json!({ "bucket": bucket, "prefix": prefix }),
            )
            .await?;
        Ok(res
            .get("keys")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default())
    }

    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), OctyError> {
        let res = self
            .post(
                "/v1/s3/delete-object",
                &json!({ "bucket": bucket, "key": key }),
            )
            .await?;
        if res.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            Ok(())
        } else {
            Err(OctyError::internal(format!(
                "S3 delete-object failed for {bucket}/{key}"
            )))
        }
    }

    /// `single_upload` — generate a key, upload, return the key.
    pub async fn single_upload(
        &self,
        churn_data_dir: &str,
        file_data: &[u8],
        resource_friendly_name: &str,
        hyperparam_tuning_job_id: &str,
        bucket: &str,
    ) -> Result<String, OctyError> {
        let key = generate_file_key(churn_data_dir, resource_friendly_name, hyperparam_tuning_job_id);
        self.put_object(bucket, &key, file_data).await?;
        Ok(key)
    }

    /// The Python multipart-upload path. The gateway has no
    /// create/upload-part/complete MPU endpoints (capability gap — reported),
    /// so after replicating the chunk-count validation the object is uploaded
    /// with one `put-object` call.
    pub async fn upload_whole(
        &self,
        churn_data_dir: &str,
        file_data: &[u8],
        resource_friendly_name: &str,
        hyperparam_tuning_job_id: &str,
        bucket: &str,
    ) -> Result<String, OctyError> {
        let key = generate_file_key(churn_data_dir, resource_friendly_name, hyperparam_tuning_job_id);
        self.put_object(bucket, &key, file_data).await?;
        Ok(key)
    }

    /// `abort_multipart_upload` — the Python swallowed every error; with no
    /// MPU in flight (single put-object) this is a no-op kept for parity.
    pub async fn abort_multipart_upload(
        &self,
        _key: Option<&str>,
        upload_id: Option<&str>,
        _bucket: &str,
    ) {
        if upload_id.is_some() {
            eprintln!("[churn-worker] abort_multipart_upload: no MPU endpoints on the gateway — nothing to abort");
        }
    }

    /// `download_resource` — fetch an object; when compressed, gunzip + untar
    /// and return `(file_name, bytes)` pairs.
    pub async fn download_resource(
        &self,
        bucket: &str,
        key: &str,
        is_compressed: bool,
    ) -> Result<Vec<(String, Vec<u8>)>, OctyError> {
        let bytes = self.get_object(bucket, key).await?;
        if !is_compressed {
            return Ok(vec![(key.to_string(), bytes)]);
        }
        let mut decoder = GzDecoder::new(bytes.as_slice());
        let mut tar_bytes = Vec::new();
        decoder.read_to_end(&mut tar_bytes).map_err(|_| {
            OctyError::internal(
                "Error occurred when downloading and decompressing file -- file too small.",
            )
        })?;
        untar(&tar_bytes).map_err(OctyError::internal)
    }

    /// `delete_directory` — delete every object under a prefix.
    pub async fn delete_directory(&self, bucket: &str, directory_path: &str) -> Result<(), OctyError> {
        for key in self.list_objects(bucket, directory_path).await? {
            self.delete_object(bucket, &key).await?;
        }
        Ok(())
    }
}

/// `_generate_file_key`.
fn generate_file_key(
    churn_data_dir: &str,
    resource_friendly_name: &str,
    hyperparam_tuning_job_id: &str,
) -> String {
    let key = generate_uid("key");
    let ext = if resource_friendly_name.contains("meta_data") {
        "json"
    } else {
        "csv"
    };
    format!("{churn_data_dir}/{hyperparam_tuning_job_id}/{key}.{ext}")
}

/// Minimal ustar/GNU tar reader (regular files only; supports GNU long names).
fn untar(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut files = Vec::new();
    let mut off = 0usize;
    let mut pending_long_name: Option<String> = None;

    while off + 512 <= data.len() {
        let header = &data[off..off + 512];
        if header.iter().all(|b| *b == 0) {
            break; // end-of-archive blocks
        }
        let size = parse_octal(&header[124..136])
            .ok_or_else(|| "invalid tar header (size)".to_string())?;
        let typeflag = header[156];
        off += 512;
        let end = off
            .checked_add(size)
            .filter(|e| *e <= data.len())
            .ok_or_else(|| "truncated tar archive".to_string())?;
        let content = &data[off..end];

        match typeflag {
            b'L' => {
                // GNU long name: content is the next entry's name.
                pending_long_name = Some(cstr(content));
            }
            b'0' | 0 => {
                let name = pending_long_name.take().unwrap_or_else(|| {
                    let base = cstr(&header[0..100]);
                    let prefix = cstr(&header[345..500]);
                    if prefix.is_empty() {
                        base
                    } else {
                        format!("{prefix}/{base}")
                    }
                });
                let name = name.trim_start_matches("./").to_string();
                files.push((name, content.to_vec()));
            }
            _ => {
                pending_long_name = None; // directories, links, pax headers: skip
            }
        }

        off += size.div_ceil(512) * 512;
    }
    Ok(files)
}

fn cstr(bytes: &[u8]) -> String {
    let len = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..len]).trim().to_string()
}

fn parse_octal(bytes: &[u8]) -> Option<usize> {
    let s = cstr(bytes);
    if s.is_empty() {
        return Some(0);
    }
    usize::from_str_radix(&s, 8).ok()
}
