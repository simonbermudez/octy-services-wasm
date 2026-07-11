//! Octy billing service — Rust/WASM (Spin HTTP component) port of `billing/`.
//!
//! Routes (unchanged from the FastAPI service):
//!   GET  /healthz
//!   GET  /v1/admin/billing/units
//!   GET  /v1/admin/billing/subscriptions
//!   POST /v1/internal/billing/delete
//!   POST /internal/amqp/consume   (new: AMQP messages forwarded by the data gateway)
//!
//! MongoDB / RabbitMQ are reached through the `octy-data-gateway` sidecar
//! over HTTP, because raw TCP drivers are unavailable inside the WASM
//! sandbox. The billing service uses neither Redis nor outbound third-party
//! HTTP.

mod amqp;
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
    router.get_async("/v1/admin/billing/units", handlers::get_billable_units);
    router.get_async("/v1/admin/billing/subscriptions", handlers::get_subscription_plans);
    router.post_async("/v1/internal/billing/delete", handlers::delete_billing_internal);
    router.post_async("/internal/amqp/consume", handlers::amqp_consume);

    router.any_async("/*", handlers::fallback);

    // The FastAPI app returned a dedicated envelope for 405s; routefinder only
    // knows path matches, so distinguish "known path, wrong method" here.
    let known: &[(&Method, &str)] = &[
        (&Method::Get, "/healthz"),
        (&Method::Get, "/v1/admin/billing/units"),
        (&Method::Get, "/v1/admin/billing/subscriptions"),
        (&Method::Post, "/v1/internal/billing/delete"),
        (&Method::Post, "/internal/amqp/consume"),
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
