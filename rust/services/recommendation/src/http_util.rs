//! Recommendation-service DTOs (port of `api/routers/dto/recommendation.py`).
//! Generic envelopes/validation live in `octy_spin::http_util` and are
//! re-exported so handlers keep a single import.

pub use octy_spin::http_util::*;

use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::http::Response;

/// `GetRecommendationsDTO` — the constructor read `training_job_id`,
/// `auc_score` and `model_created_at` out of the meta dict; a missing key was
/// a Python `KeyError` → generic 500, mirrored here.
pub fn get_recommendations_dto(recommendations: &[Value], training_job_meta: &Value) -> Response {
    let map = match training_job_meta.as_object() {
        Some(map) => map,
        None => return error_response(&OctyError::internal("training job meta is not an object")),
    };
    let (training_job_id, auc_score, model_created_at) = match (
        map.get("training_job_id"),
        map.get("auc_score"),
        map.get("model_created_at"),
    ) {
        (Some(t), Some(a), Some(c)) => (t, a, c),
        _ => {
            return error_response(&OctyError::internal(
                "training job meta missing training_job_id / auc_score / model_created_at",
            ))
        }
    };

    json_response(
        200,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Successfully predicted recommendations" },
            "recommendations": recommendations,
            "model_meta_data": {
                "training_job_id": training_job_id,
                "model_accuracy_score": auc_score,
                "recommender_event_type": "charged",
                "model_created_at": model_created_at,
            }
        }),
    )
}

/// `DeleteAccountRecommendationsDTO` — 201, echoes the deletion result.
pub fn delete_account_recommendations_dto(is_deleted: bool) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": { "request_status": "Success", "message": "All Account Recommendations deleted." },
            "is_deleted": is_deleted,
        }),
    )
}
