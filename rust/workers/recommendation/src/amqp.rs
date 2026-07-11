//! Port of `amqp/consumer.py` + the routing done in `worker/__init__.py`.
//!
//! The Python background-thread consumer parsed the message into a pydantic
//! model, ran the matching job class, and only ever rejected-without-requeue
//! (`ack_message(payload, False, False)`) on failure — both for unparseable
//! payloads and for exceptions raised while running the job. In practice the
//! job classes (`RecommenderTraining.run` / `RecommenderCompleteTrainingJob.run`)
//! swallow every internal exception themselves (dispose_job /
//! re_schedule_job), so the "job raised" path is effectively dead code; it is
//! kept here for parity (mapped to reject-without-requeue, matching Python).
//!
//! Response-code contract with the data gateway (replacing ack/reject):
//!   2xx → ack, 4xx → reject (no requeue), 5xx → reject + requeue.
//! An unparseable envelope or unrecognized routing key is a 400 (the Python
//! `UnboundLocalError` / pydantic `ValidationError` path also rejected
//! without requeue).
//!
//! NOTE: the Python consumer capped concurrent job threads at 10
//! (`threading.BoundedSemaphore(10)`) to avoid overloading the pod. There is
//! no equivalent cap here — each delivery is one Spin HTTP request handled
//! synchronously, so concurrency is bounded by whatever the host/gateway
//! allows rather than by this worker.

use serde_json::Value;

use octy_spin::ctx::Ctx;

use crate::models::{RecCompleteJob, RecTrainingJob};
use crate::services::prediction::RecommenderCompleteTrainingJob;
use crate::services::training::RecommenderTraining;

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
        "rec.training.cmd.run" => {
            let job: RecTrainingJob = match serde_json::from_value(payload) {
                Ok(job) => job,
                Err(err) => {
                    return AmqpOutcome {
                        status: 400,
                        detail: format!("refused message payload: {err}"),
                    }
                }
            };
            let training = match RecommenderTraining::new(
                ctx,
                job.account_data.account_id,
                job.account_data.account_type,
                job.account_data.account_currency,
                job.octy_job_id,
                job.account_data.bucket,
                job.account_data.algorithm_configurations,
            ) {
                Ok(training) => training,
                Err(err) => {
                    return AmqpOutcome {
                        status: 400,
                        detail: format!("refused message payload: {err}"),
                    }
                }
            };
            training.run().await;
            AmqpOutcome {
                status: 200,
                detail: "ok".to_string(),
            }
        }
        "rec.training.complete.cmd.run" => {
            let job: RecCompleteJob = match serde_json::from_value(payload) {
                Ok(job) => job,
                Err(err) => {
                    return AmqpOutcome {
                        status: 400,
                        detail: format!("refused message payload: {err}"),
                    }
                }
            };
            let completion = match RecommenderCompleteTrainingJob::new(
                ctx,
                job.account_data.account_id,
                job.account_data.account_type,
                job.account_data.account_currency,
                job.account_data.algorithm_configurations,
                job.octy_job_id,
                job.job_data.hyperparam_tuning_job_id,
                job.account_data.bucket,
                job.account_data.webhook_url,
            ) {
                Ok(completion) => completion,
                Err(err) => {
                    return AmqpOutcome {
                        status: 400,
                        detail: format!("refused message payload: {err}"),
                    }
                }
            };
            completion.run().await;
            AmqpOutcome {
                status: 200,
                detail: "ok".to_string(),
            }
        }
        other => AmqpOutcome {
            status: 400,
            detail: format!("unrecognized routing key: {other}"),
        },
    }
}
