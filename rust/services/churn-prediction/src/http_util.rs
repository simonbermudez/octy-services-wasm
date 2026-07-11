//! Churn-prediction DTOs (port of `api/routers/dto/churn_prediction.py`).
//! Generic envelopes/validation live in `octy_spin::http_util` and are
//! re-exported so handlers keep a single import.

pub use octy_spin::http_util::*;

use serde_json::{json, Value};
use spin_sdk::http::Response;

/// Port of `GenerateChurnReportDTO`.
pub fn generate_churn_report_dto(churn_prediction_report: &Value) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Successfully generated churn report",
            },
            "churn_prediction_report": churn_prediction_report,
        }),
    )
}

/// Port of `DeleteAccountChurnPredictionsDTO` (the Python returned 201).
pub fn delete_account_churn_predictions_dto(is_deleted: bool) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Octy account churn predictions deleted.",
            },
            "is_deleted": is_deleted,
        }),
    )
}
