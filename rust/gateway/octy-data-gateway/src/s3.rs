//! Port of `data/repositories/implementation/bucket_repository.py` (S3 side).
//! Booleans in the responses mirror the Python methods' return values —
//! failures are logged, not raised.

use axum::extract::State;
use axum::Json;
use base64::Engine;
use aws_sdk_s3::types::{
    BucketLocationConstraint, CorsConfiguration, CorsRule, CreateBucketConfiguration, Tag, Tagging,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::SharedState;

pub struct S3Buckets {
    client: Option<aws_sdk_s3::Client>,
    region: String,
}

impl S3Buckets {
    /// Standard AWS by default. For local development against a MinIO (or
    /// other S3-compatible) instance, set `AWS_ENDPOINT_URL` (respected by
    /// `aws-config`'s standard env resolution) and `S3_FORCE_PATH_STYLE=true`
    /// — MinIO needs path-style addressing (`endpoint/bucket/key`) since it
    /// doesn't do virtual-hosted-style DNS routing. Neither variable changes
    /// behavior when unset, so this has no effect on the real AWS path.
    pub async fn from_env() -> Self {
        let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
        let force_path_style = std::env::var("S3_FORCE_PATH_STYLE")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        match std::env::var("AWS_ACCESS_KEY_ID") {
            Ok(_) => {
                let config = aws_config::load_from_env().await;
                let mut s3_config = aws_sdk_s3::config::Builder::from(&config);
                if force_path_style {
                    s3_config = s3_config.force_path_style(true);
                }
                Self {
                    client: Some(aws_sdk_s3::Client::from_conf(s3_config.build())),
                    region,
                }
            }
            Err(_) => {
                tracing::warn!("AWS credentials not set — S3 operations disabled");
                Self { client: None, region }
            }
        }
    }

    fn client(&self) -> Option<&aws_sdk_s3::Client> {
        self.client.as_ref()
    }
}

#[derive(Deserialize)]
pub struct BucketBody {
    bucket: String,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize)]
pub struct ObjectBody {
    bucket: String,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    prefix: Option<String>,
    /// base64-encoded object body (for put-object)
    #[serde(default)]
    body_base64: Option<String>,
    #[serde(default)]
    content_type: Option<String>,
}

fn ok(result: bool) -> Json<Value> {
    Json(json!({ "ok": result }))
}

pub async fn create_bucket(State(state): State<SharedState>, Json(body): Json<BucketBody>) -> Json<Value> {
    let Some(client) = state.s3.client() else { return ok(false) };
    let constraint = BucketLocationConstraint::from(state.s3.region.as_str());
    let result = client
        .create_bucket()
        .bucket(&body.bucket)
        .create_bucket_configuration(
            CreateBucketConfiguration::builder()
                .location_constraint(constraint)
                .build(),
        )
        .send()
        .await;
    if let Err(e) = &result {
        tracing::error!("create_bucket {} failed: {e}", body.bucket);
    }
    ok(result.is_ok())
}

pub async fn configure_bucket(State(state): State<SharedState>, Json(body): Json<BucketBody>) -> Json<Value> {
    let Some(client) = state.s3.client() else { return ok(false) };
    let bucket = &body.bucket;
    let account_id = body.account_id.clone().unwrap_or_default();

    let result: Result<(), aws_sdk_s3::Error> = async {
        client
            .delete_public_access_block()
            .bucket(bucket)
            .send()
            .await?;

        let bucket_policy = json!({
            "Version": "2012-10-17",
            "Id": "S3PolicyIPRestrict",
            "Statement": [{
                "Sid": "IPAllow",
                "Effect": "Allow",
                "Principal": { "AWS": "*" },
                "Action": "s3:*",
                "Resource": format!("arn:aws:s3:::{bucket}/*"),
            }]
        });
        client
            .put_bucket_policy()
            .bucket(bucket)
            .policy(bucket_policy.to_string())
            .send()
            .await?;

        let cors_rule = CorsRule::builder()
            .allowed_headers("*")
            .allowed_headers("Access-Control-Expose-Headers")
            .allowed_methods("GET")
            .allowed_methods("POST")
            .allowed_methods("PUT")
            .allowed_origins("*")
            .expose_headers("GET")
            .expose_headers("PUT")
            .expose_headers("ETag")
            .max_age_seconds(3000)
            .build()
            .map_err(aws_sdk_s3::Error::from)?;
        client
            .put_bucket_cors()
            .bucket(bucket)
            .cors_configuration(
                CorsConfiguration::builder()
                    .cors_rules(cors_rule)
                    .build()
                    .map_err(aws_sdk_s3::Error::from)?,
            )
            .send()
            .await?;

        client
            .put_bucket_tagging()
            .bucket(bucket)
            .tagging(
                Tagging::builder()
                    .tag_set(
                        Tag::builder()
                            .key("octy_account_id")
                            .value(&account_id)
                            .build()
                            .map_err(aws_sdk_s3::Error::from)?,
                    )
                    .build()
                    .map_err(aws_sdk_s3::Error::from)?,
            )
            .send()
            .await?;
        Ok(())
    }
    .await;

    if let Err(e) = &result {
        tracing::error!("configure_bucket {bucket} failed: {e}");
    }
    ok(result.is_ok())
}

