//! Octy recommendation service — Rust/WASM (Spin HTTP component) port of
//! `recommendation/`.
//!
//! Routes (unchanged from the FastAPI service):
//!   GET  /healthz
//!   POST /v1/retention/recommendations
//!   POST /v1/internal/recommendations          (cluster-internal only)
//!   POST /v1/internal/recommendations/delete   (cluster-internal only)
//!   POST /internal/amqp/consume   (AMQP deliveries forwarded by the data gateway)
//!
//! MongoDB / RabbitMQ are reached through the `octy-data-gateway` sidecar
//! over HTTP, because raw TCP drivers are unavailable inside the WASM
//! sandbox. This service uses no Redis, S3 or SageMaker.

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
    router.post_async("/v1/retention/recommendations", handlers::get_recommendations);
    router.post_async("/v1/internal/recommendations", handlers::get_recommendations_internal);
    router.post_async(
        "/v1/internal/recommendations/delete",
        handlers::delete_account_recommendations,
    );
    router.post_async("/internal/amqp/consume", handlers::amqp_consume);

    router.any_async("/*", handlers::fallback);

    // The FastAPI app returned a dedicated envelope for 405s; routefinder only
    // knows path matches, so distinguish "known path, wrong method" here.
    let known: &[(&Method, &str)] = &[
        (&Method::Get, "/healthz"),
        (&Method::Post, "/v1/retention/recommendations"),
        (&Method::Post, "/v1/internal/recommendations"),
        (&Method::Post, "/v1/internal/recommendations/delete"),
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
