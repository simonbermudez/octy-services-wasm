//! Route handlers — ports of `api/routers/items.py` / `healthz.py`.
//!
//! Rate limits (slowapi `120/minute` on the retention routes) are enforced at
//! the ingress — Spin components are stateless per request.

use octy_shared::errors::{ErrorReason, OctyError};
use serde_json::json;
use spin_sdk::http::{Params, Request, Response};

use octy_spin::auth::{decode_account_jwt, AuthAccount};
use octy_spin::ctx::Ctx;

use crate::errors::ApiError;
use crate::http_util::*;
use crate::models;
use crate::services::items::ItemsService;

fn ctx_or_response() -> Result<Ctx, Response> {
    Ctx::load("items").map_err(|e| error_response(&e))
}

fn auth_or_response(ctx: &Ctx, req: &Request) -> Result<AuthAccount, Response> {
    decode_account_jwt(ctx, req).map_err(|e| error_response(&e))
}

/// Port of `validate_pagination_request`: any failure (missing header *or*
/// unparseable value — the Python's bare `except Exception`) yields the same
/// "Please provide a valid object identifier…" message.
fn pagination_cursor(req: &Request) -> Result<i64, &'static str> {
    const MSG: &str = "Please provide a valid object identifier within the query string eg: (?id=) or set a pagination header (-H cursor: str)";
    match header_str(req, "cursor") {
        Some(raw) => raw.trim().parse::<i64>().map_err(|_| MSG),
        None => Err(MSG),
    }
}

fn missing_parameters(ctx: &Ctx, message: &str) -> Response {
    let items_help = match ctx.config.get_str("ITEMS_EXTENDED_HELP") {
        Ok(h) => h.to_string(),
        Err(e) => return error_response(&e), // Config KeyError → 500
    };
    error_response(&OctyError::new(
        400,
        "Missing Parameters",
        vec![ErrorReason::new(message, items_help)],
    ))
}

pub async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &json!("OK"))
}

/// GET /v1/retention/items?ids=<item_id(s)>,… (optional — max MAX_GET_ITEMS)
pub async fn get_items(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };
    let account = match auth_or_response(&ctx, &req) {
        Ok(account) => account,
        Err(resp) => return resp,
    };
    let mut service = match ItemsService::new(&ctx, &account) {
        Ok(service) => service,
        Err(err) => return error_response(&err),
    };

    let ids = query_param(&req, "ids");
    let mut identifiers: Option<Vec<String>> = None;
    let mut cursor: i64 = 0;

    match ids {
        None => {
            // Validate pagination headers set
            match pagination_cursor(&req) {
                Ok(c) => cursor = c,
                Err(pag_message) => return missing_parameters(&ctx, pag_message),
            }
        }
        Some(raw) => {
            // split, drop empties, dedupe (first occurrence), then strip —
            // the Python trimmed *after* deduplication.
            let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
            let mut list: Vec<String> = Vec::new();
            for part in raw.split(',') {
                if part.is_empty() || !seen.insert(part) {
                    continue;
                }
                list.push(part.trim().to_string());
            }

            let max_get = match ctx.config.get_i64("MAX_GET_ITEMS") {
                Ok(max) => max,
                Err(e) => return error_response(&e),
            };
            if list.len() as i64 > max_get {
                let items_help = match ctx.config.get_str("ITEMS_EXTENDED_HELP") {
                    Ok(h) => h.to_string(),
                    Err(e) => return error_response(&e),
                };
                return error_response(&OctyError::new(
                    400,
                    "Invalid Parameters",
                    vec![ErrorReason::new(
                        format!("A maximum number of {max_get} identifiers can be provided with the \"?ids=\" query param per request"),
                        items_help,
                    )],
                ));
            }
            identifiers = Some(list);
        }
    }

    match service.get_items(identifiers.as_deref(), cursor).await {
        Ok((items, total)) => get_items_dto(&items, total, cursor),
        Err(err) => err.response(),
    }
}

