//! Port of `data/repositories/implementation/rfm_repository.py`.
//!
//! The event/profile/item service calls are plain outbound HTTPS (not
//! gateway-mediated, same as the account service's direct Mailjet calls);
//! Mongo access (`tbl_training_jobs`) and SageMaker go through the data
//! gateway / SigV4-signed AWS calls respectively.

use octy_shared::ejson;
use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::http::Method;

use octy_spin::ctx::Ctx;
use octy_spin::gateway::http_send;

use crate::sagemaker::{self, TrainingResource};

const COLLECTION: &str = "tbl_training_jobs";

async fn paginated_post(url: &str, payload: &Value) -> Result<Vec<Value>, OctyError> {
    let mut out = Vec::new();
    let mut cursor: i64 = 0;
    loop {
        let (status, body) = http_send(
            Method::Post,
            url,
            &[("content-type", "application/json"), ("cursor", &cursor.to_string())],
            Some(serde_json::to_vec(payload).expect("serializable json")),
        )
        .await?;
        if status != 200 {
            break;
        }
        let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        out.push(parsed);
        let count = out
            .last()
            .and_then(|v| v.get("request_meta"))
            .and_then(|m| m.get("count"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if count == 0 {
            break;
        }
        cursor += count;
    }
    Ok(out)
}

async fn paginated_get(url_base: &str) -> Result<Vec<Value>, OctyError> {
    let mut out = Vec::new();
    let mut cursor: i64 = 0;
    loop {
        let (status, body) = http_send(
            Method::Get,
            url_base,
            &[("cursor", &cursor.to_string())],
            None,
        )
        .await?;
        if status != 200 {
            break;
        }
        let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        out.push(parsed);
        let count = out
            .last()
            .and_then(|v| v.get("request_meta"))
            .and_then(|m| m.get("count"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if count == 0 {
            break;
        }
        cursor += count;
    }
    Ok(out)
}

/// `get_events(account_id, profile_ids, timeframe, event_type)`.
pub async fn get_events(
    ctx: &Ctx,
    account_id: &str,
    profile_ids: &[String],
    timeframe: i64,
    event_type: &str,
) -> Result<Vec<Value>, OctyError> {
    let url = format!("{}/v1/internal/events", ctx.config.get_str("EVENT_SERVICE_CLUSTER_IP")?);
    let payload = json!({
        "timeframe": timeframe,
        "account_id": account_id,
        "profile_ids": profile_ids,
        "event_type": event_type,
    });
    let pages = paginated_post(&url, &payload).await?;
    let mut events = Vec::new();
    for page in pages {
        if let Some(arr) = page.get("events").and_then(Value::as_array) {
            events.extend(arr.iter().cloned());
        }
    }
    Ok(events)
}

/// `get_profiles(account_id, status='active', ids='false')` — this worker
/// only ever calls it with `ids='true'`, so we return the id list directly.
pub async fn get_profile_ids(ctx: &Ctx, account_id: &str) -> Result<Vec<String>, OctyError> {
    let url = format!(
        "{}/v1/internal/profiles?ids=true&status=active",
        ctx.config.get_str("PROFILE_SERVICE_CLUSTER_IP")?
    );
    let payload = json!({ "account_id": account_id, "profiles": [], "get_all": true });
    let pages = paginated_post(&url, &payload).await?;
    let mut ids = Vec::new();
    for page in pages {
        if let Some(arr) = page.get("profiles").and_then(Value::as_array) {
            for p in arr {
                if let Some(id) = p.get("profile_id").and_then(Value::as_str) {
                    ids.push(id.to_string());
                }
            }
        }
    }
    Ok(ids)
}

/// `get_items(account_id, ids='false')` — items with full bodies (need
/// `item_price` for the training dataset).
pub async fn get_items(ctx: &Ctx, account_id: &str) -> Result<Vec<Value>, OctyError> {
    let url = format!(
        "{}/v1/internal/items?account_id={account_id}&ids=false&status=all",
        ctx.config.get_str("ITEM_SERVICE_CLUSTER_IP")?
    );
    let pages = paginated_get(&url).await?;
    let mut items = Vec::new();
    for page in pages {
        if let Some(arr) = page.get("items").and_then(Value::as_array) {
            items.extend(arr.iter().cloned());
        }
    }
    Ok(items)
}

/// `create_training_job_ref`.
pub async fn create_training_job_ref(ctx: &Ctx, training_job_id: &str, account_id: &str) -> Result<(), OctyError> {
    let now = ejson::now_legacy_date();
    let doc = json!({
        "training_job_id": training_job_id,
        "account_id": account_id,
        "status": "in_progress",
        "created_at": now,
        "updated_at": now,
    });
    ctx.gateway.insert_one(COLLECTION, doc).await?;
    Ok(())
}

/// `get_training_job(account_id, training_job_id, status='in_progress')`.
pub async fn get_training_job(ctx: &Ctx, account_id: &str, training_job_id: &str, status: &str) -> Result<Value, OctyError> {
    let filter = json!({ "account_id": account_id, "training_job_id": training_job_id, "status": status });
    ctx.gateway
        .find_one(COLLECTION, filter)
        .await?
        .ok_or_else(|| OctyError::internal(format!("training job not found: {training_job_id}")))
}

/// `delete_training_job_ref`.
pub async fn delete_training_job_ref(ctx: &Ctx, account_id: &str, training_job_id: &str) -> Result<(), OctyError> {
    let filter = json!({ "account_id": account_id, "training_job_id": training_job_id });
    ctx.gateway.delete_one(COLLECTION, filter).await
}

/// `update_training_job_ref(account_id, training_job_id, status)`.
pub async fn update_training_job_ref(ctx: &Ctx, account_id: &str, training_job_id: &str, status: &str) -> Result<(), OctyError> {
    let filter = json!({ "account_id": account_id, "training_job_id": training_job_id });
    let update = json!({ "$set": { "status": status, "updated_at": ejson::now_legacy_date() } });
    ctx.gateway.update_one(COLLECTION, filter, update).await
}

/// `start_cloud_training`.
pub async fn start_cloud_training(
    ctx: &Ctx,
    account_id: &str,
    training_job_id: &str,
    volume_size: i64,
    training_resources: &[TrainingResource],
    bucket_name: &str,
) -> Result<(), OctyError> {
    sagemaker::create_training_job(ctx, account_id, training_job_id, volume_size, training_resources, bucket_name).await
}

/// `get_cloud_training_status_time`.
pub async fn get_cloud_training_status_time(ctx: &Ctx, training_job_id: &str) -> Result<(String, f64), OctyError> {
    sagemaker::describe_training_job(ctx, training_job_id).await
}
