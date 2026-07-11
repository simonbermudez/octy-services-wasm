//! Route handlers — ports of `api/routers/profiles.py` / `healthz.py`.
//!
//! Note on rate limits: the FastAPI service used slowapi (`120/minute`).
//! Spin components are stateless per request, so enforce those limits at the
//! ingress/gateway layer.

use octy_shared::errors::{ErrorReason, OctyError};
use octy_shared::models::validation_error;
use serde_json::{json, Value};
use spin_sdk::http::{Params, Request, Response};

use crate::amqp;
use crate::http_util::*;
use crate::models;
use crate::services::profiles as profiles_service;
use octy_spin::auth::decode_account_jwt;
use octy_spin::ctx::Ctx;

fn ctx_or_response() -> Result<Ctx, Response> {
    Ctx::load("profiles").map_err(|e| error_response(&e))
}

pub async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &json!("OK"))
}

// ---------------------------------------------------------------------
// GET /v1/retention/profiles
// ---------------------------------------------------------------------

pub async fn get_customer_profiles(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(c) => c,
        Err(r) => return r,
    };
    let account = match decode_account_jwt(&ctx, &req) {
        Ok(a) => a,
        Err(e) => return error_response(&e),
    };
    let account_id = account.account_oid().unwrap_or_default().to_string();
    let help = ctx.config.opt_str("PROFILES_EXTENDED_HELP").unwrap_or("").to_string();

    let ids_raw = query_param(&req, "ids");
    let rfm_raw = query_param(&req, "rfm");
    let churn_prob_raw = query_param(&req, "churn_prob");
    let segments_raw = query_param(&req, "segments");

    let mut identifiers: Option<Vec<String>> = None;
    let mut rfm_vals: Option<Vec<i64>> = None;
    let mut segments: Option<Vec<String>> = None;
    let mut cursor: i64 = 0;

    if ids_raw.is_none() {
        if let Some(rfm) = &rfm_raw {
            let (ok, vals) = crate::utils::validate_arg_format(rfm);
            if !ok {
                return error_response(&OctyError::new(
                    400,
                    "Invalid query string argument",
                    vec![ErrorReason::new(
                        "rfm argument provided in an invalid format. Required format : int-int",
                        help.as_str(),
                    )],
                ));
            }
            rfm_vals = Some(vals);
        }

        if let Some(cp) = &churn_prob_raw {
            if cp.parse::<i64>().is_ok() {
                return error_response(&OctyError::new(
                    400,
                    "Invalid query string argument",
                    vec![ErrorReason::new(
                        "churn_prob argument provided in an invalid format. Required format : string (low, mid, high, very-high)",
                        help.as_str(),
                    )],
                ));
            }
        }

        if let Some(segs) = &segments_raw {
            segments = Some(segs.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect());
        }

        match validate_pagination_request(&req, None) {
            Ok(c) => cursor = c.unwrap_or(0),
            Err(msg) => {
                return error_response(&OctyError::new(400, "Missing Parameters", vec![ErrorReason::new(msg, help.as_str())]))
            }
        }
    } else {
        let raw = ids_raw.clone().unwrap();
        let identifiers_list = crate::utils::dedupe_identifiers(&raw);
        let max = match ctx.config.get_i64("MAX_GET_PROFILES") {
            Ok(v) => v,
            Err(e) => return error_response(&e),
        };
        if identifiers_list.len() as i64 > max {
            return error_response(&OctyError::new(
                400,
                "Invalid Parameters",
                vec![ErrorReason::new(
                    format!("A maximum number of {max} identifiers can be provided with the \"?ids=\" query param per request"),
                    help.as_str(),
                )],
            ));
        }
        identifiers = Some(identifiers_list);
    }

    match profiles_service::get_profiles(
        &ctx,
        &account_id,
        identifiers.as_deref(),
        cursor,
        segments.as_deref(),
        rfm_vals.as_deref(),
        churn_prob_raw.as_deref(),
    )
    .await
    {
        Ok((profiles, total)) => get_profiles_dto(&profiles, total, cursor),
        Err(e) => error_response(&e),
    }
}

// ---------------------------------------------------------------------
// POST /v1/retention/profiles/create
// ---------------------------------------------------------------------

