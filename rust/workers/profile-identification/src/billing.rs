//! Port of `services/billing.py::BillingUnits` (compute units only — this
//! worker never tracked data units).
//!
//! The Python fired `account.billing.cmd.capture` on the main asyncio loop
//! (`self.loop.create_task(...)`), detached from the job's own control flow
//! — publish failures never surfaced to the job. Ported as an awaited
//! gateway call whose errors are logged, not propagated, to match that
//! fire-and-forget behaviour.

use serde_json::json;

use octy_spin::ctx::Ctx;

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
    compute_quantity: i64,
    compute_metric: String,
    compute_start_time: Option<f64>,
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
        }
    }

    /// Port of `track_compute_units('hours')` — called once at job start.
    pub fn track_compute_units(&mut self, metric: &str) {
        debug_assert!(["seconds", "minutes", "hours"].contains(&metric));
        self.compute_start_time = Some(now_seconds());
        self.compute_metric = metric.to_string();
    }

    /// Port of `complete_compute_units` (`additional_unit_hours` always `0`
    /// in this worker — the Python never passed a non-default value here).
    /// May be called multiple times per job (see `job.rs`'s dispose-on-error
    /// path, which replicates the Python's duplicate-capture bug).
    pub async fn complete_compute_units(&mut self, ctx: &Ctx) {
        let elapsed = self
            .compute_start_time
            .map(|start| now_seconds() - start)
            .unwrap_or(0.0);
        self.compute_quantity = match self.compute_metric.as_str() {
            "seconds" => elapsed.ceil() as i64,
            "minutes" => (elapsed / 60.0).ceil() as i64,
            _ => ((elapsed / 60.0) / 60.0).ceil() as i64,
        };

        let quantity = if self.compute_quantity > 0 { self.compute_quantity } else { 1 };
        let payload = json!({
            "units": [
                {
                    "unit_type": "compute",
                    "metric": self.compute_metric,
                    "process_name": self.process_name,
                    "quantity": quantity,
                    "account_id": self.account_id,
                    "account_currency": self.account_currency,
                    "account_type": self.account_type,
                }
            ]
        });
        if let Err(err) = ctx.gateway.amqp_publish("account.billing.cmd.capture", &payload).await {
            eprintln!("[profile-identification-worker] failed to publish billing units: {err}");
        }
    }
}
