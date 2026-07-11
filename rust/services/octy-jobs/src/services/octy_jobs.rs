//! Port of `services/octy_jobs.py` — `OctyJobQueueService` plus the
//! `OctyJobQueue` periodic processor.
//!
//! The Python service ran `OctyJobQueue._process_octy_jobs` on an in-process
//! scheduler every `queue_process_interval` (2) minutes. A per-request WASM
//! component has no background loop, so that logic is exposed as one
//! scheduling pass behind `POST /internal/scheduler/tick`; a Kubernetes
//! CronJob (see `rust/kubernetes/octy-jobs/scheduler-cronjob.yml`) curls it on
//! the original 2-minute interval. The `is_processing` re-entrancy guard is
//! replaced by the CronJob's `concurrencyPolicy: Forbid`.

use chrono::Utc;
use octy_shared::ejson::{date_millis, now_legacy_date};
use octy_shared::errors::OctyError;
use octy_shared::utils::generate_uid;
use serde_json::{json, Value};

use crate::models::{CreateOctyJob, OctyJobCallBack};
use crate::repos::octy_jobs as repo;
use crate::repos::octy_jobs::JobUpdate;
use octy_spin::ctx::Ctx;

// ---------------------------------------------------------------------------
// OctyJobQueueService
// ---------------------------------------------------------------------------

pub async fn create_new_job(ctx: &Ctx, octy_job: &CreateOctyJob) -> Result<(), OctyError> {
    let meta = &octy_job.job_meta;
    let new_octy_job = json!({
        "octy_job_id": generate_uid("octy-job"),
        "account_id": octy_job.account_id,
        "alt_dentifier": octy_job.alt_dentifier,
        "job_meta": {
            "required_permissions": meta.required_permissions,
            // The Python inserted the pydantic RequiredConfigs *instance* here
            // (unencodable by bson after the mongoengine -> motor rewrite);
            // this port stores the plain document the schema always intended.
            "required_configurations": {
                "account_attributes": meta.required_configurations.account_attributes,
                "algorithm_configuration_idxs": meta.required_configurations.algorithm_configuration_idxs,
            },
            "amqp_routing_key": meta.amqp_routing_key,
            "job_type": meta.job_type,
            "desired_runs": if meta.desired_runs != 0 { meta.desired_runs } else { 1_000_000_000_000i64 },
            "time_interval": meta.time_interval,
            "fail_threshold": if meta.fail_threshold != 0 { meta.fail_threshold } else { 1_000_000_000_000i64 },
            // tbl_octy_jobs defaults from the (commented-out) mongoengine
            // schema, restored — the motor rewrite dropped them, leaving new
            // documents unreadable by the queue tick.
            "successful_runs": 0,
            "failed_runs": 0,
            "last_run": Value::Null,
            "status": "pending",
            "created_at": now_legacy_date(),
            "updated_at": Value::Null,
            "last_updated_action": Value::Null,
        },
        "job_data": octy_job.job_data.clone().unwrap_or(Value::Null),
    });
    repo::create_octy_job(ctx, &octy_job.account_id, &new_octy_job).await
}

pub async fn delete_octy_jobs(
    ctx: &Ctx,
    account_id: &str,
    octy_job_ids: Vec<String>,
    alt_identifiers: Vec<String>,
) -> Result<(), OctyError> {
    // `octy_job_ids.extend(alt_identifiers)`
    let mut identifiers: Vec<Value> = octy_job_ids.into_iter().map(Value::String).collect();
    identifiers.extend(alt_identifiers.into_iter().map(Value::String));
    repo::delete_octy_jobs(ctx, &[json!(account_id)], &identifiers).await
}

/// Delete all octy jobs for an account, then notify via AMQP.
pub async fn delete_all_octy_jobs(ctx: &Ctx, account_id: &str) -> Result<bool, OctyError> {
    repo::delete_all_octy_jobs(ctx, account_id).await?;

    // Python quirk preserved: this publishes with the *queue name*
    // ('octy-job-delete-queue') as the routing key, not 'octy.job.cmd.delete'.
    ctx.gateway
        .amqp_publish("octy-job-delete-queue", &json!({ "account_id": account_id }))
        .await?;

    Ok(true)
}

pub async fn status_callback(ctx: &Ctx, cb: &OctyJobCallBack) -> Result<(), OctyError> {
    repo::update_octy_job(
        ctx,
        &[JobUpdate {
            account_id: json!(cb.account_id),
            octy_job_id: json!(cb.octy_job_id),
            suc_inc_by: if cb.status == "success" { 1 } else { 0 },
            fail_inc_by: if cb.status == "failed" { 1 } else { 0 },
            status: "pending".to_string(), // back to pending for the next tick
            action: "http callback --> updated job status".to_string(),
        }],
    )
    .await
}

