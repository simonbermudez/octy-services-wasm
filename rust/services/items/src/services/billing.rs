//! Port of `services/billing.py` — captures billing units and publishes them
//! on `account.billing.cmd.capture` (via the data gateway instead of the
//! in-process amqpPublisher).
//!
//! Divergence note: the Python measured payload size with a recursive
//! `sys.getsizeof` walk over live CPython objects. `py_get_size` approximates
//! those CPython object sizes for JSON values. The measured bytes are divided
//! by 1e6 and rounded (then clamped to a minimum quantity of 1), so for the
//! ≤100-item payloads this service bills the result is identical (1 MB).

use octy_shared::errors::OctyError;
use octy_spin::auth::AuthAccount;
use octy_spin::ctx::Ctx;
use serde_json::{json, Value};

pub struct BillingUnits {
    account_id: Value,
    account_type: Value,
    account_currency: Value,
    process_name: String,
    // compute
    compute_quantity: i64,
    compute_metric: String,
    capturing_compute_units: bool,
    compute_start_time: f64,
    // data
    data_quantity: i64,
    data_metric: String,
    capturing_data_units: bool,
}

impl BillingUnits {
    /// `ItemsService.__init__` read `a_cf['a_t']` / `a_cf['a_c']` eagerly —
    /// a missing key was a `KeyError` (→ 500) on every authenticated route.
    pub fn for_account(account: &AuthAccount, process_name: &str) -> Result<Self, OctyError> {
        let a_cf = &account.account_configurations;
        let account_type = a_cf
            .get("a_t")
            .cloned()
            .ok_or_else(|| OctyError::internal("KeyError: 'a_t'"))?;
        let account_currency = a_cf
            .get("a_c")
            .cloned()
            .ok_or_else(|| OctyError::internal("KeyError: 'a_c'"))?;
        Ok(Self {
            account_id: account.account_id.clone(),
            account_type,
            account_currency,
            process_name: process_name.to_string(),
            compute_quantity: 0,
            compute_metric: "hours".to_string(),
            capturing_compute_units: false,
            compute_start_time: 0.0,
            data_quantity: 0,
            data_metric: "MB".to_string(),
            capturing_data_units: false,
        })
    }

    // ---- compute (unused by the items routes, ported for parity) ----

    #[allow(dead_code)]
    pub fn track_compute_units(&mut self, metric: &str) -> Result<(), OctyError> {
        if !matches!(metric, "seconds" | "minutes" | "hours") {
            return Err(OctyError::internal(format!(
                "Unknown compute metric specified: {metric}"
            )));
        }
        self.compute_start_time = now_secs();
        self.compute_metric = metric.to_string();
        self.capturing_compute_units = true;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn complete_compute_units(
        &mut self,
        ctx: &Ctx,
        additional_unit_hours: f64,
    ) -> Result<(), OctyError> {
        let complete_time = now_secs();
        let elapsed = complete_time - self.compute_start_time;
        self.compute_quantity = match self.compute_metric.as_str() {
            "seconds" => elapsed.ceil() as i64 + ((additional_unit_hours / 60.0) / 60.0).ceil() as i64,
            "minutes" => (elapsed / 60.0).ceil() as i64 + (additional_unit_hours / 60.0).ceil() as i64,
            _ => ((elapsed / 60.0) / 60.0).ceil() as i64 + additional_unit_hours.ceil() as i64,
        };
        self.capture_units(ctx, "compute").await
    }

    // ---- data ----

    pub fn track_data_units(&mut self, unit: &Value) {
        self.data_quantity += py_get_size(unit);
        self.capturing_data_units = true;
    }

    pub async fn complete_data_units(&mut self, ctx: &Ctx, metric: &str) -> Result<(), OctyError> {
        self.data_metric = metric.to_string();
        self.data_quantity = bytes_to_metric(self.data_quantity, &self.data_metric)?;
        self.capture_units(ctx, "data").await
    }

    async fn capture_units(&mut self, ctx: &Ctx, unit_type: &str) -> Result<(), OctyError> {
        if self.capturing_data_units && unit_type == "data" {
            self.capturing_data_units = false;
            let quantity = if self.data_quantity > 0 { self.data_quantity } else { 1 };
            ctx.gateway
                .amqp_publish(
                    "account.billing.cmd.capture",
                    &json!({
                        "units": [
                            {
                                "unit_type": unit_type,
                                "metric": self.data_metric,
                                "process_name": self.process_name,
                                "quantity": quantity,
                                "account_id": self.account_id,
                                "account_currency": self.account_currency,
                                "account_type": self.account_type,
                            }
                        ]
                    }),
                )
                .await?;
        }
        if self.capturing_compute_units && unit_type == "compute" {
            self.capturing_compute_units = false;
            let quantity = if self.compute_quantity > 0 { self.compute_quantity } else { 1 };
            ctx.gateway
                .amqp_publish(
                    "account.billing.cmd.capture",
                    &json!({
                        "units": [
                            {
                                "unit_type": unit_type,
                                "metric": self.compute_metric,
                                "process_name": self.process_name,
                                "quantity": quantity,
                                "account_id": self.account_id,
                                "account_currency": self.account_currency,
                                "account_type": self.account_type,
                            }
                        ]
                    }),
                )
                .await?;
        }
        Ok(())
    }
}

// ---- helpers ----

fn now_secs() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

/// Python 3 `round()` — round half to even.
fn py_round(x: f64) -> i64 {
    let floor = x.floor();
    let diff = x - floor;
    if diff > 0.5 {
        floor as i64 + 1
    } else if diff < 0.5 {
        floor as i64
    } else {
        let f = floor as i64;
        if f % 2 == 0 {
            f
        } else {
            f + 1
        }
    }
}

fn bytes_to_metric(bytes: i64, metric: &str) -> Result<i64, OctyError> {
    let divisor: f64 = match metric {
        "KB" => 1_000.0,
        "MB" => 1_000_000.0,
        "GB" => 1_000_000_000.0,
        "TB" => 1_000_000_000_000.0,
        _ => {
            return Err(OctyError::internal(format!(
                "Unknown data metric specified: {metric}"
            )))
        }
    };
    Ok(py_round(bytes as f64 / divisor))
}

/// Approximation of `_get_size` (recursive `sys.getsizeof`) for JSON values,
/// using CPython 64-bit object sizes. Python tracked visited object ids to
/// guard against self-referential cycles; `serde_json::Value` is a tree with
/// no shared/cyclic references, so that bookkeeping isn't needed here.
fn py_get_size(value: &Value) -> i64 {
    match value {
        Value::Null => 16,
        Value::Bool(_) => 28,
        Value::Number(n) => {
            if n.is_f64() {
                24
            } else {
                28
            }
        }
        Value::String(s) => 49 + s.len() as i64,
        Value::Array(items) => {
            56 + 8 * items.len() as i64 + items.iter().map(py_get_size).sum::<i64>()
        }
        Value::Object(map) => {
            232 + map
                .iter()
                .map(|(k, v)| 49 + k.len() as i64 + py_get_size(v))
                .sum::<i64>()
        }
    }
}
