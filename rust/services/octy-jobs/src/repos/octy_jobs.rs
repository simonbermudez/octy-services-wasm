//! Port of `data/repositories/implementation/octy_jobs_repository.py`.
//!
//! MongoDB access goes through the data gateway (documents travel as legacy
//! extended JSON). The Redis job-claim keys are written directly via Spin's
//! outbound-Redis host capability — db index 3, same as the Python
//! `db_redis_connect(db=3)`.

use octy_shared::ejson::now_legacy_date;
use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::redis::{Connection, RedisParameter, RedisResult};

use crate::gw;
use octy_spin::ctx::Ctx;
use octy_spin::gateway::http_post_json_with_retry;

const COLLECTION: &str = "tbl_octy_jobs";

/// One entry of the `octy_job_updates` list handed to `update_octy_job`.
#[derive(Debug, Clone)]
pub struct JobUpdate {
    /// The Python passed either the job's `account_id` string or the account
    /// document's `_id`, so this stays a raw JSON value.
    pub account_id: Value,
    /// Either a 24-hex string (HTTP callback) or a `{"$oid": …}` value (tick).
    pub octy_job_id: Value,
    pub suc_inc_by: i64,
    pub fail_inc_by: i64,
    pub status: String,
    pub action: String,
}

/// `ObjectId(octy_job_id)` equivalent: normalize to `{"$oid": hex}` so the
/// gateway converts it to a real ObjectId. (The Python called
/// `ObjectId(job['octy_job_id'])`, which raised for the `{"$oid": …}` dicts
/// the tick produced — normalizing both shapes is the intended behaviour.)
fn oid_filter(id: &Value) -> Result<Value, OctyError> {
    if id.get("$oid").is_some() {
        return Ok(id.clone());
    }
    if let Some(s) = id.as_str() {
        if s.len() == 24 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(json!({ "$oid": s }));
        }
        // bson.ObjectId(<bad str>) raised InvalidId -> generic 500, keep that.
        return Err(OctyError::internal(format!(
            "'{s}' is not a valid ObjectId, it must be a 12-byte input or a 24-character hex string"
        )));
    }
    Err(OctyError::internal("invalid octy_job_id"))
}

/// The Python interpolated the raw `job['_id']` into the Redis key with an
/// f-string; for the tick that value was a dict, so the key literally contains
/// the Python dict repr (`{'$oid': 'hex'}`). Reproduce it for key parity.
fn job_id_repr(id: &Value) -> String {
    if let Some(hex) = octy_shared::ejson::oid_hex(id) {
        return format!("{{'$oid': '{hex}'}}");
    }
    if let Some(s) = id.as_str() {
        return s.to_string();
    }
    id.to_string()
}

fn redis_conn(ctx: &Ctx) -> Result<Connection, OctyError> {
    Connection::open(&ctx.redis_address(3)?)
        .map_err(|e| OctyError::internal(format!("redis connect failed: {e:?}")))
}

// ---------------------------------------------------------------------------
// Repository methods
// ---------------------------------------------------------------------------

pub async fn create_octy_job(ctx: &Ctx, account_id: &str, octy_job: &Value) -> Result<(), OctyError> {
    let query = json!({
        "$and": [
            { "account_id": account_id },
            { "job_meta.job_type": octy_job["job_meta"]["job_type"] },
            { "job_meta.amqp_routing_key": octy_job["job_meta"]["amqp_routing_key"] },
            { "job_data": octy_job["job_data"] }
        ]
    });

    // `find(query).to_list(length=1)` — first match only.
    let existing = ctx.gateway.find(COLLECTION, query, 0, 1).await?;

    let mut create_job_ref = true;
    if let Some(job) = existing.first() {
        let successful = job["job_meta"]["successful_runs"].as_i64().unwrap_or(0);
        let desired = job["job_meta"]["desired_runs"].as_i64().unwrap_or(0);
        let failed = job["job_meta"]["failed_runs"].as_i64().unwrap_or(0);
        let threshold = job["job_meta"]["fail_threshold"].as_i64().unwrap_or(0);
        create_job_ref = successful >= desired || failed >= threshold;
    }

    if create_job_ref {
        ctx.gateway
            .insert_one(
                COLLECTION,
                json!({
                    "octy_job_id": octy_job["octy_job_id"],
                    "account_id": account_id,
                    "alt_dentifier": octy_job["alt_dentifier"],
                    "job_meta": octy_job["job_meta"],
                    "job_data": octy_job["job_data"],
                }),
            )
            .await?;
    }
    Ok(())
}

