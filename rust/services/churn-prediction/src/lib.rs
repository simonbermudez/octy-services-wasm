//! Octy churn_prediction service — Rust/WASM (Spin HTTP component) port of
//! `churn_prediction/`.
//!
//! Routes (unchanged from the FastAPI service):
//!   GET  /healthz
//!   GET  /v1/retention/churn_prediction/report
//!   POST /v1/internal/churn_prediction/delete
//!
//! MongoDB is reached through the `octy-data-gateway` sidecar over HTTP
//! (raw TCP drivers are unavailable inside the WASM sandbox). The service
//! has no AMQP consumers/publishers, no Redis and no S3 usage.
//!
//! Rate limits: the FastAPI service used slowapi (`120/minute` on the report
//! route). Spin components are stateless per request, so enforce those limits
//! at the ingress/gateway layer.

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
    router.get_async("/v1/retention/churn_prediction/report", handlers::get_churn_report);
    router.post_async(
        "/v1/internal/churn_prediction/delete",
        handlers::delete_churn_prediction_internal,
    );

    router.any_async("/*", handlers::fallback);

    // The FastAPI app returned a dedicated envelope for 405s; routefinder only
    // knows path matches, so distinguish "known path, wrong method" here.
    let known: &[(&Method, &str)] = &[
        (&Method::Get, "/healthz"),
        (&Method::Get, "/v1/retention/churn_prediction/report"),
        (&Method::Post, "/v1/internal/churn_prediction/delete"),
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