// ---------------------------------------------------------------------------
// OctyJobQueue — one scheduling pass (`_process_octy_jobs`)
// ---------------------------------------------------------------------------

/// `functools.reduce`-style dotted lookup (`_deep_get`).
fn deep_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in key.split('.') {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

/// Python truthiness for the JSON types (numbers are handled separately).
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `isinstance(val, numbers.Number)` — bool is a Number in Python.
fn is_number(value: &Value) -> bool {
    value.is_number() || value.is_boolean()
}

/// `_validate_account_attr` → (valid, is_nested). The Python fell off the end
/// (returned `None`, crashing the tick) when a top-level key was missing;
/// here a missing attribute simply invalidates the job.
fn validate_account_attr(account: &Value, key: &str) -> (bool, bool) {
    let val = deep_get(account, key);
    let val_is_number = val.map(is_number).unwrap_or(false);
    if key.contains('.') {
        if val_is_number {
            return (true, true);
        }
        return (truthy(val.unwrap_or(&Value::Null)), true);
    }
    if val_is_number {
        return (true, false);
    }
    match account.get(key) {
        Some(v) => (truthy(v), false),
        None => (false, false),
    }
}

/// `_validate_algorithm_config` — out-of-range/missing entries are invalid
/// (the Python only caught KeyError; an IndexError crashed the tick).
fn validate_algorithm_config(account: &Value, idx: usize) -> bool {
    truthy(&account["algorithm_configurations"][idx]["config_json"])
}

/// `_build_message_payload` → (is_valid, payload).
fn build_message_payload(account: &Value, job: &Value) -> (bool, Value) {
    // Hard coded required payload attributes
    let mut payload = json!({
        "account_data": { "account_id": account["_id"] },
        "octy_job_id": job["_id"],
        "job_data": job["job_data"],
    });

    let empty: Vec<Value> = Vec::new();

    // Assess permissions
    let required_permissions = job["job_meta"]["required_permissions"].as_array().unwrap_or(&empty);
    let account_permissions = account["permissions"].as_array().unwrap_or(&empty);
    for permission in required_permissions {
        if !account_permissions.contains(permission) {
            return (false, json!({}));
        }
    }

    // Assess required account attributes & configurations
    let account_attributes = job["job_meta"]["required_configurations"]["account_attributes"]
        .as_array()
        .unwrap_or(&empty);
    for attr in account_attributes {
        let key = attr.as_str().unwrap_or("");
        let (valid, nested) = validate_account_attr(account, key);
        if !valid {
            return (false, json!({}));
        }
        if nested {
            let field = key.rsplit('.').next().unwrap_or(key).to_string();
            payload["account_data"][field] =
                deep_get(account, key).cloned().unwrap_or(Value::Null);
        } else {
            payload["account_data"][key] = account[key].clone();
        }
    }

    // Assess required algorithm configurations
    let idxs = job["job_meta"]["required_configurations"]["algorithm_configuration_idxs"]
        .as_array()
        .unwrap_or(&empty);
    for idx in idxs {
        let i = idx.as_u64().map(|v| v as usize).unwrap_or(usize::MAX);
        if !validate_algorithm_config(account, i) {
            return (false, json!({}));
        }
        payload["account_data"]["algorithm_configurations"] =
            account["algorithm_configurations"][i]["config_json"].clone();
    }

    (true, payload)
}

/// `_get_all_jobs` — page through the collection 1000 at a time.
async fn get_all_jobs(ctx: &Ctx) -> Result<Vec<Value>, OctyError> {
    let mut jobs: Vec<Value> = Vec::new();
    let mut cursor: i64 = 0;
    loop {
        let page = repo::get_octy_jobs(ctx, cursor).await?;
        let num_jobs = page.len() as i64;
        jobs.extend(page);
        if num_jobs < 1 {
            break;
        }
        cursor += num_jobs;
    }
    Ok(jobs)
}

/// `_reset_jobs` — hung "processing" jobs go back to "pending".
async fn reset_jobs(ctx: &Ctx, jobs: &[&Value]) -> Result<(), OctyError> {
    let updates: Vec<JobUpdate> = jobs
        .iter()
        .map(|job| JobUpdate {
            account_id: job["account_id"].clone(),
            octy_job_id: job["_id"].clone(),
            suc_inc_by: 0,
            fail_inc_by: 0,
            status: "pending".to_string(),
            action: "octy job queue --> Error occurred during processing :: job status hung as \"processing\" for more than 24 hours.".to_string(),
        })
        .collect();
    repo::update_octy_job(ctx, &updates).await
}

/// `_filter_pending_exceeded_jobs` → (runnable pending jobs, invalid jobs,
/// number of hung jobs reset).
async fn filter_pending_exceeded_jobs(
    ctx: &Ctx,
    jobs: &[Value],
) -> Result<(Vec<Value>, Vec<Value>, usize), OctyError> {
    let now_ms = Utc::now().timestamp_millis();

    let meta_i64 = |job: &Value, key: &str| job["job_meta"][key].as_i64().unwrap_or(0);
    let meta_status = |job: &Value| job["job_meta"]["status"].as_str().unwrap_or("").to_string();

    let pending_jobs: Vec<&Value> = jobs
        .iter()
        .filter(|j| meta_i64(j, "successful_runs") < meta_i64(j, "desired_runs") && meta_status(j) == "pending")
        .collect();
    let exceeded_jobs: Vec<&Value> = jobs
        .iter()
        .filter(|j| meta_i64(j, "successful_runs") >= meta_i64(j, "desired_runs"))
        .collect();
    let failed_jobs: Vec<&Value> = jobs
        .iter()
        .filter(|j| meta_i64(j, "failed_runs") > meta_i64(j, "fail_threshold"))
        .collect();
    let processing_jobs: Vec<&Value> = jobs.iter().filter(|j| meta_status(j) == "processing").collect();

    // Jobs stuck in 'processing' for more than 24 hours have hung.
    let hung_jobs: Vec<&Value> = processing_jobs
        .into_iter()
        .filter(|j| {
            date_millis(&j["job_meta"]["last_run"])
                .map(|last_run_ms| now_ms - last_run_ms >= 86_400_000)
                .unwrap_or(false)
        })
        .collect();
    let hung_count = hung_jobs.len();
    if hung_count > 0 {
        reset_jobs(ctx, &hung_jobs).await?;
    }

    let mut runnable: Vec<Value> = Vec::new();
    for job in &pending_jobs {
        // last_run falls back to created_at (a job never run before);
        // if neither parses the job is treated as due (the Python raised).
        let last_ms = date_millis(&job["job_meta"]["last_run"])
            .or_else(|| date_millis(&job["job_meta"]["created_at"]))
            .unwrap_or(0);

        let time_interval = meta_i64(job, "time_interval");
        if time_interval == 0 {
            runnable.push((*job).clone());
        } else {
            // COMPARE MINUTES: round((now - last_run).total_seconds() / 60)
            let elapsed_minutes = ((now_ms - last_ms) as f64 / 1000.0 / 60.0).round() as i64;
            if elapsed_minutes >= time_interval {
                runnable.push((*job).clone());
            }
        }
    }

    let mut invalid_jobs: Vec<Value> = Vec::new();
    invalid_jobs.extend(exceeded_jobs.into_iter().cloned());
    invalid_jobs.extend(failed_jobs.into_iter().cloned());
    Ok((runnable, invalid_jobs, hung_count))
}

/// `_delete_invalid_jobs`.
async fn delete_invalid_jobs(ctx: &Ctx, invalid_jobs: &[Value]) -> Result<(), OctyError> {
    let mut identifiers: Vec<Value> = Vec::new();
    let mut account_ids: Vec<Value> = Vec::new();
    for job in invalid_jobs {
        identifiers.push(job["_id"].clone());
        account_ids.push(job["account_id"].clone());
    }
    repo::delete_octy_jobs(ctx, &account_ids, &identifiers).await
}

/// `_get_accounts` — on an empty result every pending job's failed counter is
/// incremented and the tick errors out (retried on the next cron run).
async fn get_accounts(
    ctx: &Ctx,
    account_ids: &[Value],
    pending_jobs: &[Value],
) -> Result<Vec<Value>, OctyError> {
    let accounts = repo::get_pending_job_accounts(ctx, account_ids).await?;
    if accounts.is_empty() {
        let ex = "Pending jobs found, but no accounts were returned from account service. Trying again.";
        for octy_job in pending_jobs {
            repo::update_octy_job(
                ctx,
                &[JobUpdate {
                    account_id: octy_job["account_id"].clone(),
                    octy_job_id: octy_job["_id"].clone(),
                    suc_inc_by: 0,
                    fail_inc_by: 1,
                    status: "pending".to_string(),
                    action: format!("octy job queue --> Error occurred during processing :: {ex}"),
                }],
            )
            .await?;
        }
        eprintln!("[octy-jobs] Octy Job Queue >> {ex}");
        return Err(OctyError::internal(format!("Octy Job Queue >> {ex}")));
    }
    Ok(accounts)
}

/// `_process_octy_jobs` — one full scheduling pass. Returns a summary the
/// CronJob's curl output records.
pub async fn process_octy_jobs(ctx: &Ctx) -> Result<Value, OctyError> {
    eprintln!("[octy-jobs] Octy Job Queue >> Processing Octy jobs");

    let jobs = get_all_jobs(ctx).await?;
    if jobs.is_empty() {
        eprintln!("[octy-jobs] Octy Job Queue >> No pending jobs found! Going back to sleep zzz");
        return Ok(json!({ "detail": "No pending jobs found", "total_jobs": 0 }));
    }

    // Filter pending and invalid jobs
    let (pending_jobs, invalid_jobs, hung_reset) = filter_pending_exceeded_jobs(ctx, &jobs).await?;
    if !invalid_jobs.is_empty() {
        delete_invalid_jobs(ctx, &invalid_jobs).await?;
    }

    if pending_jobs.is_empty() {
        eprintln!("[octy-jobs] Octy Job Queue >> No pending jobs found! Going back to sleep zzz");
        return Ok(json!({
            "detail": "No pending jobs found",
            "total_jobs": jobs.len(),
            "deleted_invalid": invalid_jobs.len(),
            "reset_hung": hung_reset,
        }));
    }

    // Account data for all accounts associated with pending jobs (unique ids).
    let mut account_ids: Vec<Value> = Vec::new();
    for job in &pending_jobs {
        if !account_ids.contains(&job["account_id"]) {
            account_ids.push(job["account_id"].clone());
        }
    }
    let accounts = get_accounts(ctx, &account_ids, &pending_jobs).await?;

    let pod_id = octy_spin::ctx::variable("pod_id", "POD_ID").unwrap_or_default();

    let mut octy_job_updates: Vec<JobUpdate> = Vec::new();
    let mut published = 0usize;
    let mut skipped_unclaimed = 0usize;
    let mut skipped_invalid_payload = 0usize;

    for job in &pending_jobs {
        let is_job_owner = repo::claim_pending_job(
            ctx,
            job["account_id"].as_str().unwrap_or(""),
            &job["_id"],
            &pod_id,
        )?;
        if !is_job_owner {
            eprintln!(
                "[octy-jobs] Octy Job Queue >> pending job with ID: {} is not owned by this pod. Skipping job.",
                job["_id"]
            );
            skipped_unclaimed += 1;
            continue;
        }

        let account = accounts.iter().find(|a| a["_id"] == job["account_id"]);
        let Some(account) = account else {
            // The Python appended a *nested list* here, which crashed the
            // final bulk update; the flat entry below is the intended shape.
            octy_job_updates.push(JobUpdate {
                account_id: job["account_id"].clone(),
                octy_job_id: job["_id"].clone(),
                suc_inc_by: 0,
                fail_inc_by: 1,
                status: "pending".to_string(),
                action: format!(
                    "octy job queue --> Error occurred during processing :: {} was not returned by the account service.",
                    job["account_id"].as_str().unwrap_or("")
                ),
            });
            continue;
        };

        let (valid, payload) = build_message_payload(account, job);
        if valid {
            ctx.gateway
                .amqp_publish(job["job_meta"]["amqp_routing_key"].as_str().unwrap_or(""), &payload)
                .await?;
            published += 1;
        } else {
            eprintln!(
                "[octy-jobs] Job failed to be processed due to missing attributes. Account ID : {} Job type : {}",
                account["_id"],
                job["job_meta"]["job_type"].as_str().unwrap_or("")
            );
            skipped_invalid_payload += 1;
            continue;
        }

        // update job to 'processing' to ensure future ticks do not run it again
        octy_job_updates.push(JobUpdate {
            account_id: account["_id"].clone(),
            octy_job_id: job["_id"].clone(),
            suc_inc_by: 0,
            fail_inc_by: 0,
            status: "processing".to_string(),
            action: "octy job queue --> processing job".to_string(),
        });
    }

    if !octy_job_updates.is_empty() {
        repo::update_octy_job(ctx, &octy_job_updates).await?;
    }

    Ok(json!({
        "detail": "Processed Octy jobs",
        "total_jobs": jobs.len(),
        "pending": pending_jobs.len(),
        "published": published,
        "deleted_invalid": invalid_jobs.len(),
        "reset_hung": hung_reset,
        "skipped_unclaimed": skipped_unclaimed,
        "skipped_invalid_payload": skipped_invalid_payload,
    }))
}