/// `bulk_write([UpdateOne, …])` — emulated with sequential update-one calls
/// through the gateway (same per-document semantics, N round trips).
pub async fn update_octy_job(ctx: &Ctx, octy_job_updates: &[JobUpdate]) -> Result<(), OctyError> {
    for job in octy_job_updates {
        let filter = json!({
            "account_id": job.account_id,
            "_id": oid_filter(&job.octy_job_id)?,
        });

        let update = if job.status == "processing" {
            json!({
                "$set": {
                    "job_meta.status": job.status,
                    "job_meta.last_run": now_legacy_date(),
                    "job_meta.updated_at": now_legacy_date(),
                    "job_meta.last_updated_action": job.action,
                }
            })
        } else {
            json!({
                "$inc": {
                    "job_meta.successful_runs": job.suc_inc_by,
                    "job_meta.failed_runs": job.fail_inc_by,
                },
                "$set": {
                    "job_meta.status": job.status,
                    "job_meta.updated_at": now_legacy_date(),
                    "job_meta.last_updated_action": job.action,
                }
            })
        };

        ctx.gateway.update_one(COLLECTION, filter, update).await?;
    }
    Ok(())
}

/// The Python built per-identifier `UpdateOne` no-ops it never executed and
/// then issued a single `delete_many` guarded by `if operations:` — i.e. the
/// delete runs whenever `identifiers` is non-empty. Same here.
pub async fn delete_octy_jobs(
    _ctx: &Ctx,
    account_ids: &[Value],
    identifiers: &[Value],
) -> Result<(), OctyError> {
    if identifiers.is_empty() {
        return Ok(());
    }
    gw::delete_many(
        COLLECTION,
        json!({
            "$or": [
                { "_id": { "$in": identifiers } },
                { "alt_dentifier": { "$in": identifiers } }
            ],
            "account_id": { "$in": account_ids }
        }),
    )
    .await
}

pub async fn delete_all_octy_jobs(ctx: &Ctx, account_id: &str) -> Result<bool, OctyError> {
    gw::delete_many(COLLECTION, json!({ "account_id": account_id })).await?;
    // Python quirk preserved: redis DEL does not glob, so this removes only a
    // key literally named `…:octy.job:*` (effectively a no-op) — the per-job
    // claim keys expire via their 24h TTL instead.
    redis_conn(ctx)?
        .del(&[format!("account.id:{account_id}:octy.job:*")])
        .map_err(|e| OctyError::internal(format!("redis del failed: {e:?}")))?;
    Ok(true)
}

pub async fn get_octy_jobs(_ctx: &Ctx, cursor: i64) -> Result<Vec<Value>, OctyError> {
    gw::find_sorted(
        COLLECTION,
        json!({}),
        cursor,
        1000,
        json!({ "job_meta.created_at": 1 }),
    )
    .await
}

/// Paged fetch of the accounts backing the pending jobs from the account
/// service's internal endpoint (with the `requests_retry_session` retry
/// behaviour on 500/502/504).
pub async fn get_pending_job_accounts(
    ctx: &Ctx,
    account_ids: &[Value],
) -> Result<Vec<Value>, OctyError> {
    let url = format!(
        "{}/v1/internal/accounts",
        ctx.config.get_str("ACCOUNT_SERVICE_CLUSTER_IP")?
    );
    let payload = json!({ "account_ids": account_ids });

    let mut accounts: Vec<Value> = Vec::new();
    let mut cursor: i64 = 0;
    loop {
        let cursor_header = cursor.to_string();
        let (status, body) =
            http_post_json_with_retry(&url, &[("cursor", cursor_header.as_str())], &payload).await?;
        if status != 200 {
            break;
        }
        let parsed: Value = serde_json::from_slice(&body)
            .map_err(|e| OctyError::internal(format!("invalid accounts response: {e}")))?;
        let page = parsed
            .get("accounts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let count = parsed["request_meta"]["count"].as_i64().unwrap_or(0);
        accounts.extend(page);
        // Guard the Python's unbounded loop: a 200 with count == 0 means the
        // cursor is exhausted (the Python would have spun forever here).
        if count < 1 {
            break;
        }
        cursor += count;
    }
    Ok(accounts)
}

/// `SET key pod_id NX EX 86400`; on conflict the claim belongs to whichever
/// pod's id is stored.
pub fn claim_pending_job(
    ctx: &Ctx,
    account_id: &str,
    octy_job_id: &Value,
    pod_id: &str,
) -> Result<bool, OctyError> {
    let conn = redis_conn(ctx)?;
    let name = format!("account.id:{account_id}:octy.job:{}", job_id_repr(octy_job_id));

    let args = [
        RedisParameter::Binary(name.clone().into_bytes()),
        RedisParameter::Binary(pod_id.as_bytes().to_vec()),
        RedisParameter::Binary(b"NX".to_vec()),
        RedisParameter::Binary(b"EX".to_vec()),
        RedisParameter::Binary(b"86400".to_vec()),
    ];
    let result = conn
        .execute("SET", &args)
        .map_err(|e| OctyError::internal(format!("redis set failed: {e:?}")))?;

    if matches!(result.first(), Some(RedisResult::Status(_))) {
        return Ok(true);
    }

    let current = conn
        .get(&name)
        .map_err(|e| OctyError::internal(format!("redis get failed: {e:?}")))?;
    Ok(current.map(|bytes| bytes == pod_id.as_bytes()).unwrap_or(false))
}
