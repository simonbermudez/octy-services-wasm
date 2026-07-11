//! Data-gateway operations the shared `octy_spin::gateway::GatewayClient`
//! does not expose. The events service needs:
//!
//!   * `find` with a `sort` spec (latest-event lookups)      — GATEWAY GAP:
//!     `/v1/mongo/{coll}/find` must accept an optional `"sort"` document.
//!   * `insert-many` (batch event / event-type creation)     — GATEWAY GAP:
//!     `POST /v1/mongo/{coll}/insert-many` with body
//!     `{"documents": […], "ordered": false}` returning
//!     `{"inserted": <n>, "write_errors": [{"index": <i>, "message": "…"}]}`
//!     (per-op failures — e.g. duplicate keys with `ordered:false` — must be
//!     reported in `write_errors`, not as a non-2xx status).
//!   * `delete-many` (profile/account fan-out deletion)      — GATEWAY GAP:
//!     `POST /v1/mongo/{coll}/delete-many` with `{"filter": …}` returning
//!     `{"deleted": <n>}`.
//!   * `delete-one` **with** the deleted count — the gateway already returns
//!     `{"deleted": n}`; the shared client just discards it.

use octy_shared::errors::{ErrorReason, OctyError};
use octy_spin::gateway::http_send;
use serde_json::{json, Value};
use spin_sdk::http::Method;

pub struct GatewayExt {
    base: String,
}

impl GatewayExt {
    pub fn load() -> Self {
        let base = octy_spin::ctx::variable("gateway_url", "GATEWAY_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string());
        Self {
            base: base.trim_end_matches('/').to_string(),
        }
    }

    /// Same response handling as `GatewayClient::post` (409 → 'Duplicate entry').
    async fn post(&self, path: &str, body: &Value) -> Result<Value, OctyError> {
        let url = format!("{}{}", self.base, path);
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

    /// `find(filter).sort(sort).skip(skip).limit(limit)`; `limit == 0` means
    /// no limit (matches the gateway's `filter(|l| *l > 0)` handling).
    pub async fn find_sorted(
        &self,
        collection: &str,
        filter: Value,
        sort: Value,
        skip: i64,
        limit: i64,
    ) -> Result<Vec<Value>, OctyError> {
        let res = self
            .post(
                &format!("/v1/mongo/{collection}/find"),
                &json!({ "filter": filter, "sort": sort, "skip": skip, "limit": limit }),
            )
            .await?;
        Ok(res
            .get("documents")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    /// `insert_many(documents, ordered=…)` — returns the per-op write errors
    /// (`[{"index": i, "message": "…"}]`), empty when everything inserted.
    pub async fn insert_many(
        &self,
        collection: &str,
        documents: Vec<Value>,
        ordered: bool,
    ) -> Result<Vec<Value>, OctyError> {
        let res = self
            .post(
                &format!("/v1/mongo/{collection}/insert-many"),
                &json!({ "documents": documents, "ordered": ordered }),
            )
            .await?;
        Ok(res
            .get("write_errors")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    /// `delete_many(filter)` — returns the deleted count.
    pub async fn delete_many(&self, collection: &str, filter: Value) -> Result<i64, OctyError> {
        let res = self
            .post(&format!("/v1/mongo/{collection}/delete-many"), &json!({ "filter": filter }))
            .await?;
        Ok(res.get("deleted").and_then(Value::as_i64).unwrap_or(0))
    }

    /// `delete_one(filter).deleted_count` — existing gateway endpoint; reads
    /// the `deleted` count the shared client throws away.
    pub async fn delete_one_counted(&self, collection: &str, filter: Value) -> Result<i64, OctyError> {
        let res = self
            .post(&format!("/v1/mongo/{collection}/delete-one"), &json!({ "filter": filter }))
            .await?;
        Ok(res.get("deleted").and_then(Value::as_i64).unwrap_or(0))
    }
}
