//! Port of `services/billing.py` — usage-unit capture published to
//! `account.billing.cmd.capture`. Only the data-unit path is exercised by the
//! events service (compute units were never tracked here).

use octy_spin::auth::AuthAccount;
use octy_spin::ctx::Ctx;
use serde_json::{json, Value};

use crate::http_util::ApiError;

pub struct BillingUnits {
    account_id: String,
    account_type: Value,
    account_currency: Value,
    process_name: String,
    data_quantity_bytes: f64,
    data_metric: String,
    capturing_data_units: bool,
}

impl BillingUnits {
    /// `BillingUnits(account_id, a_cf['a_t'], a_cf['a_c'], process_name)` —
    /// missing claims raised KeyError in the service constructor → 500.
    pub fn new(account: &AuthAccount, account_id: &str, process_name: &str) -> Result<Self, ApiError> {
        let account_type = account
            .account_configurations
            .get("a_t")
            .cloned()
            .ok_or_else(|| ApiError::internal("KeyError: account_configurations['a_t']"))?;
        let account_currency = account
            .account_configurations
            .get("a_c")
            .cloned()
            .ok_or_else(|| ApiError::internal("KeyError: account_configurations['a_c']"))?;
        Ok(Self {
            account_id: account_id.to_string(),
            account_type,
            account_currency,
            process_name: process_name.to_string(),
            data_quantity_bytes: 0.0,
            data_metric: "KB".to_string(),
            capturing_data_units: false,
        })
    }

    pub fn track_data_units(&mut self, unit: &Value) {
        self.data_quantity_bytes += approx_py_size(unit);
        self.capturing_data_units = true;
    }

    pub async fn complete_data_units(&mut self, ctx: &Ctx, metric: &str) -> Result<(), ApiError> {
        self.data_metric = metric.to_string();
        let quantity = bytes_to_metric(self.data_quantity_bytes, metric)?;
        if !self.capturing_data_units {
            return Ok(());
        }
        self.capturing_data_units = false;
        ctx.gateway
            .amqp_publish(
                "account.billing.cmd.capture",
                &json!({
                    "units": [{
                        "unit_type": "data",
                        "metric": self.data_metric,
                        "process_name": self.process_name,
                        "quantity": if quantity > 0 { quantity } else { 1 },
                        "account_id": self.account_id,
                        "account_currency": self.account_currency,
                        "account_type": self.account_type,
                    }]
                }),
            )
            .await
            .map_err(ApiError::from)
    }
}

/// `_bytes_to_metric` — Python `round()` (banker's rounding, half-to-even).
fn bytes_to_metric(bytes: f64, metric: &str) -> Result<i64, ApiError> {
    let divisor = match metric {
        "KB" => 1_000.0,
        "MB" => 1_000_000.0,
        "GB" => 1_000_000_000.0,
        "TB" => 1_000_000_000_000.0,
        _ => return Err(ApiError::internal(format!("Unknown data metric specified: {metric}"))),
    };
    Ok((bytes / divisor).round_ties_even() as i64)
}

/// Approximation of the recursive `sys.getsizeof` walk in `_get_size` using
/// CPython 64-bit object sizes. Exact byte parity with the interpreter is
/// impossible from JSON, but the events service only ever bills at MB
/// granularity, so the rounding result is unaffected for realistic payloads.
fn approx_py_size(value: &Value) -> f64 {
    match value {
        Value::Null => 16.0,
        Value::Bool(_) => 28.0,
        Value::Number(n) => {
            if n.is_f64() {
                24.0
            } else {
                28.0
            }
        }
        Value::String(s) => 49.0 + s.len() as f64,
        Value::Array(items) => {
            56.0 + 8.0 * items.len() as f64 + items.iter().map(approx_py_size).sum::<f64>()
        }
        Value::Object(map) => {
            64.0 + map
                .iter()
                .map(|(k, v)| 49.0 + k.len() as f64 + approx_py_size(v))
                .sum::<f64>()
        }
    }
}
