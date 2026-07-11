//! Segmentation-service DTOs (port of `api/routers/dto/segmentation.py`).
//! Generic envelopes/validation live in `octy_spin::http_util` and are
//! re-exported so handlers keep a single import.

pub use octy_spin::http_util::*;

use serde_json::{json, Value};
use spin_sdk::http::Response;

/// FastAPI `RequestValidationError` envelope — the `errors` array carries the
/// raw pydantic-style dicts (`{"loc": …, "msg": …, "type": …}`), exactly as
/// the Python handler appended them.
pub fn validation_response(errors: &[Value]) -> Response {
    json_response(
        422,
        &json!({
            "request_meta": { "request_status": "Failed", "message": "Unprocessable Entity" },
            "error": {
                "code": 422,
                "reason": "Missing or invalid JSON parameters",
                "errors": errors,
            }
        }),
    )
}

/// GetSegmentsDTO — 200 with a `cursor` response header advanced by the
/// number of returned segments.
pub fn get_segments_dto(segments: &[Value], total: i64, cursor: i64) -> Response {
    let body = json!({
        "request_meta": {
            "request_status": "Success",
            "message": "Segments found.",
            "count": segments.len(),
            "total": total,
        },
        "segments": segments,
    });
    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("cursor", (cursor + segments.len() as i64).to_string())
        .body(serde_json::to_vec(&body).expect("serializable json"))
        .build()
}

/// CreateSegmentDTO — 201.
pub fn create_segment_dto(segment: &Value, message: &str) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": { "request_status": "Success", "message": message },
            "segment_id": segment["segment_id"],
            "segment_name": segment["segment_name"],
            "segment_type": segment["segment_type"],
            "segment_sub_type": segment["segment_sub_type"],
            "segment_status": segment["segment_status"],
        }),
    )
}

/// DeleteSegmentsDTO — 200.
pub fn delete_segments_dto(deleted_segments: &[Value], failed_to_delete: &[Value]) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Segments flagged to be deleted.",
            },
            "deleted_segments": deleted_segments,
            "failed_to_delete": failed_to_delete,
        }),
    )
}

/// DeleteAccountSegmentationsDTO — 201.
pub fn delete_account_segmentations_dto(is_deleted: bool) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Octy account segmentations deleted.",
            },
            "is_deleted": is_deleted,
        }),
    )
}
