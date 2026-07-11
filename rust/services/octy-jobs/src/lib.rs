//! Octy jobs service — Rust/WASM (Spin HTTP component) port of `octy_jobs/`.
//!
//! Routes (unchanged from the FastAPI service unless noted):
//!   GET  /healthz
//!   POST /v1/internal/jobs/callback   (workers update a job's status)
//!   POST /v1/internal/jobs/delete     (account-deletion fan-out)
//!   POST /internal/amqp/consume       (new: AMQP deliveries forwarded by the data gateway —
//!                                      octy.job.cmd.create / octy.job.cmd.delete)
//!   POST /internal/scheduler/tick     (new: one OctyJobQueue._process_octy_jobs pass,
//!                                      driven by a Kubernetes CronJob every 2 minutes)
//!
//! MongoDB / RabbitMQ are reached through the `octy-data-gateway` sidecar
//! over HTTP (raw TCP drivers are unavailable inside the WASM sandbox).
//! Redis (job-claim keys, db 3) and the account-service internal HTTP calls
//! go directly from the component.

mod amqp;
mod gw;
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
    router.post_async("/v1/internal/jobs/callback", handlers::octy_job_callback);
    router.post_async("/v1/internal/jobs/delete", handlers::delete_account_jobs);
    router.post_async("/internal/amqp/consume", handlers::amqp_consume);
    router.post_async("/internal/scheduler/tick", handlers::scheduler_tick);

    router.any_async("/*", handlers::fallback);

    // The FastAPI app returned a dedicated envelope for 405s; routefinder only
    // knows path matches, so distinguish "known path, wrong method" here.
    let known: &[(&Method, &str)] = &[
        (&Method::Get, "/healthz"),
        (&Method::Post, "/v1/internal/jobs/callback"),
        (&Method::Post, "/v1/internal/jobs/delete"),
        (&Method::Post, "/internal/amqp/consume"),
        (&Method::Post, "/internal/scheduler/tick"),
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
