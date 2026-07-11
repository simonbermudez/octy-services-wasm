//! Port of `amqp/consumer.py`.
//!
//! In the Python service a background thread consumed RabbitMQ directly. A
//! WASM component has no long-lived consumer loop, so the data gateway owns
//! the AMQP connection (queue `capture-billing-units-queue`, routing key
//! `account.billing.cmd.capture`) and forwards each delivery here as
//! `POST /internal/amqp/consume` with body `{"routing_key": …, "payload": …}`.
//!
//! Response-code contract with the gateway (replacing ack/reject):
//!   2xx → ack
//!   4xx → reject, no requeue   (unparseable / "[toxic]::" payloads)
//!   5xx → reject, requeue
//!
//! Expose this route only inside the cluster (same as the Python internal
//! endpoints — do not add it to the ingress).

use serde_json::Value;

use octy_spin::ctx::Ctx;

use crate::models::BillableUnits;
use crate::services::billing as billing_service;

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

    // Payloads that fail `BillableUnits(**message_json)` validation are
    // refused without requeue (Python rejected them with requeue=False).
    let billable_units = match BillableUnits::from_value(&payload) {
        Ok(u) => u,
        Err(err) => {
            return AmqpOutcome {
                status: 400,
                detail: format!("refused message payload: {err}"),
            }
        }
    };

    // The Python consumer only acted on this routing key; anything else fell
    // through and was acked.
    if routing_key != "account.billing.cmd.capture" {
        return AmqpOutcome {
            status: 200,
            detail: format!("ignored routing key {routing_key}"),
        };
    }

    match billing_service::calculate_persist_billable_units(ctx, &billable_units.units).await {
        Ok(()) => AmqpOutcome {
            status: 200,
            detail: "ok".to_string(),
        },
        Err(err) => {
            // '[toxic]::' errors are rejected without requeue, like the Python.
            let toxic = err.to_string().contains("[toxic]::")
                || err.reasons.iter().any(|r| r.error_message.contains("[toxic]::"));
            AmqpOutcome {
                status: if toxic { 422 } else { 500 },
                detail: format!("error calculating and persisting billable units: {err}"),
            }
        }
    }
}
