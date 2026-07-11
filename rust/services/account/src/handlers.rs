//! Route handlers — ports of `api/routers/account.py`, `auth.py`, `healthz.py`.
//!
//! Note on rate limits: the FastAPI service used slowapi (`120/minute` on
//! create, `10000/minute` on authenticate). Spin components are stateless per
//! request, so enforce those limits at the ingress/gateway layer.

use octy_shared::errors::{ErrorReason, OctyError};
use octy_shared::models::{CreateAccount, DeleteAccount, GetAccountsInternal};
use serde_json::json;
use spin_sdk::http::{Params, Request, Response};

use crate::amqp;
use octy_spin::ctx::Ctx;
use crate::http_util::*;
use crate::services::{account as account_service, auth as auth_service};

fn ctx_or_response() -> Result<Ctx, Response> {
    Ctx::load("account").map_err(|e| error_response(&e))
}

pub async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &json!("OK"))
}

/// GET /v1/account/authenticate
pub async fn authenticate_account(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let (pk, sk) = match auth_service::validate_auth_request_headers(&ctx, &req) {
        Ok(keys) => keys,
        Err(err) => return error_response(&err),
    };

    match auth_service::authenticate(&ctx, &req, &pk, &sk).await {
        Ok(token) => authenticate_dto(&token),
        Err(err) => error_response(&err),
    }
}

/// POST /v1/admin/account/create
pub async fn create_new_account(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let errors_help = ctx.config.opt_str("ERRORS_OVERVIEW_EXTENDED_HELP").unwrap_or("").to_string();
    if let Err(err) = validate_post_headers(&req, &errors_help) {
        return error_response(&err);
    }

    let account = match CreateAccount::from_json(req.body()) {
        Ok(account) => account,
        Err(err) => return error_response(&err),
    };

    match account_service::create_account(&ctx, &account).await {
        Ok(new_account) => create_account_dto(&new_account),
        Err(err) => error_response(&err),
    }
}

/// POST /v1/admin/account/delete
pub async fn delete_account(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let errors_help = ctx.config.opt_str("ERRORS_OVERVIEW_EXTENDED_HELP").unwrap_or("").to_string();
    if let Err(err) = validate_post_headers(&req, &errors_help) {
        return error_response(&err);
    }

    let payload: DeleteAccount = match serde_json::from_slice(req.body()) {
        Ok(payload) => payload,
        Err(e) => {
            return error_response(&octy_shared::models::validation_error(vec![json!({
                "loc": ["body"], "msg": e.to_string(), "type": "value_error.jsondecode"
            })]))
        }
    };

    match account_service::delete_account(&ctx, &payload.account_id).await {
        Ok(true) => delete_account_dto(&payload.account_id),
        Ok(false) => error_response(&OctyError::new(
            400,
            "Bad Request",
            vec![ErrorReason::new(
                format!("Error occured and could not delete account {}", payload.account_id),
                "",
            )],
        )),
        Err(err) => error_response(&err),
    }
}

/// POST /v1/internal/accounts  (cluster-internal only — keep off the ingress)
pub async fn get_accounts_internal(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let cursor = match validate_pagination_request(&req, None) {
        Ok(cursor) => cursor.unwrap_or(0),
        Err(message) => {
            return error_response(&OctyError::new(
                400,
                "Missing Parameters",
                vec![ErrorReason::new(message, "")],
            ))
        }
    };

    let body: GetAccountsInternal = match serde_json::from_slice(req.body()) {
        Ok(body) => body,
        Err(e) => {
            return error_response(&octy_shared::models::validation_error(vec![json!({
                "loc": ["body", "account_ids"], "msg": e.to_string(), "type": "value_error"
            })]))
        }
    };

    match account_service::get_accounts_internal(&ctx, &body.account_ids, cursor).await {
        Ok((accounts, total)) => get_accounts_internal_dto(&accounts, total),
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
