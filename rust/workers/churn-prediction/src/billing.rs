//! Port of `services/billing.py::BillingUnits`.
//!
//! The Python fired `account.billing.cmd.capture` via
//! `loop.create_task(...)` (queued on the main asyncio loop, running after
//! the current handler returns). The Spin component handles one HTTP request
//! at a time with no background loop, so these publishes are awaited inline
//! — the AMQP message still goes out before the `/internal/amqp/consume`
//! response is returned to the gateway, which is a strict improvement (the
//! Python could lose the capture if the process exited before the task ran).

use octy_spin::ctx::Ctx;

pub struct BillingUnits {
    account_id: String,
    account_type: String,
    account_currency: String,
    process_name: String,

    compute_quantity: i64,
    compute_metric: &'static str,
    compute_start_ms: i64,
    capturing_compute_units: bool,

    data_quantity: i64,
    data_metric: &'static str,
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
            compute_metric: "hours",
            compute_start_ms: 0,
            capturing_compute_units: false,
            data_quantity: 0,
            data_metric: "MB",
            capturing_data_units: false,
        }
    }

    /// `track_compute_units(metric)`.
    pub fn track_compute_units(&mut self, metric: &'static str) {
        // Python raised on an unknown metric; the two call sites always pass
        // "hours", so that's the only value accepted here too.
        debug_assert!(matches!(metric, "seconds" | "minutes" | "hours"));
        self.compute_start_ms = now_ms();
        self.compute_metric = metric;
        self.capturing_compute_units = true;
    }

    /// `complete_compute_units(additional_unit_hours=0)`.
    pub async fn complete_compute_units(&mut self, ctx: &Ctx, additional_unit_hours: f64) {
        let elapsed_secs = ((now_ms() - self.compute_start_ms) as f64 / 1000.0).max(0.0);
        self.compute_quantity = match self.compute_metric {
            "seconds" => elapsed_secs.ceil() as i64 + ((additional_unit_hours / 60.0 / 60.0).ceil() as i64),
            "minutes" => (elapsed_secs / 60.0).ceil() as i64 + (additional_unit_hours / 60.0).ceil() as i64,
            _ => (elapsed_secs / 60.0 / 60.0).ceil() as i64 + additional_unit_hours.ceil() as i64,
        };
        self.capture_units_compute(ctx).await;
    }

    /// `track_data_units(csv_object)` — approximates CPython's
    /// `sys.getsizeof` recursive walk over `{'data': str, 'type': str}`.
    pub fn track_data_units(&mut self, data_len: usize, type_len: usize) {
        self.data_quantity += crate::util::py_csv_object_sizeof(data_len, type_len);
        self.capturing_data_units = true;
    }

    /// `complete_data_units(metric)`.
    pub async fn complete_data_units(&mut self, ctx: &Ctx, metric: &'static str) {
        self.data_metric = metric;
        self.data_quantity = bytes_to_metric(self.data_quantity, metric);
        self.capture_units_data(ctx).await;
    }

    async fn capture_units_data(&mut self, ctx: &Ctx) {
        if !self.capturing_data_units {
            return;
        }
        self.capturing_data_units = false;
        let quantity = if self.data_quantity > 0 { self.data_quantity } else { 1 };
        self.publish(ctx, "data", self.data_metric, quantity).await;
    }

    async fn capture_units_compute(&mut self, ctx: &Ctx) {
        if !self.capturing_compute_units {
            return;
        }
        self.capturing_compute_units = false;
        let quantity = if self.compute_quantity > 0 { self.compute_quantity } else { 1 };
        self.publish(ctx, "compute", self.compute_metric, quantity).await;
    }

    async fn publish(&self, ctx: &Ctx, unit_type: &str, metric: &str, quantity: i64) {
        let payload = serde_json::json!({
            "units": [{
                "unit_type": unit_type,
                "metric": metric,
                "process_name": self.process_name,
                "quantity": quantity,
                "account_id": self.account_id,
                "account_currency": self.account_currency,
                "account_type": self.account_type,
            }]
        });
        if let Err(err) = ctx.gateway.amqp_publish("account.billing.cmd.capture", &payload).await {
            eprintln!("[churn-worker] failed to publish account.billing.cmd.capture: {err}");
        }
    }
}

fn now_ms() -> i64 {
    // WASI has no monotonic clock host call wired through spin-sdk here;
    // wall-clock UTC millis is sufficient for a duration measurement within
    // one request.
    chrono::Utc::now().timestamp_millis()
}

fn bytes_to_metric(bytes: i64, metric: &str) -> i64 {
    match metric {
        "KB" => (bytes as f64 / 1000.0).round() as i64,
        "MB" => (bytes as f64 / 1_000_000.0).round() as i64,
        "GB" => (bytes as f64 / 1_000_000_000.0).round() as i64,
        "TB" => (bytes as f64 / 1_000_000_000_000.0).round() as i64,
        _ => bytes,
    }
}
