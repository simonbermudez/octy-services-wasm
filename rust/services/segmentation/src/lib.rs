//! Octy segmentation service — Rust/WASM (Spin HTTP component) port of
//! `segmentation/`.
//!
//! Routes (unchanged from the FastAPI service):
//!   GET  /healthz
//!   GET  /v1/retention/segments
//!   POST /v1/retention/segments/create
//!   POST /v1/retention/segments/delete
//!   GET  /v1/internal/segments
//!   POST /v1/internal/segments/delete
//!   POST /internal/amqp/consume   (new: AMQP messages forwarded by the data gateway)
//!
//! MongoDB / RabbitMQ are reached through the `octy-data-gateway` sidecar
//! over HTTP (raw TCP drivers are unavailable inside the WASM sandbox); the
//! events-service lookup goes out directly via Spin's outbound HTTP.

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
    router.get_async("/v1/retention/segments", handlers::get_segments);
    router.post_async("/v1/retention/segments/create", handlers::create_segments);
    router.post_async("/v1/retention/segments/delete", handlers::delete_segments);
    router.get_async("/v1/internal/segments", handlers::get_segments_internal);
    router.post_async("/v1/internal/segments/delete", handlers::delete_segments_internal);
    router.post_async("/internal/amqp/consume", handlers::amqp_consume);

    router.any_async("/*", handlers::fallback);

    // The FastAPI app returned a dedicated envelope for 405s; routefinder only
    // knows path matches, so distinguish "known path, wrong method" here.
    let known: &[(&Method, &str)] = &[
        (&Method::Get, "/healthz"),
        (&Method::Get, "/v1/retention/segments"),
        (&Method::Post, "/v1/retention/segments/create"),
        (&Method::Post, "/v1/retention/segments/delete"),
        (&Method::Get, "/v1/internal/segments"),
        (&Method::Post, "/v1/internal/segments/delete"),
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
