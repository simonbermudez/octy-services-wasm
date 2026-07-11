//! Extra `octy-data-gateway` MongoDB endpoints not wrapped by
//! `octy_spin::gateway::GatewayClient` (which only exposes find/find-one/
//! count/insert-one/update-one/delete-one). The gateway image is generic and
//! already implements `update-many`, `delete-many` and `aggregate` for other
//! services, so this module talks to them directly over HTTP rather than
//! duplicating `GatewayClient`'s plumbing.
//!
//! `insert-many` is deliberately NOT used here: on a partial failure (e.g. a
//! duplicate `customer_id`) the gateway's bulk endpoints only return a single
//! collapsed error, with no per-document result — insufficient to reproduce
//! `profiles_repository.create_profiles`'s per-profile
//! `created`/`failed_to_create` split. `services::profiles` therefore issues
//! one `insert_one` per profile instead (see the divergence note there).

use octy_shared::errors::{ErrorReason, OctyError};
use serde_json::{json, Value};
use spin_sdk::http::Method;

use octy_spin::ctx::variable;
use octy_spin::gateway::http_send;

fn gateway_base() -> String {
    variable("gateway_url", "GATEWAY_URL").unwrap_or_else(|_| "http://127.0.0.1:8090".to_string())
}

async fn post(path: &str, body: &Value) -> Result<Value, OctyError> {
    let url = format!("{}{}", gateway_base().trim_end_matches('/'), path);
    let (status, response_body) = http_send(
        Method::Post,
        &url,
        &[("content-type", "application/json")],
        Some(serde_json::to_vec(body).expect("serializable json")),
    )
    .await?;

    let parsed: Value = serde_json::from_slice(&response_body).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(parsed);
    }
    let detail = parsed
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("gateway error")
        .to_string();
    if status == 409 {
        return Err(OctyError::new(
            400,
            "Duplicate entry",
            vec![ErrorReason::new(detail, "")],
        ));
    }
    Err(OctyError::internal(format!("gateway {path}: {detail}")))
}

/// `update_many` — used for bulk, fire-and-forget updates where the Python
/// service does not need per-document results (segment-tag housekeeping).
pub async fn update_many(collection: &str, filter: Value, update: Value) -> Result<(), OctyError> {
    post(&format!("/v1/mongo/{collection}/update-many"), &json!({ "filter": filter, "update": update })).await?;
    Ok(())
}

pub async fn delete_many(collection: &str, filter: Value) -> Result<(), OctyError> {
    post(&format!("/v1/mongo/{collection}/delete-many"), &json!({ "filter": filter })).await?;
    Ok(())
}

pub async fn aggregate(collection: &str, pipeline: Vec<Value>) -> Result<Vec<Value>, OctyError> {
    let res = post(&format!("/v1/mongo/{collection}/aggregate"), &json!({ "pipeline": pipeline })).await?;
    Ok(res.get("documents").and_then(Value::as_array).cloned().unwrap_or_default())
}
