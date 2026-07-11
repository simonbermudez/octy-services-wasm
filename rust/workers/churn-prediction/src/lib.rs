//! Octy churn_prediction_worker — Rust/WASM (Spin HTTP component) port of
//! `workers/churn_prediction/`.
//!
//! Routes:
//!   GET  /healthz
//!   POST /internal/amqp/consume   (deliveries forwarded by the data gateway;
//!                                  job execution runs synchronously inside
//!                                  this request — see `amqp.rs`)
//!
//! Consumed routing keys: `churn.training.cmd.run`, `churn.training.complete.cmd.run`.
//! Published routing keys: `octy.job.cmd.create`, `octy.job.cmd.delete`,
//! `profiles.cmd.update`, `churn.info.cmd.update`, `account.billing.cmd.capture`.
//!
//! See `pipeline_training.rs` / `pipeline_complete.rs` for the full pipeline
//! port and the Python bugs/artifact-format changes preserved or documented
//! along the way.

mod amqp;
mod billing;
mod bucket;
mod encode;
mod frame;
mod kmeans;
mod knee;
mod models;
mod pipeline_complete;
mod pipeline_training;
mod repos;
mod sagemaker;
mod util;
mod xgb;

use octy_spin::ctx::Ctx;
use spin_sdk::http::{Params, Request, Response, Router};
use spin_sdk::http_component;

#[http_component]
async fn handle(req: Request) -> Response {
    let mut router = Router::new();
    router.get_async("/healthz", healthz);
    router.post_async("/internal/amqp/consume", amqp_consume);
    router.any_async("/*", fallback);
    router.handle_async(req).await
}

async fn healthz(_req: Request, _params: Params) -> Response {
    octy_spin::http_util::json_response(200, &serde_json::json!("OK"))
}

/// POST /internal/amqp/consume — deliveries forwarded by the data gateway.
/// Response-code contract with the gateway: 2xx ack, 4xx reject-no-requeue,
/// 5xx reject+requeue (see `amqp.rs`).
async fn amqp_consume(req: Request, _params: Params) -> Response {
    let ctx = match Ctx::load("churn_prediction_worker") {
        Ok(ctx) => ctx,
        Err(err) => return octy_spin::http_util::error_response(&err),
    };
    let outcome = amqp::handle_delivery(&ctx, req.body()).await;
    octy_spin::http_util::json_response(outcome.status, &serde_json::json!({ "detail": outcome.detail }))
}

async fn fallback(_req: Request, _params: Params) -> Response {
    octy_spin::http_util::not_found()
}
