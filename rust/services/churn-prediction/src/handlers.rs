//! Route handlers — ports of `api/routers/churn_prediction.py` / `healthz.py`.

use serde_json::json;
use spin_sdk::http::{Params, Request, Response};

use crate::http_util::*;
use crate::models::DeleteAccountChurnPredictions;
use crate::services;
use octy_spin::auth::decode_account_jwt;
use octy_spin::ctx::Ctx;

fn ctx_or_response() -> Result<Ctx, Response> {
    Ctx::load("churn_prediction").map_err(|e| error_response(&e))
}

/// GET /healthz — k8s liveness/readiness probe target, no auth required.
pub async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &json!("OK"))
}

/// GET /v1/retention/churn_prediction/report
/// (Python rate limit `120/minute` is enforced at the ingress layer.)
pub async fn get_churn_report(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let current_account = match decode_account_jwt(&ctx, &req) {
        Ok(account) => account,
        Err(err) => return error_response(&err),
    };

    match services::generate_churn_report(&ctx, &current_account).await {
        Ok(churn_report) => generate_churn_report_dto(&churn_report),
        Err(err) => error_response(&err),
    }
}

/// POST /v1/internal/churn_prediction/delete  (cluster-internal only — keep
/// off the ingress; no auth, matching the Python route.)
pub async fn delete_churn_prediction_internal(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let payload = match DeleteAccountChurnPredictions::from_json(req.body()) {
        Ok(payload) => payload,
        Err(err) => return error_response(&err),
    };

    match services::delete_account_churn_predictions_internal(&ctx, &payload.account_id).await {
        Ok(res) => delete_account_churn_predictions_dto(res),
        Err(err) => error_response(&err),
    }
}

pub async fn fallback(_req: Request, _params: Params) -> Response {
    not_found()
}
