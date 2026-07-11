//! Port of `amqp/consumer.py` + the dispatch in `worker/__init__.py`.
//!
//! The Python background-thread consumer parsed the message into a pydantic
//! `ProfileIdenJob`, ran `ProfileIdentification.run()` to completion
//! synchronously on a per-thread event loop, and only ever
//! rejected-without-requeue on failure (`ack_message(payload, False,
//! False)`) — both for unparseable payloads and for any exception raised
//! while running the job (job retries are the Octy Job Scheduler's
//! responsibility, driven by the `/v1/internal/jobs/callback` call, not
//! RabbitMQ requeue).
//!
//! Response-code contract with the data gateway (replacing ack/reject):
//!   2xx → ack, 4xx → reject (no requeue), 5xx → reject + requeue.
//! Every failure path here is mapped to `400` to match that "always
//! reject-without-requeue on error" behaviour.

use serde_json::Value;

use octy_spin::ctx::Ctx;

use crate::job::ProfileIdentificationJob;
use crate::models::ProfileIdenJob;

pub struct AmqpOutcome {
    pub status: u16,
    pub detail: String,
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

    if routing_key != "profile.identification.cmd.run" {
        // Any other routing key was never bound to this worker's queue in
        // Python (`AMQP_CONSUMERS` only lists `profile.identification.cmd.run`),
        // so this branch is unreachable in production; reject defensively.
        return AmqpOutcome {
            status: 400,
            detail: format!("unrecognized routing key: {routing_key}"),
        };
    }

    let job_payload: ProfileIdenJob = match serde_json::from_value(payload) {
        Ok(job) => job,
        Err(err) => {
            // Mirrors `handle_message`'s `except Exception as ex` around
            // `ProfileIdenJob(**message_json)` — refused without requeue.
            return AmqpOutcome {
                status: 400,
                detail: format!("refused message payload: {err}"),
            };
        }
    };

    let job = ProfileIdentificationJob::new(job_payload.account_data, job_payload.octy_job_id);
    match job.run(ctx).await {
        Ok(()) => AmqpOutcome {
            status: 200,
            detail: "ok".to_string(),
        },
        Err(detail) => AmqpOutcome { status: 400, detail },
    }
}
