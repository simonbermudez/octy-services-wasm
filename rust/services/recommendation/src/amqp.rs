//! Port of `amqp/consumer.py`.
//!
//! In the Python service a background thread consumed RabbitMQ directly. A
//! WASM component has no long-lived consumer loop, so the data gateway owns
//! the AMQP connection and forwards each delivery here as
//! `POST /internal/amqp/consume` with body `{"routing_key": …, "payload": …}`.
//!
//! Response-code contract with the gateway (replacing ack/reject):
//!   2xx → ack
//!   4xx → reject, no requeue   (unparseable / "[toxic]::" payloads)
//!   5xx → reject, requeue
//!
//! Consumed routing keys: `reccache.cmd.delete` (published by the
//! profile-identification worker after profile merges).
//!
//! Expose this route only inside the cluster — do not add it to the ingress.

use serde_json::Value;

use octy_shared::errors::OctyError;
use octy_spin::ctx::Ctx;

use crate::models::DeleteRecCache;
use crate::repos::recommendation as repo;

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

    // The Python consumer parsed `DeleteRecCache` before looking at the
    // routing key: any delivery with an unparseable payload is refused
    // without requeue, regardless of key.
    let job = match serde_json::to_vec(&payload)
        .map_err(|e| OctyError::internal(e.to_string()))
        .and_then(|bytes| DeleteRecCache::from_json(&bytes))
    {
        Ok(job) => job,
        Err(err) => {
            return AmqpOutcome {
                status: 400,
                detail: format!("refused message payload: {err}"),
            }
        }
    };

    if routing_key != "reccache.cmd.delete" {
        // Deliveries with other routing keys were acked without any work.
        return AmqpOutcome {
            status: 200,
            detail: format!("ignored routing key {routing_key}"),
        };
    }

    match repo::delete_cached_recommendations(ctx, &job.account_id, &job.profiles).await {
        Ok(()) => AmqpOutcome {
            status: 200,
            detail: "ok".to_string(),
        },
        Err(err) => {
            // '[toxic]::' errors are rejected without requeue, like the Python.
            let toxic = err.to_string().contains("[toxic]::")
                || err
                    .reasons
                    .iter()
                    .any(|r| r.error_message.contains("[toxic]::"));
            AmqpOutcome {
                status: if toxic { 422 } else { 500 },
                detail: format!("error deleting cached recommendations: {err}"),
            }
        }
    }
}
