//! Port of `services/billing.py::BillingUnits` (data-unit metering only —
//! `profiles/` never calls `track_compute_units`/`complete_compute_units`).

use octy_shared::errors::OctyError;
use serde_json::{json, Value};

use octy_spin::ctx::Ctx;

pub struct BillingUnits {
    account_id: String,
    account_type: String,
    account_currency: String,
    process_name: String,
    data_quantity_bytes: i64,
    data_metric: String,
    capturing_data_units: bool,
}

impl BillingUnits {
    pub fn new(account_id: impl Into<String>, account_type: impl Into<String>, account_currency: impl Into<String>, process_name: impl Into<String>) -> Self {
        Self {
            account_id: account_id.into(),
            account_type: account_type.into(),
            account_currency: account_currency.into(),
            process_name: process_name.into(),
            data_quantity_bytes: 0,
            data_metric: "MB".to_string(),
            capturing_data_units: false,
        }
    }

    /// Port of `track_data_units` — `_get_size(unit)` on the created/updated
    /// profile list.
    ///
    /// NOTE (divergence): the Python `_get_size` recursively sums
    /// `sys.getsizeof(...)` across the object graph, which mirrors CPython's
    /// internal object memory layout. That layout has no Rust equivalent, so
    /// this port approximates it with fixed per-type overheads modeled on
    /// CPython 3.x on a 64-bit build (str: 49 + len bytes, dict: 64 +
    /// entries, list: 56 + entries, int/bool: 28, float: 24, null: 16). The
    /// resulting billed quantities will not match the Python service
    /// exactly; if precise parity matters for invoicing, recalibrate the
    /// constants against a real CPython measurement or move this metric to
    /// the gateway (which can report exact wire bytes).
    pub fn track_data_units(&mut self, units: &[Value]) {
        self.data_quantity_bytes += estimate_size(&Value::Array(units.to_vec()));
        self.capturing_data_units = true;
    }

    pub async fn complete_data_units(&mut self, ctx: &Ctx, metric: &str) -> Result<(), OctyError> {
        self.data_metric = metric.to_string();
        let quantity = bytes_to_metric(self.data_quantity_bytes, metric)?;
        self.capture_units(ctx, "data", quantity).await
    }

    async fn capture_units(&mut self, ctx: &Ctx, unit_type: &str, quantity: i64) -> Result<(), OctyError> {
        if self.capturing_data_units && unit_type == "data" {
            self.capturing_data_units = false;
            let quantity = if quantity > 0 { quantity } else { 1 };
            ctx.gateway
                .amqp_publish(
                    "account.billing.cmd.capture",
                    &json!({
                        "units": [{
                            "unit_type": "data",
                            "metric": self.data_metric,
                            "process_name": self.process_name,
                            "quantity": quantity,
                            "account_id": self.account_id,
                            "account_currency": self.account_currency,
                            "account_type": self.account_type,
                        }]
                    }),
                )
                .await?;
        }
        Ok(())
    }
}

fn bytes_to_metric(bytes: i64, metric: &str) -> Result<i64, OctyError> {
    let divisor = match metric {
        "KB" => 1_000.0,
        "MB" => 1_000_000.0,
        "GB" => 1_000_000_000.0,
        "TB" => 1_000_000_000_000.0,
        other => return Err(OctyError::internal(format!("Unknown data metric specified: {other}"))),
    };
    Ok((bytes as f64 / divisor).round() as i64)
}

fn estimate_size(value: &Value) -> i64 {
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
        Value::Array(items) => 56 + items.iter().map(estimate_size).sum::<i64>(),
        Value::Object(map) => {
            64 + map
                .iter()
                .map(|(k, v)| (49 + k.len() as i64) + estimate_size(v))
                .sum::<i64>()
        }
    }
}
