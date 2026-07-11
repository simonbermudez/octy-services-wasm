//! Port of `services/billing.py`.
//!
//! DIVERGENCE (documented): the Python `_capture_units` fired the AMQP
//! publish as a detached `loop.create_task(...)` (fire-and-forget on the
//! shared asyncio loop). A Spin HTTP component has no background event loop
//! outside the current request, so this port `await`s the publish inline
//! before returning. The externally observable effect — one
//! `account.billing.cmd.capture` message per completed job — is identical;
//! only the (unobservable, same-process) scheduling changes.

use chrono::{DateTime, Utc};
use octy_shared::errors::OctyError;
use octy_spin::ctx::Ctx;
use serde_json::json;

// Only `Hours` is ever passed by the engine (`track_compute_units('hours')`
// in every call site), but the other two variants of the Python
// `compute_metric` API are ported for completeness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputeMetric {
    #[allow(dead_code)]
    Seconds,
    #[allow(dead_code)]
    Minutes,
    Hours,
}

pub struct BillingUnits {
    account_id: String,
    account_type: Option<String>,
    account_currency: Option<String>,
    process_name: &'static str,
    compute_quantity: i64,
    compute_metric: ComputeMetric,
    compute_start_time: Option<DateTime<Utc>>,
}

impl BillingUnits {
    pub fn new(
        account_id: impl Into<String>,
        account_type: Option<String>,
        account_currency: Option<String>,
        process_name: &'static str,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            account_type,
            account_currency,
            process_name,
            compute_quantity: 0,
            compute_metric: ComputeMetric::Hours,
            compute_start_time: None,
        }
    }

    /// `track_compute_units(metric)`.
    pub fn track_compute_units(&mut self, metric: ComputeMetric) {
        self.compute_start_time = Some(Utc::now());
        self.compute_metric = metric;
    }

    /// `complete_compute_units(additional_unit_hours=0)`.
    pub async fn complete_compute_units(&mut self, ctx: &Ctx, additional_unit_hours: f64) {
        let complete_time = Utc::now();
        let elapsed_seconds = self
            .compute_start_time
            .map(|start| (complete_time - start).num_milliseconds() as f64 / 1000.0)
            .unwrap_or(0.0);

        self.compute_quantity = match self.compute_metric {
            ComputeMetric::Seconds => {
                (elapsed_seconds.ceil() as i64) + ((additional_unit_hours / 60.0 / 60.0).ceil() as i64)
            }
            ComputeMetric::Minutes => {
                ((elapsed_seconds / 60.0).ceil() as i64) + ((additional_unit_hours / 60.0).ceil() as i64)
            }
            ComputeMetric::Hours => {
                ((elapsed_seconds / 60.0 / 60.0).ceil() as i64) + (additional_unit_hours.ceil() as i64)
            }
        };
        self.capture_units(ctx).await;
    }

    async fn capture_units(&self, ctx: &Ctx) {
        let metric = match self.compute_metric {
            ComputeMetric::Seconds => "seconds",
            ComputeMetric::Minutes => "minutes",
            ComputeMetric::Hours => "hours",
        };
        let quantity = if self.compute_quantity > 0 { self.compute_quantity } else { 1 };
        let payload = json!({
            "units": [{
                "unit_type": "compute",
                "metric": metric,
                "process_name": self.process_name,
                "quantity": quantity,
                "account_id": self.account_id,
                "account_currency": self.account_currency,
                "account_type": self.account_type,
            }]
        });
        if let Err(err) = publish(ctx, "account.billing.cmd.capture", &payload).await {
            eprintln!("[segmentation-worker] failed to publish billing units: {err}");
        }
    }
}

async fn publish(ctx: &Ctx, routing_key: &str, payload: &serde_json::Value) -> Result<(), OctyError> {
    ctx.gateway.amqp_publish(routing_key, payload).await
}
