//! Mongo side of `data/repositories/implementation/churn_repository.py`
//! (hyper-parameter tuning job refs + the training dataset cache), ported
//! onto `octy-data-gateway`'s `/v1/mongo/{collection}/*` endpoints.
//!
//! Collections keep the mongoengine `Document` class names as-is
//! (`data/models/db_schemas.py` never overrides `meta['collection']`, and
//! mongoengine's default is the class name unchanged when it contains no
//! uppercase letters): `tbl_hparam_tuning_jobs`, `tbl_training_dataset_cache`.
//!
//! `find` with a sort spec and `insert-many` aren't wrapped by
//! `octy_spin::gateway::GatewayClient`, but both routes exist on the gateway
//! (`gateway/octy-data-gateway/src/main.rs`: `/v1/mongo/:collection/find`
//! accepts a `sort` field, and `/v1/mongo/:collection/insert-many` exists) —
//! called here directly over HTTP.

use octy_shared::ejson::now_legacy_date;
use octy_shared::errors::OctyError;
use octy_spin::ctx::Ctx;
use serde_json::{json, Value};
use spin_sdk::http::Method;

const HPARAM_JOBS: &str = "tbl_hparam_tuning_jobs";
const DATASET_CACHE: &str = "tbl_training_dataset_cache";

async fn gateway_post(ctx: &Ctx, path: &str, body: &Value) -> Result<Value, OctyError> {
    let base = octy_spin::ctx::variable("gateway_url", "GATEWAY_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string());
    let _ = &ctx.gateway; // keep Ctx borrowed for symmetry with the wrapped calls
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let (status, response) = octy_spin::gateway::http_send(
        Method::Post,
        &url,
        &[("content-type", "application/json")],
        Some(serde_json::to_vec(body).expect("serializable json")),
    )
    .await?;
    let parsed: Value = serde_json::from_slice(&response).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        let detail = parsed
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("gateway error");
        return Err(OctyError::internal(format!(
            "gateway {path} returned {status}: {detail}"
        )));
    }
    Ok(parsed)
}

/// `create_hparam_tuning_job_ref`.
pub async fn create_hparam_tuning_job_ref(
    ctx: &Ctx,
    hyperparam_tuning_job_id: &str,
    account_id: &str,
    meta_data: &Value,
) -> Result<(), OctyError> {
    let doc = json!({
        "hyperparam_tuning_job_id": hyperparam_tuning_job_id,
        "account_id": account_id,
        "meta_data": meta_data,
        "status": "in_progress",
        "created_at": now_legacy_date(),
        "updated_at": now_legacy_date(),
    });
    ctx.gateway.insert_one(HPARAM_JOBS, doc).await?;
    Ok(())
}

/// `get_hparam_tuning_job_ref` — `.objects.get(...)` raises `DoesNotExist`
/// when there's no match; surfaced here as an error (callers treat a missing
/// ref as a hard failure, same as the Python).
pub async fn get_hparam_tuning_job_ref(
    ctx: &Ctx,
    account_id: &str,
    hyperparam_tuning_job_id: &str,
    status: &str,
) -> Result<Value, OctyError> {
    let filter = json!({
        "account_id": account_id,
        "hyperparam_tuning_job_id": hyperparam_tuning_job_id,
        "status": status,
    });
    ctx.gateway
        .find_one(HPARAM_JOBS, filter)
        .await?
        .ok_or_else(|| {
            OctyError::internal(format!(
                "tbl_hparam_tuning_jobs matching account_id={account_id} \
                 hyperparam_tuning_job_id={hyperparam_tuning_job_id} status={status} does not exist"
            ))
        })
}

/// `get_parent_hparam_tuning_job_ref` — latest `Completed` ref for the
/// account, or `None` (the Python swallowed both "not found" and any other
/// error into `None`).
pub async fn get_parent_hparam_tuning_job_ref(ctx: &Ctx, account_id: &str) -> Option<Value> {
    let body = json!({
        "filter": { "account_id": account_id, "status": "Completed" },
        "sort": [["updated_at", -1]],
        "limit": 1,
    });
    let res = gateway_post(ctx, &format!("/v1/mongo/{HPARAM_JOBS}/find"), &body)
        .await
        .ok()?;
    res.get("documents")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .cloned()
}

