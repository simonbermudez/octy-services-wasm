//! Port of `amqp/consumer.py`.
//!
//! Deliveries arrive as `POST /internal/amqp/consume` with
//! `{"routing_key": ..., "payload": ...}`; the gateway maps the response
//! code to the ack/reject decision the Python's `ack_message` made:
//!   2xx → ack, 4xx → reject (no requeue), 5xx → reject + requeue.
//!
//! ## Preserved Python quirks
//!
//! * **Unknown routing key → silent ack, no-op.** In `handle_message`,
//!   `job_payload` is only assigned inside the `if/elif` for the two known
//!   routing keys; for anything else, no exception is raised (the first
//!   `try` doesn't reference `job_payload`), and the second `if/elif`
//!   (which drives `RFMAnalysis`/`RFMCompleteAnalysis`) simply matches
//!   nothing. Execution falls through to `ack_message(payload)` — a
//!   successful ack that did no work.
//! * **The job classes never let `run()` fail.** Both `RFMAnalysis.run()`
//!   and `RFMCompleteAnalysis.run()` wrap their entire body in
//!   `try/except Exception`, calling `capture_exception` +
//!   `complete_compute_units` + dispose/reschedule internally and never
//!   re-raising. So once the payload parses, the consumer always acks
//!   (200) regardless of whether the job succeeded, failed and disposed of
//!   itself, or was rescheduled — matching `training.rs`/`complete.rs`,
//!   whose `run()` methods likewise return `()`, not `Result`.

use serde_json::Value;

use octy_spin::ctx::Ctx;

use crate::complete::RfmCompleteAnalysis;
use crate::models::{RfmAnalysisJob, RfmCompleteJob};
use crate::training::RfmAnalysis;

pub struct AmqpOutcome {
    pub status: u16,
    pub detail: String,
}

pub async fn handle_delivery(ctx: &Ctx, body: &[u8]) -> AmqpOutcome {
    let Ok(envelope) = serde_json::from_slice::<Value>(body) else {
        return AmqpOutcome { status: 400, detail: "invalid delivery envelope".to_string() };
    };
    let routing_key = envelope.get("routing_key").and_then(Value::as_str).unwrap_or_default().to_string();
    let payload = envelope.get("payload").cloned().unwrap_or(Value::Null);

    match routing_key.as_str() {
        "rfm.training.cmd.run" => {
            let job: RfmAnalysisJob = match serde_json::from_value(payload) {
                Ok(job) => job,
                Err(e) => {
                    eprintln!("[rfm-worker] Refused message payload for rfm.training.cmd.run: {e}");
                    return AmqpOutcome { status: 400, detail: format!("invalid RFMAnalysisJob payload: {e}") };
                }
            };
            let mut analysis = match RfmAnalysis::new(
                ctx,
                job.account_data.account_id,
                job.account_data.account_type,
                job.account_data.account_currency,
                job.octy_job_id,
                job.account_data.bucket,
            ) {
                Ok(a) => a,
                Err(e) => return AmqpOutcome { status: 500, detail: format!("failed to initialize RFMAnalysis: {e}") },
            };
            analysis.run().await;
            AmqpOutcome { status: 200, detail: "ok".to_string() }
        }
        "rfm.training.complete.cmd.run" => {
            let job: RfmCompleteJob = match serde_json::from_value(payload) {
                Ok(job) => job,
                Err(e) => {
                    eprintln!("[rfm-worker] Refused message payload for rfm.training.complete.cmd.run: {e}");
                    return AmqpOutcome { status: 400, detail: format!("invalid RFMCompleteJob payload: {e}") };
                }
            };
            let mut analysis = RfmCompleteAnalysis::new(
                ctx,
                job.account_data.account_id,
                job.account_data.account_type,
                job.account_data.account_currency,
                job.octy_job_id,
                job.account_data.bucket,
                job.job_data.training_job_id,
                job.account_data.webhook_url,
            );
            analysis.run().await;
            AmqpOutcome { status: 200, detail: "ok".to_string() }
        }
        _ => AmqpOutcome { status: 200, detail: format!("ignored routing key {routing_key}") },
    }
}
