//! Configurations DTOs (port of `api/routers/dto/configurations.py`)
//! plus the service's 422 envelope (port of the `RequestValidationError`
//! handler in `api/routers/error_handlers.py`). Generic envelopes live in
//! `octy_spin::http_util` and are re-exported so handlers keep one import.

pub use octy_spin::http_util::*;

use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::http::Response;

use super::models::SetAccountConfigs;

/// The FastAPI `RequestValidationError` handler: each pydantic error dict is
/// embedded verbatim as `error_message`, with `Config['CONFIG_EXTENDED_HELP']`
/// as `extended_help`.
pub fn validation_response(extended_help: &str, errors: Vec<Value>) -> Response {
    let entries: Vec<Value> = errors
        .into_iter()
        .map(|e| json!({ "error_message": e, "extended_help": extended_help }))
        .collect();
    json_response(
        422,
        &json!({
            "request_meta": { "request_status": "Failed", "message": "Unprocessable Entity" },
            "error": {
                "code": 422,
                "reason": "Missing or invalid JSON parameters",
                "errors": entries,
            }
        }),
    )
}

/// `SetAccountConfigsDTO` — 202 Accepted.
pub fn set_account_configs_dto(configs: &SetAccountConfigs) -> Response {
    json_response(
        202,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Accepted. Updating account configurations.",
            },
            "account_data": {
                "contact_name": configs.contact_name,
                "contact_surname": configs.contact_surname,
                "contact_email_address": configs.contact_email_address,
                "webhook_url": configs.webhook_url,
                "authenticated_id_key": configs.authenticated_id_key,
            }
        }),
    )
}

/// `AccountConfigsDTO` — reads the `a_cf` claim. The Python indexed the dict
/// directly (`KeyError` → generic 500), so missing keys stay a 500 here.
pub fn account_configs_dto(account_configurations: &Value) -> Result<Response, OctyError> {
    let claim = |key: &str| -> Result<Value, OctyError> {
        account_configurations
            .get(key)
            .cloned()
            .ok_or_else(|| OctyError::internal(format!("KeyError: '{key}' missing from account configurations claim")))
    };
    Ok(json_response(
        200,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Successfully got account configurations.",
            },
            "account_data": {
                "contact_name": claim("c_n")?,
                "contact_surname": claim("c_s")?,
                "contact_email_address": claim("c_e")?,
                "webhook_url": claim("we")?,
                "authenticated_id_key": claim("ak")?,
            }
        }),
    ))
}

/// `SetAlgorithmConfigsDTO` — 202 Accepted.
pub fn set_algorithm_configs_dto(algorithm_name: &str, configs: &Value) -> Response {
    json_response(
        202,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Accepted. Setting algorithm configurations.",
            },
            "configurations": [{
                "algorithm_name": algorithm_name,
                "configurations": configs,
            }]
        }),
    )
}

/// `AlgorithmConfigsDTO` — reads the `al_cf` claim; strips `event_type` and
/// the per-algorithm item-identifier keys before returning. Structural
/// surprises (non-list claim, missing keys, non-dict `config_json`) raised
/// in the Python (`TypeError`/`KeyError`/`AttributeError` → 500) and do the
/// same here.
pub fn algorithm_configs_dto(algorithm_configurations: &Value) -> Result<Response, OctyError> {
    let list = algorithm_configurations
        .as_array()
        .ok_or_else(|| OctyError::internal("algorithm configurations claim is not a list"))?;

    let mut configurations: Vec<Value> = Vec::with_capacity(list.len());
    for c in list {
        let name = c
            .get("algorithm_name")
            .cloned()
            .ok_or_else(|| OctyError::internal("KeyError: 'algorithm_name' missing from algorithm configuration"))?;
        let mut cfg = c
            .get("config_json")
            .cloned()
            .ok_or_else(|| OctyError::internal("KeyError: 'config_json' missing from algorithm configuration"))?;
        let obj = cfg
            .as_object_mut()
            .ok_or_else(|| OctyError::internal("config_json is not an object"))?;
        obj.remove("event_type");
        if name == json!("rec") {
            obj.remove("rec_item_identifier");
        } else if name == json!("churn") {
            obj.remove("churn_item_identifier");
        }
        configurations.push(json!({ "algorithm_name": name, "configurations": cfg }));
    }

    Ok(json_response(
        200,
        &json!({
            "request_meta": {
                "request_status": "Success",
                "message": "Current algorithm configurations",
            },
            "configurations": configurations,
        }),
    ))
}
