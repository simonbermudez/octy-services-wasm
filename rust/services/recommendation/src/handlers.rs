//! Route handlers — port of `api/routers/recommendation.py` / `healthz.py`.
//!
//! Note on rate limits: the FastAPI service used slowapi (`120/minute` on
//! both recommendations routes). Spin components are stateless per request,
//! so enforce those limits at the ingress/gateway layer.

use octy_shared::errors::{ErrorReason, OctyError};
use serde_json::json;
use spin_sdk::http::{Params, Request, Response};

use octy_spin::auth::decode_account_jwt;
use octy_spin::ctx::Ctx;

use crate::amqp;
use crate::http_util::*;
use crate::models::{DeleteAccountRecommendations, GetRecomendations, GetRecomendationsInternal};
use crate::services::recommendation as recommendation_service;

fn ctx_or_response() -> Result<Ctx, Response> {
    Ctx::load("recommendation").map_err(|e| error_response(&e))
}

/// The router-level `MAX_REC_PREDICTIONS` guard (400). On the external route
/// it is unreachable — the pydantic validator (422) fires first at the same
/// threshold — but it is live on the internal route, whose request model has
/// no validator. Both quirks preserved from the Python.
fn limit_exceeded_error(ctx: &Ctx, max: i64) -> OctyError {
    OctyError::new(
        400,
        "Recommendation request limit exceeded.",
        vec![ErrorReason::new(
            format!("A maximum number of {max} profile ids per recommendations request allowed."),
            ctx.config.opt_str("RATE_LIMIT_EXTENDED_HELP").unwrap_or(""),
        )],
    )
}

/// GET /healthz — k8s liveness/readiness probe. No auth, no rate limit.
pub async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &json!("OK"))
}

/// POST /v1/retention/recommendations — latest cached item recommendations
/// for up to `MAX_REC_PREDICTIONS` profile ids. Auth: X-AUTH-JWT.
pub async fn get_recommendations(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let errors_help = ctx.config.opt_str("ERRORS_OVERVIEW_EXTENDED_HELP").unwrap_or("").to_string();
    if let Err(err) = validate_post_headers(&req, &errors_help) {
        return error_response(&err);
    }

    let account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return error_response(&err),
    };

    let max = match ctx.config.get_i64("MAX_REC_PREDICTIONS") {
        Ok(max) => max,
        Err(err) => return error_response(&err),
    };

    let body = match GetRecomendations::from_json(req.body(), max) {
        Ok(body) => body,
        Err(err) => return error_response(&err),
    };

    if body.profile_ids.len() as i64 > max {
        return error_response(&limit_exceeded_error(&ctx, max));
    }

    let account_id = account.account_oid().unwrap_or_default().to_string();
    match recommendation_service::get_recommendations(&ctx, &account_id, &body.profile_ids).await {
        Ok((recommendations, meta)) => get_recommendations_dto(&recommendations, &meta),
        Err(err) => error_response(&err),
    }
}

/// POST /v1/internal/recommendations  (cluster-internal only — keep off the
/// ingress). Same as the external route but the account_id comes in the body.
pub async fn get_recommendations_internal(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let errors_help = ctx.config.opt_str("ERRORS_OVERVIEW_EXTENDED_HELP").unwrap_or("").to_string();
    if let Err(err) = validate_post_headers(&req, &errors_help) {
        return error_response(&err);
    }

    let body = match GetRecomendationsInternal::from_json(req.body()) {
        Ok(body) => body,
        Err(err) => return error_response(&err),
    };

    let max = match ctx.config.get_i64("MAX_REC_PREDICTIONS") {
        Ok(max) => max,
        Err(err) => return error_response(&err),
    };
    if body.profile_ids.len() as i64 > max {
        return error_response(&limit_exceeded_error(&ctx, max));
    }

    match recommendation_service::get_recommendations(&ctx, &body.account_id, &body.profile_ids).await
    {
        Ok((recommendations, meta)) => get_recommendations_dto(&recommendations, &meta),
        Err(err) => error_response(&err),
    }
}

/// POST /v1/internal/recommendations/delete  (cluster-internal only) —
/// account-deletion fan-out: delete every recommendation for an account.
/// NB: no `validate_post_headers` dependency on this route in the Python.
pub async fn delete_account_recommendations(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let body = match DeleteAccountRecommendations::from_json(req.body()) {
        Ok(body) => body,
        Err(err) => return error_response(&err),
    };

    let is_deleted =
        recommendation_service::delete_account_recommendations(&ctx, &body.account_id).await;
    delete_account_recommendations_dto(is_deleted)
}

/// POST /internal/amqp/consume — deliveries forwarded by the data gateway.
pub async fn amqp_consume(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let outcome = amqp::handle_delivery(&ctx, req.body()).await;
    json_response(outcome.status, &json!({ "detail": outcome.detail }))
}

pub async fn fallback(_req: Request, _params: Params) -> Response {
    not_found()
}
