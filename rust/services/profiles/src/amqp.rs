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
//! Routing keys handled (see `handle_message` in the Python source):
//!   - `profiles.cmd.update`               -> `ProfilesService.update_profiles(internal=True)`
//!   - `profiles.cmd.delete`               -> `ProfilesService.delete_profiles(identification_job=True)`
//!   - `segment.tags.cmd.update.delete`    -> `profilesRepository.update_delete_segment_tags`
//!   - `grouped.segmentation.operations.cmd` -> `ProfilesService.grouped_segmentation_database_operations`
//! Any other routing key is acked without action, matching the Python
//! `handle_message`, whose `if/elif` chain falls through to a no-op ack for
//! unrecognized keys.
//!
//! Expose this route only inside the cluster (same as the Python internal
//! endpoints — do not add it to the ingress).

use serde_json::Value;

use crate::http_util::ServiceError;
use crate::models;
use crate::services::profiles as profiles_service;
use octy_spin::ctx::Ctx;

pub struct AmqpOutcome {
    pub status: u16,
    pub detail: String,
}

fn outcome_from_service_error(err: &ServiceError) -> AmqpOutcome {
    // '[toxic]::' errors are rejected without requeue, like the Python
    // (`requeue=False if '[toxic]::' in str(ex) else True`).
    match err {
        ServiceError::Octy(e) => {
            let toxic = e.error_description.contains("[toxic]::") || e.reasons.iter().any(|r| r.error_message.contains("[toxic]::"));
            AmqpOutcome {
                status: if toxic { 422 } else { 500 },
                detail: format!("error: {e}"),
            }
        }
        ServiceError::RawList { reason, .. } => {
            let toxic = reason.contains("[toxic]::");
            AmqpOutcome {
                status: if toxic { 422 } else { 500 },
                detail: format!("error: {reason}"),
            }
        }
    }
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
    let payload_bytes = serde_json::to_vec(&payload).unwrap_or_default();

    match routing_key.as_str() {
        "profiles.cmd.update" => {
            let update = match models::UpdateProfilesAmqp::from_json(&payload_bytes) {
                Ok(u) => u,
                Err(err) => {
                    return AmqpOutcome {
                        status: 400,
                        detail: format!("refused message payload: {err}"),
                    }
                }
            };
            match profiles_service::update_profiles(ctx, &update.account_id, &update.profiles, true, None).await {
                Ok(_) => AmqpOutcome { status: 200, detail: "ok".to_string() },
                Err(err) => outcome_from_service_error(&err),
            }
        }
        "profiles.cmd.delete" => {
            let delete: models::DeleteProfilesAmqp = match serde_json::from_slice(&payload_bytes) {
                Ok(d) => d,
                Err(e) => {
                    return AmqpOutcome {
                        status: 400,
                        detail: format!("refused message payload: {e}"),
                    }
                }
            };
            match profiles_service::delete_profiles(ctx, &delete.account_id, &delete.profiles, true).await {
                Ok(_) => AmqpOutcome { status: 200, detail: "ok".to_string() },
                Err(err) => outcome_from_service_error(&err),
            }
        }
        "segment.tags.cmd.update.delete" => {
            let st = match models::SegmentIdUpdateDelete::from_json(&payload_bytes) {
                Ok(s) => s,
                Err(err) => {
                    return AmqpOutcome {
                        status: 400,
                        detail: format!("refused message payload: {err}"),
                    }
                }
            };
            let segment_ids: Vec<String> = st.segment_ids.into_iter().map(|s| s.segment_id).collect();
            match profiles_service::update_delete_segment_tags(ctx, &st.account_id, &segment_ids, &st.action).await {
                Ok(()) => AmqpOutcome { status: 200, detail: "ok".to_string() },
                Err(err) => AmqpOutcome {
                    status: 500,
                    detail: format!("error updating/deleting segment tags: {err}"),
                },
            }
        }
        "grouped.segmentation.operations.cmd" => {
            let ops = match models::GroupedSegmentationDatabaseOperations::from_json(&payload_bytes) {
                Ok(o) => o,
                Err(err) => {
                    return AmqpOutcome {
                        status: 400,
                        detail: format!("refused message payload: {err}"),
                    }
                }
            };
            profiles_service::grouped_segmentation_database_operations(ctx, &ops.account_id, &ops.operations).await;
            AmqpOutcome { status: 200, detail: "ok".to_string() }
        }
        // Unknown routing keys were silently acked by the Python consumer
        // (no branch of its if/elif chain matched, so no exception was
        // raised and the message fell through to a default `ack_message`).
        _ => AmqpOutcome {
            status: 200,
            detail: format!("ignored routing key {routing_key}"),
        },
    }
}
