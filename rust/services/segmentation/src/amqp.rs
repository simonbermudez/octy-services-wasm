//! Port of `amqp/consumer.py`.
//!
//! The Python service consumed RabbitMQ in a background thread. A WASM
//! component has no long-lived consumer loop, so the data gateway owns the
//! AMQP connection and forwards each delivery here as
//! `POST /internal/amqp/consume` with body `{"routing_key": …, "payload": …}`.
//!
//! Response-code contract with the gateway (replacing ack/reject):
//!   2xx → ack
//!   4xx → reject, no requeue   (unparseable / "[toxic]::" payloads)
//!   5xx → reject, requeue
//!
//! Consumed routing keys: `segment.profiles.cmd.update`.
//! Expose this route only inside the cluster.

use serde_json::Value;

use octy_spin::ctx::Ctx;

use crate::models::UpdatePastSegementProfiles;
use crate::services::segmentation as service;

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

    // The Python parsed `UpdatePastSegementProfiles` before looking at the
    // routing key: unparseable payloads are refused without requeue whatever
    // the key.
    let u_p_s_p: UpdatePastSegementProfiles = match serde_json::from_value(payload) {
        Ok(model) => model,
        Err(err) => {
            eprintln!("[segmentation] refused message payload. Exception : {err}");
            return AmqpOutcome {
                status: 400,
                detail: format!("refused message payload: {err}"),
            };
        }
    };

    if routing_key != "segment.profiles.cmd.update" {
        // Unknown routing keys were acked without processing.
        return AmqpOutcome {
            status: 200,
            detail: format!("ignored routing key {routing_key}"),
        };
    }

    match service::update_past_segment_profiles(ctx, &u_p_s_p.account_id, &u_p_s_p.profiles).await
    {
        Ok(()) => AmqpOutcome {
            status: 200,
            detail: "ok".to_string(),
        },
        Err(err) => {
            eprintln!("[segmentation] Error updating segments: {err}");
            // '[toxic]::' errors are rejected without requeue, like the Python.
            let toxic = err.to_string().contains("[toxic]::")
                || err
                    .reasons
                    .iter()
                    .any(|r| r.error_message.contains("[toxic]::"));
            AmqpOutcome {
                status: if toxic { 422 } else { 500 },
                detail: format!("error updating segments: {err}"),
            }
        }
    }
}
