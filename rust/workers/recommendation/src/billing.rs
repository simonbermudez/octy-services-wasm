//! Port of `services/billing.py` — BillingUnits.
//!
//! The Python fire-and-forgot `account.billing.cmd.capture` publishes on the
//! main event loop; here they go through the data gateway and are awaited
//! (publish failures are logged, not fatal — matching the detached-task
//! behaviour).
//!
//! Bug-for-bug: `complete_compute_units` without a prior
//! `track_compute_units` raised `AttributeError` in Python (no
//! `compute_start_time`); that surfaces here as an `Err`, which propagates
//! exactly where the Python exception did.

use anyhow::{anyhow, bail, Result};
use serde_json::json;

use octy_spin::ctx::Ctx;

use crate::utils::bytes_to_metric;

fn now_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub struct BillingUnits {
    account_id: String,
    account_type: String,
    account_currency: String,
    process_name: String,
    // compute
    compute_quantity: i64,
    compute_metric: String,
    compute_start_time: Option<f64>,
    capturing_compute_units: bool,
    // data
    data_quantity: i64,
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
            compute_quantity: 0,
            compute_metric: "hours".to_string(),
            compute_start_time: None,
            capturing_compute_units: false,
            data_quantity: 0,
            data_metric: "MB".to_string(),
            capturing_data_units: false,
        }
    }

    // compute

    pub fn track_compute_units(&mut self, metric: &str) -> Result<()> {
        if !["seconds", "minutes", "hours"].contains(&metric) {
            bail!("Unknown compute metric specified: {metric}");
        }
        self.compute_start_time = Some(now_seconds());
        self.compute_metric = metric.to_string();
        self.capturing_compute_units = true;
        Ok(())
    }

    pub async fn complete_compute_units(&mut self, ctx: &Ctx, additional_unit_hours: f64) -> Result<()> {
        let start = self
            .compute_start_time
            .ok_or_else(|| anyhow!("'BillingUnits' object has no attribute 'compute_start_time'"))?;
        let elapsed = now_seconds() - start;
        self.compute_quantity = match self.compute_metric.as_str() {
            "seconds" => elapsed.ceil() as i64 + ((additional_unit_hours / 60.0) / 60.0).ceil() as i64,
            "minutes" => (elapsed / 60.0).ceil() as i64 + (additional_unit_hours / 60.0).ceil() as i64,
            _ => ((elapsed / 60.0) / 60.0).ceil() as i64 + additional_unit_hours.ceil() as i64,
        };
        self.capture_units(ctx, "compute").await;
        Ok(())
    }

    // data

    pub fn track_data_units(&mut self, approx_py_size: i64) {
        self.data_quantity += approx_py_size;
        self.capturing_data_units = true;
    }

    pub async fn complete_data_units(&mut self, ctx: &Ctx, metric: &str) -> Result<()> {
        self.data_metric = metric.to_string();
        self.data_quantity = bytes_to_metric(self.data_quantity, &self.data_metric)?;
        self.capture_units(ctx, "data").await;
        Ok(())
    }

    async fn capture_units(&mut self, ctx: &Ctx, unit_type: &str) {
        let (metric, quantity) = if self.capturing_data_units && unit_type == "data" {
            self.capturing_data_units = false;
            (self.data_metric.clone(), self.data_quantity)
        } else if self.capturing_compute_units && unit_type == "compute" {
            self.capturing_compute_units = false;
            (self.compute_metric.clone(), self.compute_quantity)
        } else {
            return;
        };

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
        if let Err(e) = ctx
            .gateway
            .amqp_publish("account.billing.cmd.capture", &payload)
            .await
        {
            // Python published on a detached task; failures never reached the job.
            eprintln!("[recommendation-worker] failed to publish billing units: {e}");
        }
    }
}
