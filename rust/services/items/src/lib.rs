//! Octy items service — Rust/WASM (Spin HTTP component) port of `items/`.
//!
//! Routes (unchanged from the FastAPI service):
//!   GET  /healthz
//!   GET  /v1/retention/items
//!   POST /v1/retention/items/create
//!   POST /v1/retention/items/update
//!   POST /v1/retention/items/delete
//!   GET  /v1/internal/items          (cluster-internal only)
//!   POST /v1/internal/items/delete   (cluster-internal only — account-deletion fan-out)
//!
//! The service has no AMQP consumers (it only publishes
//! `account.billing.cmd.capture` and `algo.configs.cmd.update`), so there is
//! no `/internal/amqp/consume` route and the per-service data-gateway
//! deployment omits the consumer environment.
//!
//! MongoDB / RabbitMQ are reached through the `octy-data-gateway` sidecar
//! over HTTP, because raw TCP drivers are unavailable inside the WASM
//! sandbox. This service does not use Redis or S3.

mod errors;
mod handlers;
mod http_util;
mod models;
mod repos;
mod services;

use spin_sdk::http::{Method, Request, Response, Router};
use spin_sdk::http_component;

#[http_component]
async fn handle(req: Request) -> Response {
    let mut router = Router::new();

    router.get_async("/healthz", handlers::healthz);
    router.get_async("/v1/retention/items", handlers::get_items);
    router.post_async("/v1/retention/items/create", handlers::create_items);
    router.post_async("/v1/retention/items/update", handlers::update_items);
    router.post_async("/v1/retention/items/delete", handlers::delete_items);
    router.get_async("/v1/internal/items", handlers::get_items_internal);
    router.post_async("/v1/internal/items/delete", handlers::delete_account_items_internal);

    router.any_async("/*", handlers::fallback);

    // The FastAPI app returned a dedicated envelope for 405s; routefinder only
    // knows path matches, so distinguish "known path, wrong method" here.
    let known: &[(&Method, &str)] = &[
        (&Method::Get, "/healthz"),
        (&Method::Get, "/v1/retention/items"),
        (&Method::Post, "/v1/retention/items/create"),
        (&Method::Post, "/v1/retention/items/update"),
        (&Method::Post, "/v1/retention/items/delete"),
        (&Method::Get, "/v1/internal/items"),
        (&Method::Post, "/v1/internal/items/delete"),
    ];
    let path = req.path().to_string();
    let method = req.method().clone();
    let path_known = known.iter().any(|(_, p)| *p == path);
    let method_matches = known.iter().any(|(m, p)| *p == path && **m == method);

    if path_known && !method_matches {
        return http_util::method_not_allowed();
    }

    router.handle_async(req).await
}
