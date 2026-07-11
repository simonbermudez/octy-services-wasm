//! Octy recommendation_worker — Rust/WASM (Spin HTTP component) port of
//! `workers/recommendation/`.
//!
//! Routes:
//!   GET  /healthz
//!   POST /internal/amqp/consume   (AMQP deliveries forwarded by the data gateway;
//!                                  job execution runs synchronously inside the request)
//!
//! MongoDB / RabbitMQ / S3 are reached through the `octy-data-gateway`
//! sidecar over HTTP (raw TCP drivers are unavailable inside the WASM
//! sandbox). SageMaker is reached with SigV4-signed HTTPS requests straight
//! from the component. Expose `/internal/amqp/consume` only inside the
//! cluster, same as the Python internal endpoints.

mod amqp;
mod artifacts;
mod billing;
mod frame;
mod http;
mod models;
mod repos;
mod sagemaker;
mod services;
mod tar_gz;
mod utils;

use spin_sdk::http::{Params, Request, Response, Router};
use spin_sdk::http_component;

use octy_spin::ctx::Ctx;
use octy_spin::http_util::json_response;

fn load_ctx() -> Result<Ctx, Response> {
    Ctx::load("recommendation_worker").map_err(|err| {
        eprintln!("[recommendation-worker] {err}");
        json_response(500, &serde_json::json!({ "detail": "failed to load worker context" }))
    })
}

async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &serde_json::json!("OK"))
}

async fn amqp_consume(req: Request, _params: Params) -> Response {
    let ctx = match load_ctx() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };
    let outcome = amqp::handle_delivery(&ctx, req.body()).await;
    json_response(outcome.status, &serde_json::json!({ "detail": outcome.detail }))
}

async fn fallback(_req: Request, _params: Params) -> Response {
    octy_spin::http_util::not_found()
}

#[http_component]
async fn handle(req: Request) -> Response {
    let mut router = Router::new();
    router.get_async("/healthz", healthz);
    router.post_async("/internal/amqp/consume", amqp_consume);
    router.any_async("/*", fallback);
    router.handle_async(req).await
}
