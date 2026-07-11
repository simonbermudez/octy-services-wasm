//! Account-service DTOs (ports of `api/routers/dto/account.py` / `auth.py`).
//! Generic envelopes/validation live in `octy_spin::http_util` and are
//! re-exported so handlers keep a single import.

pub use octy_spin::http_util::*;

use serde_json::{json, Value};
use spin_sdk::http::Response;

pub fn create_account_dto(new_account: &Value) -> Response {
    json_response(
        201,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Account created!" },
            "account_name": new_account["account_name"],
            "account_type": new_account["account_type"],
            "account_currency": new_account["account_currency"],
            "pk": new_account["pk"],
            "sk": new_account["sk"],
            "notification_sent": new_account["notification_sent"],
            "sent_to": new_account["contact_email_address"],
        }),
    )
}

pub fn delete_account_dto(account_id: &str) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": format!("Successfully deleted account {account_id} and all associated data!"),
            }
        }),
    )
}

pub fn get_accounts_internal_dto(accounts: &[Value], total: i64) -> Response {
    json_response(
        200,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Found Accounts",
                "count": accounts.len(),
                "total": total,
            },
            "accounts": accounts,
        }),
    )
}

pub fn authenticate_dto(auth_token: &str) -> Response {
    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("X-AUTH-JWT", auth_token)
        .body(
            serde_json::to_vec(&json!({
                "request_meta": {
                    "request_status": "Success",
                    "message": "Successfully generated account authorization token",
                },
                "auth": { "jwt": auth_token }
            }))
            .expect("serializable json"),
        )
        .build()
}
