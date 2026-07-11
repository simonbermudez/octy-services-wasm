//! Route handlers — ports of `api/routers/events.py` and `event_types.py`.
//!
//! Note on rate limits: the FastAPI service used slowapi (`120/minute` on
//! every public route). Spin components are stateless per request, so
//! enforce those limits at the ingress/gateway layer.

use serde_json::json;
use spin_sdk::http::{Params, Request, Response};

use octy_spin::auth::decode_account_jwt;
use octy_spin::ctx::Ctx;
use octy_spin::http_util::{query_param, validate_pagination_request};

use crate::amqp;
use crate::http_util::*;
use crate::models::{
    BatchCreateEvents, CreateEvent, CreateEventTypes, DeleteEventTypes, DeleteEventsInternal,
    GetEventsInternal, GetEventTypesInternal,
};
use crate::services::{event_types as event_types_service, events as events_service};

fn ctx_or_response() -> Result<Ctx, Response> {
    Ctx::load("events").map_err(|e| ApiError::from(e).to_response())
}

pub async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &json!("OK"))
}

// ---------------------------------------------------------------------------
// Events routers (`api/routers/events.py`)
// ---------------------------------------------------------------------------

/// POST /v1/retention/events/create
pub async fn create_event_instance(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return ApiError::from(err).to_response(),
    };

    let event = match CreateEvent::from_json(req.body()) {
        Ok(event) => event,
        Err(err) => return err.to_response(),
    };

    match events_service::create_event(&ctx, &account, &event).await {
        Ok(created) => create_event_dto(&created),
        Err(err) => err.to_response(),
    }
}

/// POST /v1/retention/events/create/batch
pub async fn batch_create_event_instances(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return ApiError::from(err).to_response(),
    };

    let max_create_events = match ctx.config.get_i64("MAX_CREATE_EVENTS") {
        Ok(v) => v,
        Err(e) => return ApiError::from(e).to_response(),
    };
    let events = match BatchCreateEvents::from_json(req.body(), max_create_events) {
        Ok(events) => events,
        Err(err) => return err.to_response(),
    };

    match events_service::batch_create_events(&ctx, &account, &events).await {
        Ok((valid, invalid)) => batch_create_events_dto(&valid, &invalid),
        Err(err) => err.to_response(),
    }
}

/// POST /v1/retention/events — get latest checkout_contact_info_submitted
/// event for a given checkout id (`checkout_id` is a query param).
pub async fn get_latest_checkout_info_submmited_event(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return ApiError::from(err).to_response(),
    };

    let Some(checkout_id) = query_param(&req, "checkout_id") else {
        return ApiError::validation(vec![json!({
            "loc": ["query", "checkout_id"],
            "msg": "field required",
            "type": "value_error.missing",
        })])
        .to_response();
    };

    match events_service::get_latest_checkout_info_submmited_event(&ctx, &account, &checkout_id).await {
        Ok(event) => get_event_dto(&event),
        Err(err) => err.to_response(),
    }
}

/// POST /v1/internal/events — cluster-internal only. Do not expose this
/// route in the ingress.
pub async fn get_events_internal(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let cursor = match validate_pagination_request(&req, None) {
        Ok(cursor) => match cursor {
            Some(c) => c,
            None => {
                return ApiError::reason(400, "Missing Parameters", "internal error", "").to_response()
            }
        },
        Err(message) => return ApiError::reason(400, "Missing Parameters", message, "").to_response(),
    };

    let body = match GetEventsInternal::from_json(req.body()) {
        Ok(body) => body,
        Err(err) => return err.to_response(),
    };

    match events_service::get_events(
        &ctx,
        &body.account_id,
        body.timeframe,
        cursor,
        body.event_sequence_event.as_ref(),
        body.profile_ids.as_ref(),
        body.event_type.as_deref(),
    )
    .await
    {
        Ok((events, total)) => internal_get_events_dto(&events, total),
        Err(err) => err.to_response(),
    }
}

/// POST /v1/internal/events/delete — cluster-internal only, account deletion
/// fan-out. Do not expose this route in the ingress.
pub async fn delete_events_internal(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let body = match DeleteEventsInternal::from_json(req.body()) {
        Ok(body) => body,
        Err(err) => return err.to_response(),
    };

    match events_service::delete_account_events_internal(&ctx, &body.account_id).await {
        Ok(_) => internal_delete_events_dto(),
        Err(err) => err.to_response(),
    }
}

// ---------------------------------------------------------------------------
// Custom event types routers (`api/routers/event_types.py`)
// ---------------------------------------------------------------------------

/// Strip zero-width/whitespace-like unicode characters (port of the
/// `re.sub(r'(\s|᠋|​|‌|‍|⁠|﻿)+', '', ids)`).
fn strip_zero_width(input: &str) -> String {
    input
        .chars()
        .filter(|c| {
            !c.is_whitespace()
                && !matches!(*c, '\u{180B}' | '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}')
        })
        .collect()
}

