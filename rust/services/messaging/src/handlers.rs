//! Route handlers — ports of `api/routers/messaging.py`.

use serde_json::json;
use spin_sdk::http::{Params, Request, Response};

use octy_shared::errors::{ErrorReason, OctyError};
use octy_spin::auth::decode_account_jwt;
use octy_spin::ctx::Ctx;

use crate::http_util::*;
use crate::models::{CreateTemplates, DeleteAccountMessaging, DeleteTemplates, GenerateContent, UpdateTemplates};
use crate::services::messaging as messaging_service;
use crate::services::template_engine::TemplateEngine;

fn ctx_or_response() -> Result<Ctx, Response> {
    Ctx::load("messaging").map_err(|e| error_response(&e))
}

pub async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &json!("OK"))
}

/// GET /v1/retention/messaging/templates
pub async fn get_templates(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return error_response(&err),
    };

    let errors_help = ctx.config.opt_str("MESSAGING_EXTENDED_HELP").unwrap_or("").to_string();
    let ids_param = query_param(&req, "ids");

    let (identifiers, cursor) = if let Some(ids) = &ids_param {
        // Same normalization as the Python: split on ',', drop empties
        // (dict.fromkeys preserves first-seen order while de-duplicating),
        // trim leading/trailing whitespace.
        let mut seen: Vec<String> = Vec::new();
        for raw in ids.split(',') {
            let trimmed = raw.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            if !seen.contains(&trimmed) {
                seen.push(trimmed);
            }
        }
        (Some(seen), 0)
    } else {
        match validate_pagination_request(&req, None) {
            Ok(cursor) => (None, cursor.unwrap_or(0)),
            Err(message) => {
                return error_response(&OctyError::new(
                    400,
                    "Missing Parameters",
                    vec![ErrorReason::new(message, errors_help)],
                ))
            }
        }
    };

    if let Some(ids) = &identifiers {
        let max = match ctx.config.get_i64("MAX_GET_TEMPLATES") {
            Ok(v) => v,
            Err(err) => return error_response(&err),
        };
        if ids.len() as i64 > max {
            return error_response(&OctyError::new(
                400,
                "Invalid Parameters",
                vec![ErrorReason::new(
                    format!(
                        "A maximum number of {max} identifiers can be provided with the \"?ids=\" query param per request"
                    ),
                    errors_help,
                )],
            ));
        }
    }

    match messaging_service::get_templates(&ctx, &account, identifiers, cursor).await {
        Ok((templates, total)) => get_templates_dto(&templates, total, cursor),
        Err(err) => error_response(&err),
    }
}

/// POST /v1/retention/messaging/templates/create
pub async fn create_templates(req: Request, _params: Params) -> Response {
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

    let templates = match CreateTemplates::from_json(req.body(), &ctx.config) {
        Ok(t) => t,
        Err(err) => return err.response(),
    };

    match messaging_service::create_templates(&ctx, &account, &templates).await {
        Ok((created, failed)) => create_templates_dto(&created, &failed),
        Err(err) => err.response(),
    }
}

/// POST /v1/retention/messaging/templates/update
pub async fn update_templates(req: Request, _params: Params) -> Response {
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

    let templates = match UpdateTemplates::from_json(req.body(), &ctx.config) {
        Ok(t) => t,
        Err(err) => return err.response(),
    };

    match messaging_service::update_templates(&ctx, &account, &templates).await {
        Ok((updated, failed)) => update_templates_dto(&updated, &failed),
        Err(err) => err.response(),
    }
}

/// POST /v1/retention/messaging/templates/delete
pub async fn delete_templates(req: Request, _params: Params) -> Response {
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

    let templates = match DeleteTemplates::from_json(req.body(), &ctx.config) {
        Ok(t) => t,
        Err(err) => return err.response(),
    };

    match messaging_service::delete_templates(&ctx, &account, &templates).await {
        Ok((deleted, failed)) => delete_templates_dto(&deleted, &failed),
        Err(err) => err.response(),
    }
}

/// POST /v1/retention/messaging/content/generate
pub async fn generate_content(req: Request, _params: Params) -> Response {
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

    let messages = match GenerateContent::from_json(req.body(), &ctx.config) {
        Ok(m) => m,
        Err(err) => return err.response(),
    };

    let mut engine = TemplateEngine::new(&ctx, &account);
    if let Err(err) = engine.generate(&messages).await {
        return err.response();
    }
    generate_content_dto(&engine.created_messages, &engine.failed_messages, &engine.failed_templates)
}

/// POST /v1/internal/messaging/delete
pub async fn delete_messaging_internal(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let payload = match DeleteAccountMessaging::from_json(req.body()) {
        Ok(p) => p,
        Err(err) => return err.response(),
    };

    match messaging_service::delete_account_messaging_internal(&ctx, &payload.account_id).await {
        Ok(res) => delete_account_messaging_dto(res),
        Err(err) => error_response(&err),
    }
}
