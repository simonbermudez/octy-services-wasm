//! Port of `amqp/consumer.py::handle_message` — deliveries arrive as
//! `POST /internal/amqp/consume` from the data gateway (see `rust/README.md`
//! and `services/account/src/amqp.rs` for the shared contract).
//!
//! Response-code contract with the gateway:
//!   2xx → ack
//!   4xx → reject, no requeue   (unparseable payload / pydantic-equivalent
//!                                validation failure — the Python rejected
//!                                these with `requeue=False`)
//!   5xx → reject, requeue      (not used by this worker: both job runners
//!                                catch their own internal errors — see the
//!                                PYTHON BUG notes in `pipeline_complete.rs`
//!                                for the one path where a completion job can
//!                                still fail twice and fall through here)
//!
//! Unknown routing keys are acked without action, exactly like the Python
//! (`handle_message`'s `if/elif` had no `else`, and the function still
//! called `ack_message(payload)` — a plain ack — at the end regardless).

use octy_spin::ctx::Ctx;
use serde_json::Value;

use crate::models::{ChurnCompleteJob, ChurnTrainingJob};
use crate::pipeline_complete::ChurnPredictionCompleteTrainingJob;
use crate::pipeline_training::ChurnPredictionTraining;

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

    match routing_key.as_str() {
        "churn.training.cmd.run" => run_training(ctx, payload).await,
        "churn.training.complete.cmd.run" => run_completion(ctx, payload).await,
        _ => AmqpOutcome {
            status: 200,
            detail: format!("ignored routing key {routing_key}"),
        },
    }
}

async fn run_training(ctx: &Ctx, payload: Value) -> AmqpOutcome {
    let job: ChurnTrainingJob = match serde_json::from_value(payload.clone()) {
        Ok(job) => job,
        Err(err) => {
            eprintln!("Refused message payload: {payload}. Exception : {err}");
            return AmqpOutcome {
                status: 400,
                detail: format!("refused message payload: {err}"),
            };
        }
    };

    let item_feature_cols: Vec<String> = ctx
        .config
        .get_array("ITEM_FEATURE_COLS")
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let timeframe = ctx.config.get_i64("DATA_SET_TIMEFRAME").unwrap_or(0);

    let mut training = ChurnPredictionTraining::new(
        job.account_data.account_id,
        job.account_data.account_type,
        job.account_data.account_currency,
        job.octy_job_id,
        job.account_data.bucket,
        job.account_data.algorithm_configurations,
        timeframe,
        item_feature_cols,
    );
    // `run()` never returns an error — every internal failure is caught and
    // routed through `_dispose_job` (see pipeline_training.rs), matching the
    // Python's broad `except Exception` in `ChurnPredictionTraining.run()`.
    training.run(ctx).await;
    AmqpOutcome {
        status: 200,
        detail: "ok".to_string(),
    }
}

async fn run_completion(ctx: &Ctx, payload: Value) -> AmqpOutcome {
    let job: ChurnCompleteJob = match serde_json::from_value(payload.clone()) {
        Ok(job) => job,
        Err(err) => {
            eprintln!("Refused message payload: {payload}. Exception : {err}");
            return AmqpOutcome {
                status: 400,
                detail: format!("refused message payload: {err}"),
            };
        }
    };

    let mut complete = ChurnPredictionCompleteTrainingJob::new(
        job.account_data.account_id,
        job.account_data.account_type,
        job.account_data.account_currency,
        job.octy_job_id,
        job.account_data.bucket,
        job.job_data.hyperparam_tuning_job_id,
        job.account_data.churn_percentage,
        job.account_data.webhook_url,
    );
    match complete.run(ctx).await {
        Ok(()) => AmqpOutcome {
            status: 200,
            detail: "ok".to_string(),
        },
        // Only reachable when `_re_schedule_job`'s own callback HTTP request
        // fails twice in a row (see pipeline_complete.rs) — the Python
        // consumer's outer `except Exception` rejected without requeue
        // (`ack_message(payload, False, False)`), reproduced here as a 400.
        Err(err) => AmqpOutcome {
            status: 400,
            detail: format!("error running churn prediction completion job: {err}"),
        },
    }
}
