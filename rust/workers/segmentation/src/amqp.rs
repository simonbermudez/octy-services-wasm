//! Port of `amqp/consumer.py`.
//!
//! In the Python service a background thread pool (bounded to 10 concurrent
//! jobs via `threading.BoundedSemaphore(10)`) consumed RabbitMQ directly and
//! ran each job to completion before acking. A Spin component has no
//! background consumer loop; the data gateway owns the AMQP connection and
//! forwards each delivery here as `POST /internal/amqp/consume` with body
//! `{"routing_key": …, "payload": …}`, running the job synchronously inside
//! the request (concurrency is bounded by the gateway's own consumer
//! parallelism, not a semaphore in this component).
//!
//! Response-code contract with the gateway (replacing ack/reject):
//!   2xx → ack
//!   4xx → reject, no requeue
//!   5xx → reject, requeue
//!
//! Python's `handle_message` only ever produced two outcomes: reject-no-requeue
//! for an unparseable/invalid payload, or ack after `run()` returns (`run()`
//! internally catches every exception and reports failure via the job-service
//! callback instead of raising) — so this port never returns a 5xx.
//!
//! Rejecting without requeue is intentional even on failure: the Octy Job
//! Scheduler (not RabbitMQ redelivery) owns retries for these jobs, so the
//! AMQP message itself doesn't need to come back around.

use serde_json::Value;

use octy_spin::ctx::Ctx;

use crate::engine::{LiveSegmentation, PastSegmentation, PendingLiveSegmentation};
use crate::models::{LiveSegmentationJob, PastSegmentationJob};

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
    let routing_key = envelope.get("routing_key").and_then(Value::as_str).unwrap_or_default().to_string();
    let payload = envelope.get("payload").cloned().unwrap_or(Value::Null);

    match routing_key.as_str() {
        "past.segmentation.cmd.run" => {
            let job: PastSegmentationJob = match serde_json::from_value(payload.clone()) {
                Ok(job) => job,
                Err(err) => {
                    eprintln!("[segmentation-worker] Refused message payload: {payload}. Exception : {err}");
                    return AmqpOutcome {
                        status: 400,
                        detail: format!("invalid past.segmentation.cmd.run payload: {err}"),
                    };
                }
            };
            let engine = PastSegmentation::new(
                ctx,
                job.account_data.account_id,
                job.account_data.account_type,
                job.account_data.account_currency,
                job.octy_job_id,
                job.job_data.segment_data.segment_id.unwrap_or_default(),
            );
            engine.run().await;
            AmqpOutcome {
                status: 200,
                detail: "ok".to_string(),
            }
        }
        "live.segmentation.cmd.run" => {
            let job: LiveSegmentationJob = match serde_json::from_value(payload.clone()) {
                Ok(job) => job,
                Err(err) => {
                    eprintln!("[segmentation-worker] Refused message payload: {payload}. Exception : {err}");
                    return AmqpOutcome {
                        status: 400,
                        detail: format!("invalid live.segmentation.cmd.run payload: {err}"),
                    };
                }
            };
            match job.job_data.segment_data.segmentation_type.as_str() {
                "live" => {
                    let engine = LiveSegmentation::new(
                        ctx,
                        job.account_data.account_id,
                        job.account_data.webhook_url,
                        job.octy_job_id,
                        job.job_data.event_data.as_value(),
                    );
                    engine.run().await;
                }
                "pending-live" => {
                    let profile_id = job.job_data.event_data.profile.profile_id.clone();
                    let event_timeframe = job.job_data.event_data.event_timeframe.unwrap_or(0);
                    let engine = PendingLiveSegmentation::new(
                        ctx,
                        job.account_data.account_id,
                        job.account_data.webhook_url,
                        job.job_data.segment_data.segment_id.unwrap_or_default(),
                        profile_id,
                        job.octy_job_id,
                        job.job_data.live_octy_job_id.unwrap_or_default(),
                        event_timeframe,
                    );
                    engine.run().await;
                }
                // Python: neither `if`/`elif` branch matches -> no-op, message acked.
                _ => {}
            }
            AmqpOutcome {
                status: 200,
                detail: "ok".to_string(),
            }
        }
        // Python: routing key not matched by the consumer's `if`/`elif` ->
        // no-op, message acked (the gateway only forwards configured keys
        // anyway).
        _ => AmqpOutcome {
            status: 200,
            detail: format!("ignored routing key {routing_key}"),
        },
    }
}