pub async fn create_customer_profiles(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(c) => c,
        Err(r) => return r,
    };
    let errors_help = ctx.config.opt_str("ERRORS_OVERVIEW_EXTENDED_HELP").unwrap_or("").to_string();
    if let Err(e) = validate_post_headers(&req, &errors_help) {
        return error_response(&e);
    }

    let max_create = match ctx.config.get_i64("MAX_CREATE_PROFILES") {
        Ok(v) => v,
        Err(e) => return error_response(&e),
    };
    let profiles = match models::CreateProfiles::from_json(req.body(), max_create) {
        Ok(p) => p,
        Err(e) => return error_response(&e),
    };

    let account = match decode_account_jwt(&ctx, &req) {
        Ok(a) => a,
        Err(e) => return error_response(&e),
    };

    match profiles_service::create_profiles(&ctx, &account, profiles).await {
        Ok((created, failed)) => create_profiles_dto(&created, &failed),
        Err(e) => service_error_response(&e),
    }
}

// ---------------------------------------------------------------------
// POST /v1/retention/profiles/update
// ---------------------------------------------------------------------

pub async fn update_customer_profiles(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(c) => c,
        Err(r) => return r,
    };
    let errors_help = ctx.config.opt_str("ERRORS_OVERVIEW_EXTENDED_HELP").unwrap_or("").to_string();
    if let Err(e) = validate_post_headers(&req, &errors_help) {
        return error_response(&e);
    }

    let max_update = match ctx.config.get_i64("MAX_UPDATE_DELETE_PROFILES") {
        Ok(v) => v,
        Err(e) => return error_response(&e),
    };
    let body = match models::UpdateProfilesHttp::from_json(req.body(), max_update) {
        Ok(b) => b,
        Err(e) => return error_response(&e),
    };

    let account = match decode_account_jwt(&ctx, &req) {
        Ok(a) => a,
        Err(e) => return error_response(&e),
    };
    let account_id = account.account_oid().unwrap_or_default().to_string();
    let cfg = &account.account_configurations;
    let account_type = cfg.get("a_t").and_then(Value::as_str).unwrap_or("").to_string();
    let account_currency = cfg.get("a_c").and_then(Value::as_str).unwrap_or("").to_string();

    match profiles_service::update_profiles(&ctx, &account_id, &body.profiles, false, Some((&account_type, &account_currency))).await {
        Ok((updated, failed)) => update_profiles_dto(&updated, &failed),
        Err(e) => service_error_response(&e),
    }
}

// ---------------------------------------------------------------------
// POST /v1/retention/profiles/delete
// ---------------------------------------------------------------------

pub async fn delete_customer_profiles(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(c) => c,
        Err(r) => return r,
    };
    let errors_help = ctx.config.opt_str("ERRORS_OVERVIEW_EXTENDED_HELP").unwrap_or("").to_string();
    if let Err(e) = validate_post_headers(&req, &errors_help) {
        return error_response(&e);
    }

    let max_delete = match ctx.config.get_i64("MAX_UPDATE_DELETE_PROFILES") {
        Ok(v) => v,
        Err(e) => return error_response(&e),
    };
    let body = match models::DeleteProfilesHttp::from_json(req.body(), max_delete) {
        Ok(b) => b,
        Err(e) => return error_response(&e),
    };

    let account = match decode_account_jwt(&ctx, &req) {
        Ok(a) => a,
        Err(e) => return error_response(&e),
    };
    let account_id = account.account_oid().unwrap_or_default().to_string();

    match profiles_service::delete_profiles(&ctx, &account_id, &body.profiles, false).await {
        Ok((deleted, failed)) => delete_profiles_dto(&deleted, &failed),
        Err(e) => service_error_response(&e),
    }
}

// ---------------------------------------------------------------------
// GET /v1/retention/profiles/metadata
// ---------------------------------------------------------------------

