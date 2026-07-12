//! Octy admin service — Rust/WASM (Spin HTTP component) port of `admin/`
//! **and** `configurations/`, merged into one deployable.
//!
//! The two source services shared no functional overlap (admin is
//! Redis-only version tracking + a GitHub webhook; configurations is
//! JWT-authenticated account/algorithm config updates published to AMQP) —
//! they were merged purely to cut the deployable count for two services too
//! small to justify independent scaling or failure isolation (WASM's
//! per-request sandboxing already isolates a bug in one route group from the
//! other at a finer grain than a separate microservice would). Nothing about
//! either service's behavior, config schema, or route paths changed — see
//! `configurations/` submodule for the folded-in service, still reading its
//! own `configurations_config`/`configurations_secrets` variables.
//!
//! Routes (unchanged from the two FastAPI services):
//!   GET  /healthz
//!   GET  /v1/admin/application/versioning
//!   POST /v1/admin/application/versioning/hook
//!   GET  /v1/admin/application/resources/format
//!   POST /v1/configurations/account/set
//!   GET  /v1/configurations/account
//!   POST /v1/configurations/retention/algorithms/set
//!   GET  /v1/configurations/retention/algorithms
//!
//! admin's routes talk only to Redis, reached directly via Spin's
//! outbound-Redis host support. configurations' routes talk only to the
//! octy-data-gateway sidecar (AMQP publish) and, for `rec` algorithm updates,
//! the items service's internal HTTP endpoint.

mod configurations;
mod handlers;
mod http_util;
mod repos;

use spin_sdk::http::{Method, Request, Response, Router};
use spin_sdk::http_component;

#[http_component]
async fn handle(req: Request) -> Response {
    let mut router = Router::new();

    router.get_async("/healthz", handlers::healthz);
    router.get_async("/v1/admin/application/versioning", handlers::version_info);
    router.post_async("/v1/admin/application/versioning/hook", handlers::version_info_hook);
    router.get_async("/v1/admin/application/resources/format", handlers::resource_format);

    router.post_async(
        "/v1/configurations/account/set",
        configurations::handlers::set_account_configs,
    );
    router.get_async(
        "/v1/configurations/account",
        configurations::handlers::get_account_configs,
    );
    router.post_async(
        "/v1/configurations/retention/algorithms/set",
        configurations::handlers::set_algorithm_configs,
    );
    router.get_async(
        "/v1/configurations/retention/algorithms",
        configurations::handlers::get_algorithm_configs,
    );

    router.any_async("/*", handlers::fallback);

    // The FastAPI apps each returned a dedicated envelope for 405s; routefinder
    // only knows path matches, so distinguish "known path, wrong method" here.
    let known: &[(&Method, &str)] = &[
        (&Method::Get, "/healthz"),
        (&Method::Get, "/v1/admin/application/versioning"),
        (&Method::Post, "/v1/admin/application/versioning/hook"),
        (&Method::Get, "/v1/admin/application/resources/format"),
        (&Method::Post, "/v1/configurations/account/set"),
        (&Method::Get, "/v1/configurations/account"),
        (&Method::Post, "/v1/configurations/retention/algorithms/set"),
        (&Method::Get, "/v1/configurations/retention/algorithms"),
    ];
    let path = req.path().to_string();
    let method = req.method().clone();
    let path_known = known.iter().any(|(_, p)| *p == path);
    let method_matches = known
        .iter()
        .any(|(m, p)| *p == path && **m == method);

    if path_known && !method_matches {
        return http_util::method_not_allowed();
    }

    router.handle_async(req).await
}
