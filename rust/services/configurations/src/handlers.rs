//! Route handlers — ports of `api/routers/account_configurations.py`,
//! `api/routers/algorithm_configurations.py`, `api/routers/healthz.py`.
//!
//! Rate limits: the FastAPI service used slowapi (`120/minute` on every
//! route). Spin components are stateless per request, so enforce those
//! limits at the ingress/gateway layer.
//!
//! Ordering matches FastAPI's dependency resolution: `decode_account_jwt`
//! runs before the request body is validated.

use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::http::{Params, Request, Response};

use octy_spin::auth::{decode_account_jwt, AuthAccount};
use octy_spin::ctx::Ctx;

use crate::http_util::*;
use crate::models;
use crate::repos;

fn ctx_or_response() -> Result<Ctx, Response> {
    Ctx::load("configurations").map_err(|e| error_response(&e))
}

/// Wrap pydantic-style errors in the 422 envelope. The Python handler read
/// `Config['CONFIG_EXTENDED_HELP']` (a `KeyError` there became a 500).
fn validation_422(ctx: &Ctx, errors: Vec<Value>) -> Response {
    match ctx.config.get_str("CONFIG_EXTENDED_HELP") {
        Ok(help) => validation_response(help, errors),
        Err(e) => error_response(&e),
    }
}

/// `current_account.account_id` — the Python pydantic `Account` model coerced
/// the `b.a_id` claim to `str` (anything non-string blew up as a 500).
fn account_id_or_response(account: &AuthAccount) -> Result<String, Response> {
    account
        .account_oid()
        .map(str::to_string)
        .ok_or_else(|| {
            error_response(&OctyError::internal(
                "could not determine account id from auth token claims",
            ))
        })
}

/// GET /healthz
pub async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &json!("OK"))
}

/// POST /v1/configurations/account/set
pub async fn set_account_configs(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let current_account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return error_response(&err),
    };

    let mut configs = match models::SetAccountConfigs::from_json(req.body()) {
        Ok(configs) => configs,
        Err(errors) => return validation_422(&ctx, errors),
    };

    let account_id = match account_id_or_response(&current_account) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    configs.account_id = Some(account_id);

    if let Err(err) = repos::set_account_configs(&ctx, &configs).await {
        return error_response(&err);
    }

    set_account_configs_dto(&configs)
}

/// GET /v1/configurations/account
pub async fn get_account_configs(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let current_account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return error_response(&err),
    };

    match account_configs_dto(&current_account.account_configurations) {
        Ok(resp) => resp,
        Err(err) => error_response(&err),
    }
}

/// POST /v1/configurations/retention/algorithms/set
pub async fn set_algorithm_configs(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let current_account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return error_response(&err),
    };

    // `Config['OCTY_ALGO_TYPES']` — a KeyError inside the pydantic validator
    // escaped as a 500 in the Python; same here.
    let allowed_algorithms = match ctx.config.get_array("OCTY_ALGO_TYPES") {
        Ok(allowed) => allowed.clone(),
        Err(err) => return error_response(&err),
    };

    let base = match models::BaseSetAlgoConfigs::from_json(req.body(), &allowed_algorithms) {
        Ok(base) => base,
        Err(errors) => return validation_422(&ctx, errors),
    };

    let account_id = match account_id_or_response(&current_account) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let (config_json, mut return_configs) = if base.algorithm_name == "rec" {
        // Algorithm-specific validations: the Python fetched the account's
        // item ids BEFORE validating the rec configurations — kept as-is.
        let items = match repos::get_items(&ctx, &account_id).await {
            Ok(items) => items,
            Err(err) => return error_response(&err),
        };

        let rec = match models::RecConfigs::from_value(&base.configurations) {
            Ok(rec) => rec,
            Err(errors) => return validation_422(&ctx, errors),
        };

        // Silently drop stop-list ids that are not among the account's items.
        // (`if i:` in the Python also skipped matched-but-falsy ids.)
        let mut valid_stop_list: Vec<Value> = Vec::new();
        for item_id in &rec.item_id_stop_list {
            let matched = items
                .iter()
                .find(|iid| **iid == Value::String(item_id.clone()));
            if let Some(i) = matched {
                if is_truthy(i) {
                    valid_stop_list.push(json!({ "item_id": i }));
                }
            }
        }

        let config_json = rec.config_json(Value::Array(valid_stop_list));
        let mut return_configs = config_json.clone();
        return_configs
            .as_object_mut()
            .expect("config_json is an object")
            .remove("rec_item_identifier");
        (config_json, return_configs)
    } else if base.algorithm_name == "churn" {
        let churn = match models::ChurnPredConfigs::from_value(&base.configurations) {
            Ok(churn) => churn,
            Err(errors) => return validation_422(&ctx, errors),
        };
        let config_json = churn.config_json();
        let mut return_configs = config_json.clone();
        return_configs
            .as_object_mut()
            .expect("config_json is an object")
            .remove("churn_item_identifier");
        (config_json, return_configs)
    } else {
        // Config allowed an algorithm the handler does not implement — the
        // Python crashed on the unbound `return_configs` (500).
        return error_response(&OctyError::internal(format!(
            "unhandled algorithm name: {}",
            base.algorithm_name
        )));
    };

    return_configs
        .as_object_mut()
        .expect("return configs is an object")
        .remove("event_type");

    if let Err(err) =
        repos::set_algorithm_configs(&ctx, &account_id, &base.algorithm_name, &config_json).await
    {
        return error_response(&err);
    }

    set_algorithm_configs_dto(&base.algorithm_name, &return_configs)
}

/// GET /v1/configurations/retention/algorithms
pub async fn get_algorithm_configs(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let current_account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return error_response(&err),
    };

    match algorithm_configs_dto(&current_account.algorithm_configurations) {
        Ok(resp) => resp,
        Err(err) => error_response(&err),
    }
}

pub async fn fallback(_req: Request, _params: Params) -> Response {
    not_found()
}

/// Python truthiness (`if i:`) for JSON values.
fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}
