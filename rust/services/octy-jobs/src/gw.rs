//! Gateway operations the shared `octy_spin::gateway::GatewayClient` does not
//! expose yet. The Python repository needs two Mongo capabilities the gateway
//! currently lacks:
//!
//!   * `delete_many` (used by `delete_octy_jobs` / `delete_all_octy_jobs`) —
//!     coded against a hypothetical `POST /v1/mongo/{coll}/delete-many`
//!     endpoint with body `{"filter": ...}` → `{"deleted_count": n}`.
//!   * sorted `find` (`get_octy_jobs` sorts on `job_meta.created_at` asc) —
//!     a `"sort"` field is sent with the standard `/find` body; the current
//!     gateway deserializer silently ignores it (documents come back in
//!     natural order until the gateway grows sort support).
//!
//! Both gaps are reported to the maintainers rather than patched here (the
//! gateway is out of scope for this port).

use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::http::Method;

fn base_url() -> String {
    octy_spin::ctx::variable("gateway_url", "GATEWAY_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string())
        .trim_end_matches('/')
        .to_string()
}

async fn post(path: &str, body: &Value) -> Result<Value, OctyError> {
    let url = format!("{}{}", base_url(), path);
    let (status, response_body) = octy_spin::gateway::http_send(
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
    Err(OctyError::internal(format!("gateway {path}: {detail}")))
}

/// `collection.delete_many(filter)` — hypothetical gateway endpoint (see
/// module docs).
pub async fn delete_many(collection: &str, filter: Value) -> Result<(), OctyError> {
    post(&format!("/v1/mongo/{collection}/delete-many"), &json!({ "filter": filter })).await?;
    Ok(())
}

/// `collection.find(filter).sort(...).skip(...).limit(...)` — the `sort` field
/// is a gateway extension (currently ignored; see module docs).
pub async fn find_sorted(
    collection: &str,
    filter: Value,
    skip: i64,
    limit: i64,
    sort: Value,
) -> Result<Vec<Value>, OctyError> {
    let res = post(
        &format!("/v1/mongo/{collection}/find"),
        &json!({ "filter": filter, "skip": skip, "limit": limit, "sort": sort }),
    )
    .await?;
    Ok(res
        .get("documents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}
