//! Octy profile_identification_worker — Rust/WASM (Spin HTTP component) port
//! of `workers/profile_identification/`.
//!
//! Handles identity resolution: merging authenticated and anonymous
//! customer profiles that share an account-configured `authenticated_id_key`
//! value (e.g. an email address captured post-login on a profile that was
//! first created anonymously).
//!
//! Routes:
//!   GET  /healthz
//!   POST /internal/amqp/consume   (AMQP deliveries forwarded by the data
//!                                  gateway; job execution runs
//!                                  synchronously inside the request)
//!
//! MongoDB / RabbitMQ are reached through the `octy-data-gateway` sidecar
//! over HTTP (raw TCP drivers are unavailable inside the WASM sandbox). The
//! `profiles` and `octy-jobs` services are reached directly over outbound
//! HTTP (same as the Python's `requests_retry_session` calls), and the
//! per-account profile-key-type cache is read directly from Redis via
//! Spin's outbound-Redis host capability. Expose `/internal/amqp/consume`
//! only inside the cluster, same as the Python internal endpoints.

mod amqp;
mod billing;
mod http;
mod job;
mod matching;
mod models;
mod repos;

use spin_sdk::http::{Params, Request, Response, Router};
use spin_sdk::http_component;

use octy_spin::ctx::Ctx;
use octy_spin::http_util::json_response;

fn load_ctx() -> Result<Ctx, Response> {
    Ctx::load("profile_identification_worker").map_err(|err| {
        eprintln!("[profile-identification-worker] {err}");
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
