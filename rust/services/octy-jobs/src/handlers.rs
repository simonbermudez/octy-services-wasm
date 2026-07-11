//! Route handlers — ports of `api/routers/octy_jobs.py` / `healthz.py`, plus
//! the two internal routes replacing the Python background machinery (AMQP
//! consumer thread and the in-process job-queue scheduler).
//!
//! Rate limiting (slowapi) is not replicated — enforce at the ingress.
//! Sentry is replaced by stderr logging.

use serde_json::json;
use spin_sdk::http::{Params, Request, Response};

use crate::amqp;
use crate::http_util::*;
use crate::models::{DeleteAccountJobs, OctyJobCallBack};
use crate::services::octy_jobs as service;
use octy_spin::ctx::Ctx;

/// `config.py` read `OCTY_JOB_CONFIG` / `OCTY_JOB_SECRETS` (singular "JOB"),
/// so the variable prefix is `octy_job`, not `octy_jobs`.
fn ctx_or_response() -> Result<Ctx, Response> {
    Ctx::load("octy_job").map_err(|e| error_response(&e))
}

/// GET /healthz
pub async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &json!("OK"))
}

/// POST /v1/internal/jobs/callback — workers report job outcomes here.
pub async fn octy_job_callback(req: Request, _params: Params) -> Response {
    // FastAPI validated the body before the handler ran.
    let cb = match OctyJobCallBack::from_json(req.body()) {
        Ok(cb) => cb,
        Err(errors) => return validation_response(&errors),
    };

    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    match service::status_callback(&ctx, &cb).await {
        Ok(()) => json_response(200, &json!("OK")),
        Err(err) => error_response(&err),
    }
}

/// POST /v1/internal/jobs/delete — account-deletion fan-out from the account
/// service (cluster-internal only, keep off the ingress).
pub async fn delete_account_jobs(req: Request, _params: Params) -> Response {
    let payload = match DeleteAccountJobs::from_json(req.body()) {
        Ok(payload) => payload,
        Err(errors) => return validation_response(&errors),
    };

    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    match service::delete_all_octy_jobs(&ctx, &payload.account_id).await {
        Ok(is_deleted) => delete_account_jobs_dto(is_deleted),
        Err(err) => error_response(&err),
    }
}

/// POST /internal/amqp/consume — deliveries forwarded by the data gateway.
pub async fn amqp_consume(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let outcome = amqp::handle_delivery(&ctx, req.body()).await;
    json_response(outcome.status, &json!({ "detail": outcome.detail }))
}

/// POST /internal/scheduler/tick — one `OctyJobQueue._process_octy_jobs`
/// pass. Hit by a Kubernetes CronJob every 2 minutes (the Python
/// `queue_process_interval`). Cluster-internal only.
pub async fn scheduler_tick(_req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    match service::process_octy_jobs(&ctx).await {
        Ok(summary) => json_response(200, &summary),
        Err(err) => error_response(&err),
    }
}

pub async fn fallback(_req: Request, _params: Params) -> Response {
    not_found()
}
