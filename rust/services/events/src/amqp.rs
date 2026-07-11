//! Port of `amqp/consumer.py`.
//!
//! The Python background thread consumed RabbitMQ directly; in the WASM port
//! the data gateway owns the AMQP connection and forwards each delivery as
//! `POST /internal/amqp/consume` with body `{"routing_key": …, "payload": …}`.
//!
//! Response-code contract with the gateway:
//!   2xx → ack
//!   4xx → reject, no requeue   (unparseable payload — Python rejected with
//!         requeue=False on a JSON/model parse failure)
//!   5xx → reject, requeue      (repository error — Python's default reject
//!         path requeues unless the error text contained "[toxic]::", which
//!         no code in this service ever raises)

use serde_json::Value;

use octy_spin::ctx::Ctx;

use crate::models::{DeleteProfiles, UpdateEventsOwner};
use crate::repos::events as events_repo;

pub struct AmqpOutcome {
    pub status: u16,
    pub detail: String,
}

pub async fn handle_delivery(_ctx: &Ctx, body: &[u8]) -> AmqpOutcome {
    let Ok(envelope) = serde_json::from_slice::<Value>(body) else {
        return AmqpOutcome {
            status: 400,
            detail: "refused message payload: invalid JSON envelope".to_string(),
        };
    };
    let routing_key = envelope["routing_key"].as_str().unwrap_or_default().to_string();
    let payload = envelope.get("payload").cloned().unwrap_or(Value::Null);
    let payload_bytes = serde_json::to_vec(&payload).unwrap_or_default();

    match routing_key.as_str() {
        "events.cmd.delete" => {
            let parsed: Result<DeleteProfiles, _> = serde_json::from_slice(&payload_bytes);
            let p = match parsed {
                Ok(p) => p,
                Err(e) => {
                    return AmqpOutcome {
                        status: 400,
                        detail: format!("refused message payload: {e}"),
                    }
                }
            };
            match events_repo::delete_profile_events(&p.account_id, &p.profile_id).await {
                Ok(()) => AmqpOutcome {
                    status: 200,
                    detail: "ok".to_string(),
                },
                Err(err) => AmqpOutcome {
                    status: 500,
                    detail: format!("Error updating or deleting events: {err}"),
                },
            }
        }
        "events.cmd.update" => {
            let parsed: Result<UpdateEventsOwner, _> = serde_json::from_slice(&payload_bytes);
            let p = match parsed {
                Ok(p) => p,
                Err(e) => {
                    return AmqpOutcome {
                        status: 400,
                        detail: format!("refused message payload: {e}"),
                    }
                }
            };
            let ctx = match Ctx::load("events") {
                Ok(ctx) => ctx,
                Err(e) => {
                    return AmqpOutcome {
                        status: 500,
                        detail: format!("ctx load failed: {e}"),
                    }
                }
            };
            match events_repo::update_events_owner(&ctx, &p.account_id, &p.profiles).await {
                Ok(()) => AmqpOutcome {
                    status: 200,
                    detail: "ok".to_string(),
                },
                Err(err) => AmqpOutcome {
                    status: 500,
                    detail: format!("Error updating or deleting events: {err}"),
                },
            }
        }
        // The Python `handle_message` silently dropped unmatched routing
        // keys (`p` stayed unbound, then `routing_key == 'events.cmd.delete'`
        // was False for both branches — the message was still ack'd because
        // no exception was raised). Preserve that: ack without action.
        _ => AmqpOutcome {
            status: 200,
            detail: format!("ignored routing key {routing_key}"),
        },
    }
}
