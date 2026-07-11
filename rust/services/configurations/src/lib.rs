//! Octy configurations service — Rust/WASM (Spin HTTP component) port of
//! `configurations/`.
//!
//! Routes (unchanged from the FastAPI service):
//!   GET  /healthz
//!   POST /v1/configurations/account/set
//!   GET  /v1/configurations/account
//!   POST /v1/configurations/retention/algorithms/set
//!   GET  /v1/configurations/retention/algorithms
//!
//! The service has no MongoDB access and no AMQP consumers: both repositories
//! publish update commands to RabbitMQ (through the `octy-data-gateway`
//! sidecar's `/v1/amqp/publish`), and the algorithm repository calls the
//! items service's internal HTTP endpoint directly.

mod handlers;
mod http_util;
mod models;
mod repos;

use spin_sdk::http::{Method, Request, Response, Router};
use spin_sdk::http_component;

#[http_component]
async fn handle(req: Request) -> Response {
    let mut router = Router::new();

    router.get_async("/healthz", handlers::healthz);
    router.post_async("/v1/configurations/account/set", handlers::set_account_configs);
    router.get_async("/v1/configurations/account", handlers::get_account_configs);
    router.post_async(
        "/v1/configurations/retention/algorithms/set",
        handlers::set_algorithm_configs,
    );
    router.get_async(
        "/v1/configurations/retention/algorithms",
        handlers::get_algorithm_configs,
    );

    router.any_async("/*", handlers::fallback);

    // The FastAPI app returned a dedicated envelope for 405s; routefinder only
    // knows path matches, so distinguish "known path, wrong method" here.
    let known: &[(&Method, &str)] = &[
        (&Method::Get, "/healthz"),
        (&Method::Post, "/v1/configurations/account/set"),
        (&Method::Get, "/v1/configurations/account"),
        (&Method::Post, "/v1/configurations/retention/algorithms/set"),
        (&Method::Get, "/v1/configurations/retention/algorithms"),
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
