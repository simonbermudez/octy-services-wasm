//! HTTP client for the `octy-data-gateway` sidecar — the bridge to MongoDB,
//! RabbitMQ and S3 (which need raw TCP / native SDKs unavailable in WASM).
//! Also hosts the generic outbound-HTTP helper used for Mailjet and the
//! internal cleanup fan-out.

use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::http::{Method, Request, Response};

pub async fn http_send(
    method: Method,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<Vec<u8>>,
) -> Result<(u16, Vec<u8>), OctyError> {
    let mut builder = Request::builder();
    builder.method(method).uri(url);
    for (name, value) in headers {
        builder.header(*name, *value);
    }
    let request = builder.body(body.unwrap_or_default()).build();
    let response: Response = spin_sdk::http::send(request)
        .await
        .map_err(|e| OctyError::internal(format!("outbound http to {url} failed: {e}")))?;
    let status = *response.status();
    Ok((status, response.into_body()))
}

/// Port of `requests_retry_session` (retry on 500/502/504, 4 retries). WASI
/// has no timer host call in this component, so retries are immediate.
pub async fn http_post_json_with_retry(
    url: &str,
    headers: &[(&str, &str)],
    body: &Value,
) -> Result<(u16, Vec<u8>), OctyError> {
    let payload = serde_json::to_vec(body).expect("serializable json");
    let mut all_headers = vec![("content-type", "application/json")];
    all_headers.extend_from_slice(headers);

    let mut last_err = OctyError::internal(format!("request to {url} failed"));
    for _attempt in 0..=4 {
        match http_send(Method::Post, url, &all_headers, Some(payload.clone())).await {
            Ok((status, response_body)) if !matches!(status, 500 | 502 | 504) => {
                return Ok((status, response_body));
            }
            Ok((status, _)) => {
                last_err = OctyError::internal(format!("{url} returned status {status}"));
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

pub struct GatewayClient {
    base: String,
}

impl GatewayClient {
    pub fn new(base: String) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
        }
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value, OctyError> {
        let url = format!("{}{}", self.base, path);
        let (status, response_body) = http_send(
            Method::Post,
            &url,
            &[("content-type", "application/json")],
            Some(serde_json::to_vec(body).expect("serializable json")),
        )
        .await?;

        let parsed: Value = serde_json::from_slice(&response_body).unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            return Ok(parsed);
        }
        let detail = parsed
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("gateway error")
            .to_string();
        // 409 = Mongo duplicate key, surfaced like the Python 'Duplicate entry'.
        if status == 409 {
            return Err(OctyError::new(
                400,
                "Duplicate entry",
                vec![octy_shared::errors::ErrorReason::new(detail, "")],
            ));
        }
        Err(OctyError::internal(format!("gateway {path}: {detail}")))
    }

    // ---- MongoDB (documents travel as legacy extended JSON) ----

    pub async fn find_one(&self, collection: &str, filter: Value) -> Result<Option<Value>, OctyError> {
        let res = self
            .post(&format!("/v1/mongo/{collection}/find-one"), &json!({ "filter": filter }))
            .await?;
        match res.get("document") {
            Some(Value::Null) | None => Ok(None),
            Some(doc) => Ok(Some(doc.clone())),
        }
    }

    pub async fn find(
        &self,
        collection: &str,
        filter: Value,
        skip: i64,
        limit: i64,
    ) -> Result<Vec<Value>, OctyError> {
        let res = self
            .post(
                &format!("/v1/mongo/{collection}/find"),
                &json!({ "filter": filter, "skip": skip, "limit": limit }),
            )
            .await?;
        Ok(res
            .get("documents")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    pub async fn count(&self, collection: &str, filter: Value) -> Result<i64, OctyError> {
        let res = self
            .post(&format!("/v1/mongo/{collection}/count"), &json!({ "filter": filter }))
            .await?;
        Ok(res.get("count").and_then(Value::as_i64).unwrap_or(0))
    }

    /// Returns the inserted `_id` (legacy extended JSON `{"$oid": …}`).
    pub async fn insert_one(&self, collection: &str, document: Value) -> Result<Value, OctyError> {
        let res = self
            .post(&format!("/v1/mongo/{collection}/insert-one"), &json!({ "document": document }))
            .await?;
        Ok(res.get("inserted_id").cloned().unwrap_or(Value::Null))
    }

    pub async fn update_one(
        &self,
        collection: &str,
        filter: Value,
        update: Value,
    ) -> Result<(), OctyError> {
        self.post(
            &format!("/v1/mongo/{collection}/update-one"),
            &json!({ "filter": filter, "update": update }),
        )
        .await?;
        Ok(())
    }

    pub async fn delete_one(&self, collection: &str, filter: Value) -> Result<(), OctyError> {
        self.post(&format!("/v1/mongo/{collection}/delete-one"), &json!({ "filter": filter }))
            .await?;
        Ok(())
    }

    // ---- RabbitMQ ----

    pub async fn amqp_publish(&self, routing_key: &str, payload: &Value) -> Result<(), OctyError> {
        self.post(
            "/v1/amqp/publish",
            &json!({ "routing_key": routing_key, "payload": payload }),
        )
        .await?;
        Ok(())
    }

    // ---- S3 (port of BucketRepository; booleans mirror the Python API) ----

    async fn s3_ok(&self, path: &str, body: Value) -> bool {
        match self.post(path, &body).await {
            Ok(res) => res.get("ok").and_then(Value::as_bool).unwrap_or(false),
            Err(_) => false,
        }
    }

    pub async fn create_bucket(&self, bucket: &str) -> bool {
        self.s3_ok("/v1/s3/create-bucket", json!({ "bucket": bucket })).await
    }

    pub async fn configure_bucket(&self, bucket: &str, account_id: &str) -> bool {
        self.s3_ok(
            "/v1/s3/configure-bucket",
            json!({ "bucket": bucket, "account_id": account_id }),
        )
        .await
    }

    pub async fn create_directory(&self, bucket: &str, path: &str) -> bool {
        self.s3_ok("/v1/s3/create-directory", json!({ "bucket": bucket, "path": path })).await
    }

    pub async fn delete_bucket(&self, bucket: &str) -> bool {
        self.s3_ok("/v1/s3/delete-bucket", json!({ "bucket": bucket })).await
    }
}
