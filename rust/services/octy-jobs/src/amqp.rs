//! Port of `amqp/consumer.py`.
//!
//! The Python service consumed `octy.job.cmd.create` / `octy.job.cmd.delete`
//! on a background thread. A WASM component has no long-lived consumer loop,
//! so the data gateway owns the AMQP connection and forwards each delivery
//! here as `POST /internal/amqp/consume` with body
//! `{"routing_key": …, "payload": …}`.
//!
//! Response-code contract with the gateway (replacing ack/reject):
//!   2xx → ack
//!   4xx → reject, no requeue   (unparseable payloads / "[toxic]::" errors)
//!   5xx → reject, requeue
//!
//! Expose this route only inside the cluster — do not add it to the ingress.
//!
//! NOTE: the Python capped concurrent delivery handling at 10 via a
//! `threading.BoundedSemaphore(10)`; that throttle has no equivalent here —
//! concurrency is whatever the gateway/host chooses to dispatch.

use serde_json::Value;

use crate::models::{CreateOctyJob, DeleteOctyJob};
use crate::services::octy_jobs as service;
use octy_spin::ctx::Ctx;

pub struct AmqpOutcome {
    pub status: u16,
    pub detail: String,
}

fn refused(errors: Vec<Value>) -> AmqpOutcome {
    AmqpOutcome {
        status: 400,
        detail: format!("refused message payload: {}", Value::Array(errors)),
    }
}

fn service_error(err: octy_shared::errors::OctyError, what: &str) -> AmqpOutcome {
    // '[toxic]::' errors are rejected without requeue, like the Python.
    let toxic = err.to_string().contains("[toxic]::")
        || err.reasons.iter().any(|r| r.error_message.contains("[toxic]::"));
    eprintln!("[octy-jobs] Error {what} octy jobs: {err}");
    AmqpOutcome {
        status: if toxic { 422 } else { 500 },
        detail: format!("error {what} octy jobs: {err}"),
    }
}

pub async fn handle_delivery(ctx: &Ctx, body: &[u8]) -> AmqpOutcome {
    let Ok(envelope) = serde_json::from_slice::<Value>(body) else {
        return AmqpOutcome {
            status: 400,
            detail: "invalid delivery envelope".to_string(),
        };
    };
    let routing_key = envelope["routing_key"].as_str().unwrap_or_default().to_string();
    let payload = envelope.get("payload").cloned().unwrap_or(Value::Null);

    match routing_key.as_str() {
        "octy.job.cmd.create" => {
            let octy_job = match CreateOctyJob::from_value(&payload) {
                Ok(job) => job,
                Err(errors) => return refused(errors),
            };
            match service::create_new_job(ctx, &octy_job).await {
                Ok(()) => AmqpOutcome { status: 200, detail: "ok".to_string() },
                Err(err) => service_error(err, "creating"),
            }
        }
        "octy.job.cmd.delete" => {
            let delete_jobs = match DeleteOctyJob::from_value(&payload) {
                Ok(msg) => msg,
                Err(errors) => return refused(errors),
            };
            let octy_job_ids = delete_jobs.octy_job_ids.unwrap_or_default();
            let alt_identifiers = delete_jobs.alt_identifiers.unwrap_or_default();
            match service::delete_octy_jobs(ctx, &delete_jobs.account_id, octy_job_ids, alt_identifiers).await
            {
                Ok(()) => AmqpOutcome { status: 200, detail: "ok".to_string() },
                Err(err) => service_error(err, "deleting"),
            }
        }
        // Unknown routing keys fell through both branches and were acked.
        _ => AmqpOutcome {
            status: 200,
            detail: format!("ignored routing key {routing_key}"),
        },
    }
}
