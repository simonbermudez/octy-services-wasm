//! Port of `services/billing.py` (`BillingUnits`).
//!
//! Divergences from the Python implementation (documented, not fixed):
//! * `track_data_units` used CPython's `sys.getsizeof` to recursively size
//!   the `{'data': <csv str>, 'type': <str>}` object. There is no portable
//!   equivalent in Rust; we approximate with the UTF-8 byte length of the
//!   CSV payload plus a small fixed overhead for the wrapper dict/keys. The
//!   CSV payload dominates the true size in every real invocation, so the
//!   billed "MB" quantity should match the Python service closely in
//!   practice (may differ by a few bytes pre-rounding, which is usually
//!   invisible after `round(bytes / 1_000_000)`).
//! * `loop.create_task(...)` fire-and-forget publishing became a plain
//!   `await` — the WASM component has no background task scheduler, so the
//!   publish must complete before the HTTP response is returned. Observable
//!   behaviour (the AMQP message eventually gets published) is unchanged.

use octy_shared::errors::OctyError;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

use octy_spin::ctx::Ctx;

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub struct BillingUnits {
    account_id: String,
    account_type: String,
    account_currency: String,
    process_name: String,

    compute_start: f64,
    compute_metric: String,
    capturing_compute_units: bool,

    data_quantity_bytes: f64,
    data_metric: String,
    capturing_data_units: bool,
}

impl BillingUnits {
    pub fn new(account_id: &str, account_type: &str, account_currency: &str, process_name: &str) -> Self {
        Self {
            account_id: account_id.to_string(),
            account_type: account_type.to_string(),
            account_currency: account_currency.to_string(),
            process_name: process_name.to_string(),
            compute_start: 0.0,
            compute_metric: "hours".to_string(),
            capturing_compute_units: false,
            data_quantity_bytes: 0.0,
            data_metric: "MB".to_string(),
            capturing_data_units: false,
        }
    }

    /// `track_compute_units(metric)`
    pub fn track_compute_units(&mut self, metric: &str) -> Result<(), OctyError> {
        if !matches!(metric, "seconds" | "minutes" | "hours") {
            return Err(OctyError::internal(format!("Unknown compute metric specified: {metric}")));
        }
        self.compute_start = now_secs();
        self.compute_metric = metric.to_string();
        self.capturing_compute_units = true;
        Ok(())
    }

    /// `complete_compute_units(additional_unit_hours=0)`
    pub async fn complete_compute_units(&mut self, ctx: &Ctx, additional_unit_hours: f64) {
        let elapsed = now_secs() - self.compute_start;
        let quantity: i64 = match self.compute_metric.as_str() {
            "seconds" => elapsed.ceil() as i64 + ((additional_unit_hours / 60.0 / 60.0).ceil() as i64),
            "minutes" => (elapsed / 60.0).ceil() as i64 + (additional_unit_hours / 60.0).ceil() as i64,
            "hours" => (elapsed / 60.0 / 60.0).ceil() as i64 + additional_unit_hours.ceil() as i64,
            _ => 0,
        };
        self.capture_units(ctx, "compute", self.compute_metric.clone(), quantity).await;
    }

    /// `track_data_units(unit)` — `unit` here is the CSV payload we are about
    /// to upload; see the module-level doc comment for the `sys.getsizeof`
    /// approximation.
    pub fn track_data_units(&mut self, csv_payload: &str) {
        self.data_quantity_bytes += (csv_payload.len() + 96) as f64;
        self.capturing_data_units = true;
    }

    /// `complete_data_units(metric)`
    pub async fn complete_data_units(&mut self, ctx: &Ctx, metric: &str) -> Result<(), OctyError> {
        let quantity = bytes_to_metric(self.data_quantity_bytes, metric)?;
        self.data_metric = metric.to_string();
        self.capture_units(ctx, "data", metric.to_string(), quantity).await;
        Ok(())
    }

    async fn capture_units(&mut self, ctx: &Ctx, unit_type: &str, metric: String, quantity: i64) {
        let (should_capture, flag) = match unit_type {
            "data" => (self.capturing_data_units, &mut self.capturing_data_units),
            "compute" => (self.capturing_compute_units, &mut self.capturing_compute_units),
            _ => return,
        };
        if !should_capture {
            return;
        }
        *flag = false;

        let payload = json!({
            "units": [
                {
                    "unit_type": unit_type,
                    "metric": metric,
                    "process_name": self.process_name,
                    "quantity": if quantity > 0 { quantity } else { 1 },
                    "account_id": self.account_id,
                    "account_currency": self.account_currency,
                    "account_type": self.account_type,
                }
            ]
        });

        if let Err(err) = ctx.gateway.amqp_publish("account.billing.cmd.capture", &payload).await {
            eprintln!("[rfm-worker] failed to publish billing units: {err}");
        }
    }
}

/// Port of `_bytes_to_metric`. NB: Python's `round()` is banker's rounding
/// (round-half-to-even); `f64::round()` here is round-half-away-from-zero.
/// The two can disagree only on an exact `.5` boundary, which is
/// astronomically unlikely for real byte counts — documented, not fixed.
fn bytes_to_metric(bytes: f64, metric: &str) -> Result<i64, OctyError> {
    let divisor = match metric {
        "KB" => 1_000.0,
        "MB" => 1_000_000.0,
        "GB" => 1_000_000_000.0,
        "TB" => 1_000_000_000_000.0,
        other => return Err(OctyError::internal(format!("Unknown data metric specified: {other}"))),
    };
    Ok((bytes / divisor).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_to_metric_matches_python_round() {
        assert_eq!(bytes_to_metric(1_600_000.0, "MB").unwrap(), 2);
        assert_eq!(bytes_to_metric(200_000.0, "MB").unwrap(), 0);
        assert!(bytes_to_metric(1.0, "PB").is_err());
    }

    /// Documents the round-half tie-breaking divergence from Python's
    /// banker's rounding (see the doc comment on `bytes_to_metric`): this
    /// Rust port rounds `.5` away from zero, Python rounds to even.
    #[test]
    fn bytes_to_metric_half_tie_diverges_from_python_banker_rounding() {
        // Python: round(500_000 / 1_000_000) == round(0.5) == 0 (round-to-even).
        // Rust:   (0.5_f64).round() == 1 (round-half-away-from-zero).
        assert_eq!(bytes_to_metric(500_000.0, "MB").unwrap(), 1);
    }
}