pub async fn create_directory(State(state): State<SharedState>, Json(body): Json<BucketBody>) -> Json<Value> {
    let Some(client) = state.s3.client() else { return ok(false) };
    let path = body.path.clone().unwrap_or_default();
    let result = client
        .put_object()
        .bucket(&body.bucket)
        .key(format!("{path}/"))
        .send()
        .await;
    if let Err(e) = &result {
        tracing::error!("create_directory {}/{path} failed: {e}", body.bucket);
    }
    ok(result.is_ok())
}

/// ---- Object operations (used by messaging templates + the ML workers) ----

pub async fn put_object(State(state): State<SharedState>, Json(body): Json<ObjectBody>) -> Json<Value> {
    let Some(client) = state.s3.client() else { return ok(false) };
    let key = body.key.clone().unwrap_or_default();
    let bytes = match body
        .body_base64
        .as_deref()
        .map(|b64| base64::engine::general_purpose::STANDARD.decode(b64))
        .transpose()
    {
        Ok(bytes) => bytes.unwrap_or_default(),
        Err(_) => return Json(json!({ "ok": false, "error": "invalid base64 body" })),
    };
    let mut request = client
        .put_object()
        .bucket(&body.bucket)
        .key(&key)
        .body(bytes.into());
    if let Some(content_type) = &body.content_type {
        request = request.content_type(content_type);
    }
    let result = request.send().await;
    if let Err(e) = &result {
        tracing::error!("put_object {}/{key} failed: {e}", body.bucket);
    }
    ok(result.is_ok())
}

pub async fn get_object(State(state): State<SharedState>, Json(body): Json<ObjectBody>) -> Json<Value> {
    let Some(client) = state.s3.client() else { return Json(json!({ "found": false })) };
    let key = body.key.clone().unwrap_or_default();
    match client.get_object().bucket(&body.bucket).key(&key).send().await {
        Ok(output) => match output.body.collect().await {
            Ok(data) => Json(json!({
                "found": true,
                "body_base64": base64::engine::general_purpose::STANDARD.encode(data.into_bytes()),
            })),
            Err(e) => {
                tracing::error!("get_object {}/{key} read failed: {e}", body.bucket);
                Json(json!({ "found": false }))
            }
        },
        Err(e) => {
            tracing::warn!("get_object {}/{key}: {e}", body.bucket);
            Json(json!({ "found": false }))
        }
    }
}

pub async fn list_objects(State(state): State<SharedState>, Json(body): Json<ObjectBody>) -> Json<Value> {
    let Some(client) = state.s3.client() else { return Json(json!({ "keys": [] })) };
    let mut keys: Vec<String> = Vec::new();
    let mut continuation: Option<String> = None;
    loop {
        let mut request = client.list_objects_v2().bucket(&body.bucket);
        if let Some(prefix) = &body.prefix {
            request = request.prefix(prefix);
        }
        if let Some(token) = &continuation {
            request = request.continuation_token(token);
        }
        match request.send().await {
            Ok(page) => {
                keys.extend(page.contents().iter().filter_map(|o| o.key().map(String::from)));
                match page.next_continuation_token() {
                    Some(token) => continuation = Some(token.to_string()),
                    None => break,
                }
            }
            Err(e) => {
                tracing::error!("list_objects {} failed: {e}", body.bucket);
                break;
            }
        }
    }
    Json(json!({ "keys": keys }))
}

pub async fn delete_object(State(state): State<SharedState>, Json(body): Json<ObjectBody>) -> Json<Value> {
    let Some(client) = state.s3.client() else { return ok(false) };
    let key = body.key.clone().unwrap_or_default();
    let result = client.delete_object().bucket(&body.bucket).key(&key).send().await;
    if let Err(e) = &result {
        tracing::error!("delete_object {}/{key} failed: {e}", body.bucket);
    }
    ok(result.is_ok())
}

pub async fn delete_bucket(State(state): State<SharedState>, Json(body): Json<BucketBody>) -> Json<Value> {
    let Some(client) = state.s3.client() else { return ok(false) };
    let bucket = &body.bucket;

    let result: Result<(), aws_sdk_s3::Error> = async {
        // Empty the bucket first (objects.all().delete() in boto3).
        let mut continuation: Option<String> = None;
        loop {
            let mut request = client.list_objects_v2().bucket(bucket);
            if let Some(token) = &continuation {
                request = request.continuation_token(token);
            }
            let page = request.send().await?;
            for object in page.contents() {
                if let Some(key) = object.key() {
                    client.delete_object().bucket(bucket).key(key).send().await?;
                }
            }
            match page.next_continuation_token() {
                Some(token) => continuation = Some(token.to_string()),
                None => break,
            }
        }
        client.delete_bucket().bucket(bucket).send().await?;
        Ok(())
    }
    .await;

    if let Err(e) = &result {
        tracing::error!("delete_bucket {bucket} failed: {e}");
    }
    ok(result.is_ok())
}
