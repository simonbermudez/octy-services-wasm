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
//! Expose this route only inside the cluster (same as the Python internal
//! endpoints — do not add it to the ingress).

use octy_shared::errors::OctyError;
use octy_shared::models::UpdateAccount;
use serde_json::Value;

use octy_spin::ctx::Ctx;
use crate::repos::account as account_repo;

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

    // Unparseable message bodies are refused without requeue (Python rejected
    // them with requeue=False).
    let account = match serde_json::to_vec(&payload)
        .map_err(|e| OctyError::internal(e.to_string()))
        .and_then(|bytes| UpdateAccount::from_json(&bytes))
    {
        Ok(account) => account,
        Err(err) => {
            return AmqpOutcome {
                status: 400,
                detail: format!("refused message payload: {err}"),
            }
        }
    };

    let action = match routing_key.as_str() {
        "account.configs.cmd.update" => "account-config",
        "algo.configs.cmd.update" => "algorithm-config",
        "churn.info.cmd.update" => "churn-info",
        // Unknown routing keys were silently acked by the Python consumer.
        _ => {
            return AmqpOutcome {
                status: 200,
                detail: format!("ignored routing key {routing_key}"),
            }
        }
    };

    match account_repo::update_account(ctx, &account, action).await {
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
                detail: format!("error updating account: {err}"),
            }
        }
    }
}