/// GET /v1/retention/events/types?ids=<comma-separated ids>
pub async fn get_custom_event_types(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return ApiError::from(err).to_response(),
    };

    let ids = query_param(&req, "ids");
    let mut cursor: i64 = 0;
    let mut identifiers: Option<Vec<String>> = None;

    match &ids {
        None => {
            let help = ctx.config.opt_str("CUSTOM_EVENTS_EXTENDED_HELP").unwrap_or("").to_string();
            match validate_pagination_request(&req, None) {
                Ok(Some(c)) => cursor = c,
                Ok(None) => {
                    return ApiError::reason(400, "Missing Parameters", "internal error", "").to_response()
                }
                Err(message) => {
                    return ApiError::reason(400, "Missing Parameters", message, help).to_response()
                }
            }
        }
        Some(raw) => {
            let cleaned = strip_zero_width(raw);
            let mut seen = std::collections::HashSet::new();
            let mut ordered: Vec<String> = Vec::new();
            for part in cleaned.split(',') {
                if part.is_empty() {
                    continue;
                }
                if seen.insert(part.to_string()) {
                    ordered.push(part.to_string());
                }
            }

            let max_get_event_types = match ctx.config.get_i64("MAX_GET_EVENT_TYPES") {
                Ok(v) => v,
                Err(e) => return ApiError::from(e).to_response(),
            };
            if ordered.len() as i64 > max_get_event_types {
                let help = ctx.config.opt_str("CUSTOM_EVENTS_EXTENDED_HELP").unwrap_or("").to_string();
                return ApiError::reason(
                    400,
                    "Invalid Parameters",
                    format!(
                        "A maximum number of {max_get_event_types} identifiers can be provided with the \"?ids=\" query param per request"
                    ),
                    help,
                )
                .to_response();
            }
            identifiers = Some(ordered);
        }
    }

    match event_types_service::get_event_types(&ctx, &account, identifiers.as_deref(), cursor).await {
        Ok((event_types, total)) => get_event_types_dto(&event_types, total, cursor),
        Err(err) => err.to_response(),
    }
}

/// POST /v1/retention/events/types/create
pub async fn create_custom_event_types(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return ApiError::from(err).to_response(),
    };

    let max_create_event_types = match ctx.config.get_i64("MAX_CREATE_EVENT_TYPES") {
        Ok(v) => v,
        Err(e) => return ApiError::from(e).to_response(),
    };
    let event_types = match CreateEventTypes::from_json(req.body(), max_create_event_types) {
        Ok(v) => v,
        Err(err) => return err.to_response(),
    };

    match event_types_service::create_event_types(&ctx, &account, &event_types).await {
        Ok((created, failed)) => create_event_types_dto(&created, &failed),
        Err(err) => err.to_response(),
    }
}

/// POST /v1/retention/events/types/delete
pub async fn delete_custom_event_types(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return ApiError::from(err).to_response(),
    };

    let max_delete_event_types = match ctx.config.get_i64("MAX_DELETE_EVENT_TYPES") {
        Ok(v) => v,
        Err(e) => return ApiError::from(e).to_response(),
    };
    let event_type_ids = match DeleteEventTypes::from_json(req.body(), max_delete_event_types) {
        Ok(v) => v,
        Err(err) => return err.to_response(),
    };

    match event_types_service::delete_event_types(&ctx, &account, &event_type_ids).await {
        Ok((deleted, failed)) => delete_event_types_dto(&deleted, &failed),
        Err(err) => err.to_response(),
    }
}

/// POST /v1/retention/events/types/delete/all
pub async fn delete_all_custom_event_types(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return ApiError::from(err).to_response(),
    };

    match event_types_service::delete_all_event_types(&ctx, &account).await {
        Ok((deleted, failed)) => delete_event_types_dto(&deleted, &failed),
        Err(err) => err.to_response(),
    }
}

/// POST /v1/internal/events/types — cluster-internal only. Do not expose
/// this route in the ingress.
pub async fn get_event_types_internal(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let body = match GetEventTypesInternal::from_json(req.body()) {
        Ok(body) => body,
        Err(err) => return err.to_response(),
    };

    if body.event_type_names.len() > 200 {
        return ApiError::reason(
            400,
            "Exceeded resource request limit",
            "can only get 200 event_type_names per request",
            "",
        )
        .to_response();
    }

    match event_types_service::get_event_types_internal(&ctx, &body.account_id, &body.event_type_names).await
    {
        Ok((found, not_found)) => get_event_types_internal_dto(&found, &not_found),
        Err(err) => err.to_response(),
    }
}

// ---------------------------------------------------------------------------
// AMQP + fallback
// ---------------------------------------------------------------------------

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
