//! Octy messaging service — Rust/WASM (Spin HTTP component) port of `messaging/`.
//!
//! Routes (unchanged from the FastAPI service):
//!   GET  /healthz
//!   GET  /v1/retention/messaging/templates
//!   POST /v1/retention/messaging/templates/create
//!   POST /v1/retention/messaging/templates/update
//!   POST /v1/retention/messaging/templates/delete
//!   POST /v1/retention/messaging/content/generate
//!   POST /v1/internal/messaging/delete
//!
//! MongoDB is reached through the `octy-data-gateway` sidecar over HTTP
//! (`tbl_templates` / `tbl_currency_rates`); the recommendation, items and
//! Rybbon reward-card APIs are called directly from the component via Spin's
//! outbound HTTP capability. The Python service used no Redis and no AMQP
//! publish/consume — none is wired up here either (see the port report).

mod currency;
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
    router.get_async("/v1/retention/messaging/templates", handlers::get_templates);
    router.post_async(
        "/v1/retention/messaging/templates/create",
        handlers::create_templates,
    );
    router.post_async(
        "/v1/retention/messaging/templates/update",
        handlers::update_templates,
    );
    router.post_async(
        "/v1/retention/messaging/templates/delete",
        handlers::delete_templates,
    );
    router.post_async(
        "/v1/retention/messaging/content/generate",
        handlers::generate_content,
    );
    router.post_async(
        "/v1/internal/messaging/delete",
        handlers::delete_messaging_internal,
    );

    router.any_async("/*", |_req: Request, _params: spin_sdk::http::Params| async move {
        http_util::not_found()
    });

    // The FastAPI app returned a dedicated envelope for 405s; routefinder only
    // knows path matches, so distinguish "known path, wrong method" here.
    let known: &[(&Method, &str)] = &[
        (&Method::Get, "/healthz"),
        (&Method::Get, "/v1/retention/messaging/templates"),
        (&Method::Post, "/v1/retention/messaging/templates/create"),
        (&Method::Post, "/v1/retention/messaging/templates/update"),
        (&Method::Post, "/v1/retention/messaging/templates/delete"),
        (&Method::Post, "/v1/retention/messaging/content/generate"),
        (&Method::Post, "/v1/internal/messaging/delete"),
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
