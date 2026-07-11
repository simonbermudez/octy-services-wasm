//! Octy rfm_worker — Rust/WASM (Spin HTTP component) port of `workers/rfm/`.
//!
//! Routes:
//!   GET  /healthz
//!   POST /internal/amqp/consume   (AMQP deliveries forwarded by octy-data-gateway;
//!                                  job execution runs synchronously in the request)
//!
//! MongoDB (`tbl_training_jobs`), RabbitMQ publish and S3 object storage are
//! reached through the `octy-data-gateway` sidecar over HTTP (raw TCP
//! drivers are unavailable inside the WASM sandbox). The internal
//! event/profile/item/job services and SageMaker are called directly
//! (plain outbound HTTPS / SigV4-signed requests) from the component, same
//! as the Python service's `requests`/`boto3` calls.

mod amqp;
mod billing;
mod complete;
mod csv_util;
// Standalone, independently unit-tested KMeans/Kneedle building blocks (see
// the module docs in `ml.rs` for why they are not currently wired into the
// live pipeline — the real RFM clustering runs in an external, opaque
// SageMaker container). Not dead code from a project standpoint, just not
// yet a caller inside this crate.
#[allow(dead_code)]
mod ml;
mod models;
mod rfm_repository;
mod s3;
mod sagemaker;
mod training;
mod util;

use spin_sdk::http::{Params, Request, Response, Router};
use spin_sdk::http_component;

use octy_spin::ctx::Ctx;
use octy_spin::http_util::{error_response, json_response, not_found};

async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &serde_json::json!("OK"))
}

async fn amqp_consume(req: Request, _params: Params) -> Response {
    let ctx = match Ctx::load("rfm_worker") {
        Ok(ctx) => ctx,
        Err(e) => return error_response(&e),
    };
    let outcome = amqp::handle_delivery(&ctx, req.body()).await;
    json_response(outcome.status, &serde_json::json!({ "detail": outcome.detail }))
}

async fn fallback(_req: Request, _params: Params) -> Response {
    not_found()
}

#[http_component]
async fn handle(req: Request) -> Response {
    let mut router = Router::new();
    router.get_async("/healthz", healthz);
    router.post_async("/internal/amqp/consume", amqp_consume);
    router.any_async("/*", fallback);
    router.handle_async(req).await
}
