//! Octy segmentation_worker — Rust/WASM (Spin HTTP component) port of
//! `workers/segmentation/`.
//!
//! Routes:
//!   GET  /healthz
//!   POST /internal/amqp/consume   (AMQP deliveries forwarded by the data gateway)
//!
//! The original was a FastAPI app whose only externally meaningful job was
//! consuming two AMQP routing keys (`past.segmentation.cmd.run`,
//! `live.segmentation.cmd.run`) and running the segmentation pipeline to
//! completion inside the delivery handler; `/healthz` was its only HTTP
//! route. Both are preserved here, with job execution now running
//! synchronously inside the `/internal/amqp/consume` request (see `amqp.rs`).

mod amqp;
mod billing;
mod engine;
mod models;
mod pyval;
mod repository;

use octy_spin::ctx::Ctx;
use octy_spin::http_util::{error_response, json_response, not_found};
use spin_sdk::http::{Params, Request, Response, Router};
use spin_sdk::http_component;

#[http_component]
async fn handle(req: Request) -> Response {
    let mut router = Router::new();
    router.get_async("/healthz", healthz);
    router.post_async("/internal/amqp/consume", amqp_consume);
    router.any_async("/*", |_req: Request, _params: Params| async move { not_found() });
    router.handle_async(req).await
}

async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &serde_json::json!("OK"))
}

async fn amqp_consume(req: Request, _params: Params) -> Response {
    let ctx = match Ctx::load("segmentation_worker") {
        Ok(ctx) => ctx,
        Err(err) => return error_response(&err),
    };
    let outcome = amqp::handle_delivery(&ctx, req.body()).await;
    json_response(outcome.status, &serde_json::json!({ "detail": outcome.detail }))
}
