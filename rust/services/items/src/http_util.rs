//! Items-service DTOs (port of `api/routers/dto/items.py`). Generic
//! envelopes live in `octy_spin::http_util` and are re-exported so handlers
//! keep a single import.

pub use octy_spin::http_util::*;

use serde_json::{json, Value};
use spin_sdk::http::Response;

/// GetItemsDTO — 200 + a `cursor` response header of `cursor + len(items)`.
pub fn get_items_dto(items: &[Value], total: i64, cursor: i64) -> Response {
    let body = json!({
        "request_meta": {
            "request_status": "Success",
            "message": "Items found.",
            "count": items.len(),
            "total": total,
        },
        "items": items,
    });
    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("cursor", (cursor + items.len() as i64).to_string())
        .body(serde_json::to_vec(&body).expect("serializable json"))
        .build()
}

/// CreateItemsDTO — 201.
pub fn create_items_dto(created_items: &[Value], failed_to_create: &[Value]) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Items created.",
                "count": created_items.len(),
            },
            "items": created_items,
            "failed_to_create": failed_to_create,
        }),
    )
}

/// UpdateItemsDTO — 200.
pub fn update_items_dto(updated_items: &[Value], failed_to_update: &[Value]) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Items updated.",
            },
            "items": updated_items,
            "failed_to_update": failed_to_update,
        }),
    )
}

/// DeleteItemsDTO — 200.
pub fn delete_items_dto(deleted_items: &[Value], failed_to_delete: &[Value]) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Items deleted.",
            },
            "deleted_items": deleted_items,
            "failed_to_delete": failed_to_delete,
        }),
    )
}

/// DeleteAccountItemsDTO — 201.
pub fn delete_account_items_dto(is_deleted: bool) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Account Items deleted.",
            },
            "is_deleted": is_deleted,
        }),
    )
}