pub async fn get_profiles_meta(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(c) => c,
        Err(r) => return r,
    };
    let account = match decode_account_jwt(&ctx, &req) {
        Ok(a) => a,
        Err(e) => return error_response(&e),
    };
    let account_id = account.account_oid().unwrap_or_default().to_string();
    let help = ctx.config.opt_str("PROFILES_EXTENDED_HELP").unwrap_or("").to_string();

    let Some(ids_raw) = query_param(&req, "ids") else {
        return error_response(&validation_error(vec![json!({
            "loc": ["query", "ids"], "msg": "field required", "type": "value_error.missing"
        })]));
    };

    // Port of: re.sub(r'(\s|᠋|​|‌|‍|⁠|﻿)+', '', ids)
    let strip_chars = ['\u{180B}', '\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}'];
    let cleaned: String = ids_raw.chars().filter(|c| !c.is_whitespace() && !strip_chars.contains(c)).collect();

    let mut seen = std::collections::HashSet::new();
    let mut identifiers = Vec::new();
    for part in cleaned.split(',') {
        if part.is_empty() {
            continue;
        }
        if seen.insert(part.to_string()) {
            identifiers.push(part.to_string());
        }
    }

    let max = match ctx.config.get_i64("MAX_IDENTIFY_PROFILES") {
        Ok(v) => v,
        Err(e) => return error_response(&e),
    };
    if identifiers.len() as i64 > max {
        return error_response(&OctyError::new(
            400,
            "Invalid Parameters",
            vec![ErrorReason::new(
                format!("A maximum number of {max} identifiers can be provided with the \"?ids=\" query param per request"),
                help.as_str(),
            )],
        ));
    } else if identifiers.is_empty() {
        return error_response(&OctyError::new(
            400,
            "Invalid Parameters",
            vec![ErrorReason::new("A minimum number of 1 identifier must be provided with each request", help.as_str())],
        ));
    }

    match profiles_service::get_profiles_meta(&ctx, &account_id, &identifiers).await {
        Ok(meta) => get_profiles_meta_dto(&meta),
        Err(e) => error_response(&e),
    }
}

// ---------------------------------------------------------------------
// Internal: POST /v1/internal/profiles  (cluster-internal only)
// ---------------------------------------------------------------------

pub async fn get_profiles_internal(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(c) => c,
        Err(r) => return r,
    };

    let Some(ids_raw) = query_param(&req, "ids") else {
        return error_response(&validation_error(vec![json!({
            "loc": ["query", "ids"], "msg": "field required", "type": "value_error.missing"
        })]));
    };
    let ids_bool = matches!(ids_raw.to_lowercase().as_str(), "true" | "1" | "yes" | "on");
    let status = query_param(&req, "status").unwrap_or_else(|| "active".to_string());

    let body = match models::GetProfilesInternal::from_json(req.body()) {
        Ok(b) => b,
        Err(e) => return error_response(&e),
    };

    let mut cursor = 0i64;
    if body.get_all {
        match validate_pagination_request(&req, None) {
            Ok(c) => cursor = c.unwrap_or(0),
            Err(msg) => return error_response(&OctyError::new(400, "Missing Parameters", vec![ErrorReason::new(msg, "")])),
        }
    } else if body.profiles.len() > 2000 {
        return error_response(&OctyError::new(
            400,
            "Exceeded resource request limit",
            vec![ErrorReason::new("can only get 2000 profiles per request", "")],
        ));
    }

    let account_id = body.account_id.clone();
    match profiles_service::get_profiles_internal(&ctx, &account_id, &body, &status, cursor, ids_bool).await {
        Ok((profiles, not_found, total)) => {
            let not_found_val = match not_found {
                Some(nf) => json!(nf),
                None => Value::Null,
            };
            get_profiles_internal_dto(&profiles, &not_found_val, total, cursor)
        }
        Err(e) => error_response(&e),
    }
}

// ---------------------------------------------------------------------
// Internal: POST /v1/internal/profiles/delete  (account deletion fan-out)
// ---------------------------------------------------------------------

pub async fn delete_account_profiles(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(c) => c,
        Err(r) => return r,
    };
    let body = match models::DeleteAccountProfiles::from_json(req.body()) {
        Ok(b) => b,
        Err(e) => return error_response(&e),
    };
    match profiles_service::delete_account_profiles_internal(&ctx, &body.account_id).await {
        Ok(res) => delete_account_profiles_dto(res),
        Err(e) => error_response(&e),
    }
}

// ---------------------------------------------------------------------
// POST /internal/amqp/consume — deliveries forwarded by the data gateway.
// ---------------------------------------------------------------------

pub async fn amqp_consume(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(c) => c,
        Err(r) => return r,
    };
    let outcome = amqp::handle_delivery(&ctx, req.body()).await;
    json_response(outcome.status, &json!({ "detail": outcome.detail }))
}

pub async fn fallback(_req: Request, _params: Params) -> Response {
    not_found()
}