/// POST /v1/retention/items/create
pub async fn create_items(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };
    let account = match auth_or_response(&ctx, &req) {
        Ok(account) => account,
        Err(resp) => return resp,
    };
    let mut service = match ItemsService::new(&ctx, &account) {
        Ok(service) => service,
        Err(err) => return error_response(&err),
    };

    let create_items = match models::parse_create_items(req.body(), ctx.config.get_i64("MAX_CREATE_ITEMS")) {
        Ok(model) => model,
        Err(err) => return err.response(),
    };

    match service.create_items(create_items).await {
        Ok((created, failed)) => create_items_dto(&created, &failed),
        Err(err) => err.response(),
    }
}

/// POST /v1/retention/items/update
pub async fn update_items(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };
    let account = match auth_or_response(&ctx, &req) {
        Ok(account) => account,
        Err(resp) => return resp,
    };
    let mut service = match ItemsService::new(&ctx, &account) {
        Ok(service) => service,
        Err(err) => return error_response(&err),
    };

    let update_items = match models::parse_update_items(req.body(), ctx.config.get_i64("MAX_UPDATE_DELETE_ITEMS")) {
        Ok(model) => model,
        Err(err) => return err.response(),
    };

    match service.update_items(update_items).await {
        Ok((updated, failed)) => update_items_dto(&updated, &failed),
        Err(err) => err.response(),
    }
}

/// POST /v1/retention/items/delete
pub async fn delete_items(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };
    let account = match auth_or_response(&ctx, &req) {
        Ok(account) => account,
        Err(resp) => return resp,
    };
    let mut service = match ItemsService::new(&ctx, &account) {
        Ok(service) => service,
        Err(err) => return error_response(&err),
    };

    let delete_items = match models::parse_delete_items(req.body(), ctx.config.get_i64("MAX_UPDATE_DELETE_ITEMS")) {
        Ok(model) => model,
        Err(err) => return err.response(),
    };

    match service.delete_items(delete_items).await {
        Ok((deleted, failed)) => delete_items_dto(&deleted, &failed),
        Err(err) => err.response(),
    }
}

/// GET /v1/internal/items?account_id=…&ids=…&status=…
/// (cluster-internal only — keep off the ingress)
pub async fn get_items_internal(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    // FastAPI query-parameter validation (account_id: str, ids: bool, status: str)
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let account_id = query_param(&req, "account_id");
    if account_id.is_none() {
        errors.push(models::query_error("account_id", "field required", "value_error.missing"));
    }
    let ids = match query_param(&req, "ids") {
        None => {
            errors.push(models::query_error("ids", "field required", "value_error.missing"));
            None
        }
        Some(raw) => match models::parse_query_bool(&raw) {
            Some(b) => Some(b),
            None => {
                errors.push(models::query_error(
                    "ids",
                    "value could not be parsed to a boolean",
                    "type_error.bool",
                ));
                None
            }
        },
    };
    let status = query_param(&req, "status");
    if status.is_none() {
        errors.push(models::query_error("status", "field required", "value_error.missing"));
    }
    if !errors.is_empty() {
        return ApiError::validation(errors).response();
    }
    let (account_id, ids, status) = (account_id.unwrap(), ids.unwrap(), status.unwrap());

    // Validate pagination headers set
    let cursor = match pagination_cursor(&req) {
        Ok(cursor) => cursor,
        Err(pag_message) => return missing_parameters(&ctx, pag_message),
    };

    let mut service = ItemsService::internal(&ctx);
    match service.get_items_internal(&account_id, cursor, ids, &status).await {
        Ok((items, total)) => get_items_dto(&items, total, cursor),
        Err(err) => err.response(),
    }
}

/// POST /v1/internal/items/delete — account-deletion fan-out.
/// (cluster-internal only — keep off the ingress)
pub async fn delete_account_items_internal(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let payload = match models::parse_delete_account_items_internal(req.body()) {
        Ok(payload) => payload,
        Err(err) => return err.response(),
    };

    let mut service = ItemsService::internal(&ctx);
    match service.delete_account_items_internal(&payload.account_id).await {
        Ok(res) => delete_account_items_dto(res),
        Err(err) => err.response(),
    }
}

pub async fn fallback(_req: Request, _params: Params) -> Response {
    not_found()
}