/// `update_hparam_tuning_job_ref`.
///
/// PYTHON BUG (preserved): `_destroy_job` calls this without a `model_meta`
/// argument, but the method (and its ABC) declare `model_meta` as a required
/// positional parameter with no default — that call raises
/// `TypeError: missing 1 required positional argument: 'model_meta'` in the
/// original service. The exception propagates to `_destroy_job`'s own
/// `try/except`, so the tuning-job-ref status update, the
/// `octy.job.cmd.delete` publish and the failure webhook in `_destroy_job`
/// never run — only the preceding `delete_directory` call takes effect. This
/// port reproduces that: `update_status_only` (used by `_destroy_job`)
/// deliberately returns the same failure instead of silently "fixing" the
/// bug by defaulting `model_meta`.
pub async fn update_hparam_tuning_job_ref(
    ctx: &Ctx,
    account_id: &str,
    hyperparam_tuning_job_id: &str,
    best_model_training_job_id: &str,
    status: &str,
    model_meta: Option<&Value>,
) -> Result<(), OctyError> {
    let filter = json!({
        "account_id": account_id,
        "hyperparam_tuning_job_id": hyperparam_tuning_job_id,
    });
    let set = match model_meta {
        Some(meta) => json!({
            "status": status,
            "best_model_meta_data": meta,
            "best_model_training_job_id": best_model_training_job_id,
            "updated_at": now_legacy_date(),
        }),
        None => json!({
            "status": status,
            "updated_at": now_legacy_date(),
        }),
    };
    ctx.gateway
        .update_one(HPARAM_JOBS, filter, json!({ "$set": set }))
        .await
}

/// See the PYTHON BUG note on [`update_hparam_tuning_job_ref`] — this is the
/// call `_destroy_job` actually makes (no `model_meta`), which crashed with a
/// `TypeError` before reaching Mongo. Faithfully reproduced as an immediate
/// error rather than silently supplying a default.
pub fn destroy_job_update_status_call_would_crash() -> OctyError {
    OctyError::internal(
        "update_hparam_tuning_job_ref() missing 1 required positional argument: 'model_meta' \
         (bug preserved from services/churn_prediction.py::_destroy_job)",
    )
}

/// `delete_hparam_tuning_job_ref`.
pub async fn delete_hparam_tuning_job_ref(
    ctx: &Ctx,
    account_id: &str,
    hyperparam_tuning_job_id: &str,
) -> Result<(), OctyError> {
    ctx.gateway
        .delete_one(
            HPARAM_JOBS,
            json!({ "account_id": account_id, "hyperparam_tuning_job_id": hyperparam_tuning_job_id }),
        )
        .await
}

/// `cache_dataset` — bulk-inserts every row whose `churn` field is falsy
/// (profiles that have not yet churned). Uses `/v1/mongo/.../insert-many`
/// directly (unordered, matching `initialize_unordered_bulk_op`).
pub async fn cache_dataset(
    ctx: &Ctx,
    account_id: &str,
    hyperparam_tuning_job_id: &str,
    dataset: &Value,
) -> Result<(), OctyError> {
    let Some(rows) = dataset.as_object() else {
        return Err(OctyError::internal("cache_dataset: dataset is not an object"));
    };
    let documents: Vec<Value> = rows
        .values()
        .filter(|row| !row.get("churn").and_then(Value::as_bool).unwrap_or(false))
        .map(|row| {
            json!({
                "account_id": account_id,
                "hyperparam_tuning_job_id": hyperparam_tuning_job_id,
                "row_data": row,
            })
        })
        .collect();
    if documents.is_empty() {
        return Ok(());
    }
    gateway_post(
        ctx,
        &format!("/v1/mongo/{DATASET_CACHE}/insert-many"),
        &json!({ "documents": documents, "ordered": false }),
    )
    .await?;
    Ok(())
}

/// `get_cached_dataset` — the list of `row_data` documents.
pub async fn get_cached_dataset(
    ctx: &Ctx,
    account_id: &str,
    hyperparam_tuning_job_id: &str,
) -> Result<Vec<Value>, OctyError> {
    let filter = json!({
        "account_id": account_id,
        "hyperparam_tuning_job_id": hyperparam_tuning_job_id,
    });
    let docs = ctx.gateway.find(DATASET_CACHE, filter, 0, 0).await?;
    Ok(docs
        .into_iter()
        .map(|d| d.get("row_data").cloned().unwrap_or(Value::Null))
        .collect())
}

/// `delete_cached_dataset` — `.objects(...).delete()` removes every matching
/// row, so this uses `delete-many` (the wrapped `GatewayClient::delete_one`
/// would silently leave rows behind whenever a dataset has more than one
/// cached document).
pub async fn delete_cached_dataset(
    ctx: &Ctx,
    account_id: &str,
    hyperparam_tuning_job_id: &str,
) -> Result<(), OctyError> {
    gateway_post(
        ctx,
        &format!("/v1/mongo/{DATASET_CACHE}/delete-many"),
        &json!({
            "filter": {
                "account_id": account_id,
                "hyperparam_tuning_job_id": hyperparam_tuning_job_id,
            }
        }),
    )
    .await?;
    Ok(())
}
