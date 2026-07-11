//! Octy events service — Rust/WASM (Spin HTTP component) port of `events/`.
//!
//! Routes (unchanged from the FastAPI service):
//!   GET  /healthz
//!   POST /v1/retention/events/create
//!   POST /v1/retention/events/create/batch
//!   POST /v1/retention/events
//!   GET  /v1/retention/events/types
//!   POST /v1/retention/events/types/create
//!   POST /v1/retention/events/types/delete
//!   POST /v1/retention/events/types/delete/all
//!   POST /v1/internal/events              (cluster-internal only)
//!   POST /v1/internal/events/delete       (cluster-internal only)
//!   POST /v1/internal/events/types        (cluster-internal only)
//!   POST /internal/amqp/consume           (new: AMQP messages forwarded by the data gateway)
//!
//! MongoDB / RabbitMQ are reached through the `octy-data-gateway` sidecar
//! over HTTP, because raw TCP drivers are unavailable inside the WASM
//! sandbox. Outbound HTTP to the profile/segmentation services goes directly
//! from the component, exactly like the Python `requests_retry_session`.

mod amqp;
mod gateway_ext;
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
    router.post_async("/v1/retention/events/create", handlers::create_event_instance);
    router.post_async(
        "/v1/retention/events/create/batch",
        handlers::batch_create_event_instances,
    );
    router.post_async(
        "/v1/retention/events",
        handlers::get_latest_checkout_info_submmited_event,
    );
    router.get_async("/v1/retention/events/types", handlers::get_custom_event_types);
    router.post_async(
        "/v1/retention/events/types/create",
        handlers::create_custom_event_types,
    );
    router.post_async(
        "/v1/retention/events/types/delete",
        handlers::delete_custom_event_types,
    );
    router.post_async(
        "/v1/retention/events/types/delete/all",
        handlers::delete_all_custom_event_types,
    );
    router.post_async("/v1/internal/events", handlers::get_events_internal);
    router.post_async("/v1/internal/events/delete", handlers::delete_events_internal);
    router.post_async("/v1/internal/events/types", handlers::get_event_types_internal);
    router.post_async("/internal/amqp/consume", handlers::amqp_consume);

    router.any_async("/*", handlers::fallback);

    // The FastAPI app returned a dedicated envelope for 405s; routefinder only
    // knows path matches, so distinguish "known path, wrong method" here.
    let known: &[(&Method, &str)] = &[
        (&Method::Get, "/healthz"),
        (&Method::Post, "/v1/retention/events/create"),
        (&Method::Post, "/v1/retention/events/create/batch"),
        (&Method::Post, "/v1/retention/events"),
        (&Method::Get, "/v1/retention/events/types"),
        (&Method::Post, "/v1/retention/events/types/create"),
        (&Method::Post, "/v1/retention/events/types/delete"),
        (&Method::Post, "/v1/retention/events/types/delete/all"),
        (&Method::Post, "/v1/internal/events"),
        (&Method::Post, "/v1/internal/events/delete"),
        (&Method::Post, "/v1/internal/events/types"),
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
