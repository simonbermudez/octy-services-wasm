//! Octy admin service — Rust/WASM (Spin HTTP component) port of `admin/`.
//!
//! Routes (unchanged from the FastAPI service):
//!   GET  /healthz
//!   GET  /v1/admin/application/versioning
//!   POST /v1/admin/application/versioning/hook
//!   GET  /v1/admin/application/resources/format
//!
//! The admin service is Redis-only: version metadata for the `octy-services`
//! and `octy-cli` GitHub repositories is cached in Redis sets (db 0) and
//! served back to trusted applications. It has no MongoDB collections, no
//! AMQP consumers/publishers and no S3 buckets, so — unlike the other
//! services — it never talks to the octy-data-gateway sidecar. Redis is
//! reached directly via Spin's outbound-Redis host support.

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

    router.any_async("/*", handlers::fallback);

    // The FastAPI app returned a dedicated envelope for 405s; routefinder only
    // knows path matches, so distinguish "known path, wrong method" here.
    let known: &[(&Method, &str)] = &[
        (&Method::Get, "/healthz"),
        (&Method::Get, "/v1/admin/application/versioning"),
        (&Method::Post, "/v1/admin/application/versioning/hook"),
        (&Method::Get, "/v1/admin/application/resources/format"),
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
