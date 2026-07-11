//! Route handlers — port of `api/routers/segmentation.py` and `healthz.py`.
//!
//! Note on rate limits: the FastAPI service used slowapi (`120/minute` on the
//! retention routes). Spin components are stateless per request, so enforce
//! those limits at the ingress/gateway layer.

use octy_shared::errors::{ErrorReason, OctyError};
use serde_json::{json, Value};
use spin_sdk::http::{Params, Request, Response};

use octy_spin::auth::decode_account_jwt;
use octy_spin::ctx::Ctx;

use crate::amqp;
use crate::http_util::*;
use crate::models::{self, CreateSegment, DeleteAccountSegmentations, DeleteSegments};
use crate::services::segmentation as service;

fn ctx_or_response() -> Result<Ctx, Response> {
    Ctx::load("segmentation").map_err(|e| error_response(&e))
}

pub async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &json!("OK"))
}

/// GET /v1/retention/segments?ids=<segment_id(s) | segment_name(s)>,…
pub async fn get_segments(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };
    let account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return error_response(&err),
    };
    let account_id = account.account_oid().unwrap_or_default().to_string();

    let ids = query_param(&req, "ids");
    let mut cursor: i64 = 0;
    let identifiers: Option<Vec<String>>;

    match &ids {
        None => {
            // Validate pagination headers set.
            match validate_pagination_request(&req, None) {
                Ok(Some(c)) => cursor = c,
                Ok(None) => {}
                Err(pag_message) => {
                    return error_response(&OctyError::new(
                        400,
                        "Missing Parameters",
                        vec![ErrorReason::new(
                            pag_message,
                            ctx.config.opt_str("SEGMENTATION_EXTENDED_HELP").unwrap_or(""),
                        )],
                    ))
                }
            }
            identifiers = None;
        }
        Some(raw) => {
            // split, drop empties, dedupe (order kept), then trim — the trim
            // runs *after* dedupe, exactly like the Python.
            let mut deduped: Vec<&str> = Vec::new();
            for part in raw.split(',') {
                if !part.is_empty() && !deduped.contains(&part) {
                    deduped.push(part);
                }
            }
            let list: Vec<String> = deduped.iter().map(|s| s.trim().to_string()).collect();

            let max_get = match ctx.config.get_i64("MAX_GET_SEGMENTS") {
                Ok(max) => max,
                Err(err) => return error_response(&err),
            };
            if list.len() as i64 > max_get {
                return error_response(&OctyError::new(
                    400,
                    "Invalid Parameters",
                    vec![ErrorReason::new(
                        format!(
                            "A maximum number of {max_get} identifiers can be provided with the \"?ids=\" query param per request"
                        ),
                        ctx.config.opt_str("SEGMENTATION_EXTENDED_HELP").unwrap_or(""),
                    )],
                ));
            }
            identifiers = Some(list);
        }
    }

    match service::get_segments(
        &ctx,
        &account_id,
        identifiers.as_deref(),
        Some(cursor),
        "active",
        "all",
        false,
    )
    .await
    {
        Ok((segments, total)) => get_segments_dto(&segments, total, cursor),
        Err(err) => error_response(&err),
    }
}

/// POST /v1/retention/segments/create
pub async fn create_segments(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };
    let account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return error_response(&err),
    };

    let segment = match CreateSegment::from_json(req.body()) {
        Ok(segment) => segment,
        Err(errors) => return validation_response(&errors),
    };

    match service::create_segment(&ctx, &account, &segment).await {
        Ok((created, message)) => create_segment_dto(&created, &message),
        Err(err) => error_response(&err),
    }
}

/// POST /v1/retention/segments/delete
pub async fn delete_segments(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };
    let account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return error_response(&err),
    };
    let account_id = account.account_oid().unwrap_or_default().to_string();

    // Config['MAX_DELETE_SEGMENTS'] is read inside the pydantic validator; a
    // missing key was a KeyError → 500.
    let max_delete = match ctx.config.get_i64("MAX_DELETE_SEGMENTS") {
        Ok(max) => max,
        Err(err) => return error_response(&err),
    };
    let payload = match DeleteSegments::from_json(req.body(), max_delete) {
        Ok(payload) => payload,
        Err(errors) => return validation_response(&errors),
    };

    match service::delete_segments(&ctx, &account_id, &payload.segments).await {
        Ok((deleted, failed)) => delete_segments_dto(&deleted, &failed),
        Err(err) => error_response(&err),
    }
}

/// GET /v1/internal/segments?account_id=…&segment_type=…&status=…
/// (cluster-internal only — keep off the ingress)
pub async fn get_segments_internal(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    // Required query parameters (FastAPI 422s when any is missing).
    let mut errors: Vec<Value> = Vec::new();
    let mut required = |name: &str| -> String {
        match query_param(&req, name) {
            Some(value) => value,
            None => {
                errors.push(models::field_error(
                    vec![json!("query"), json!(name)],
                    "field required",
                    "value_error.missing",
                ));
                String::new()
            }
        }
    };
    let account_id = required("account_id");
    let segment_type = required("segment_type");
    let status = required("status");
    if !errors.is_empty() {
        return validation_response(&errors);
    }

    let cursor = 0;
    match service::get_segments(
        &ctx,
        &account_id,
        None,
        Some(cursor),
        &status,
        &segment_type,
        true,
    )
    .await
    {
        Ok((segments, total)) => get_segments_dto(&segments, total, cursor),
        Err(err) => error_response(&err),
    }
}

/// POST /v1/internal/segments/delete — account-deletion fan-out.
/// (cluster-internal only — keep off the ingress)
pub async fn delete_segments_internal(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let payload = match DeleteAccountSegmentations::from_json(req.body()) {
        Ok(payload) => payload,
        Err(errors) => return validation_response(&errors),
    };

    match service::delete_account_segmentations_internal(&ctx, &payload.account_id).await {
        Ok(is_deleted) => delete_account_segmentations_dto(is_deleted),
        Err(err) => error_response(&err),
    }
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
