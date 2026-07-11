//! Octy profiles — Rust/WASM (Spin HTTP component) port of `profiles/`.
//!
//! Routes (unchanged from the FastAPI service):
//!   GET  /healthz
//!   GET  /v1/retention/profiles
//!   POST /v1/retention/profiles/create
//!   POST /v1/retention/profiles/update
//!   POST /v1/retention/profiles/delete
//!   GET  /v1/retention/profiles/metadata
//!   POST /v1/internal/profiles           (cluster-internal only)
//!   POST /v1/internal/profiles/delete    (cluster-internal only — account deletion fan-out)
//!   POST /internal/amqp/consume          (new: AMQP messages forwarded by the data gateway)
//!
//! MongoDB / RabbitMQ are reached through the `octy-data-gateway` sidecar
//! over HTTP, because raw TCP drivers are unavailable inside the WASM
//! sandbox. Redis (the `{account_id}_profile_key_types` cache, db index 1)
//! is reached directly via Spin's outbound-Redis host capability.

mod amqp;
mod gateway_ext;
mod handlers;
mod http_util;
mod models;
mod repos;
mod services;
mod utils;

use spin_sdk::http::{Method, Request, Response, Router};
use spin_sdk::http_component;

#[http_component]
async fn handle(req: Request) -> Response {
    let mut router = Router::new();

    router.get_async("/healthz", handlers::healthz);
    router.get_async("/v1/retention/profiles", handlers::get_customer_profiles);
    router.post_async("/v1/retention/profiles/create", handlers::create_customer_profiles);
    router.post_async("/v1/retention/profiles/update", handlers::update_customer_profiles);
    router.post_async("/v1/retention/profiles/delete", handlers::delete_customer_profiles);
    router.get_async("/v1/retention/profiles/metadata", handlers::get_profiles_meta);
    router.post_async("/v1/internal/profiles", handlers::get_profiles_internal);
    router.post_async("/v1/internal/profiles/delete", handlers::delete_account_profiles);
    router.post_async("/internal/amqp/consume", handlers::amqp_consume);

    router.any_async("/*", handlers::fallback);

    // The FastAPI app returned a dedicated envelope for 405s; routefinder only
    // knows path matches, so distinguish "known path, wrong method" here.
    let known: &[(&Method, &str)] = &[
        (&Method::Get, "/healthz"),
        (&Method::Get, "/v1/retention/profiles"),
        (&Method::Post, "/v1/retention/profiles/create"),
        (&Method::Post, "/v1/retention/profiles/update"),
        (&Method::Post, "/v1/retention/profiles/delete"),
        (&Method::Get, "/v1/retention/profiles/metadata"),
        (&Method::Post, "/v1/internal/profiles"),
        (&Method::Post, "/v1/internal/profiles/delete"),
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
