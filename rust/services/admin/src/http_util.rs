//! Admin-service DTOs (port of `api/routers/dto/admin.py`). Generic
//! envelopes/validation live in `octy_spin::http_util` and are re-exported so
//! handlers keep a single import.

pub use octy_spin::http_util::*;

use octy_shared::errors::OctyError;
use serde_json::{json, Value};
use spin_sdk::http::Response;

/// Port of `VersioningDTO` — redacts `change_log`/`release_id`, then splits
/// the (already sorted) versions into `current_version` (head of the list)
/// and `previous_versions` (everything whose `id` differs).
///
/// Python quirks preserved: an empty cache raised `IndexError`
/// (`versions[0]`) and a version without an `id` raised `KeyError` — both
/// surfaced as the generic 500 envelope.
pub fn versioning_dto(application_type: &str, mut versions: Vec<Value>) -> Response {
    for version in versions.iter_mut() {
        match version.as_object_mut() {
            Some(obj) => {
                obj.insert("change_log".to_string(), json!("*REDACTED*"));
                obj.insert("release_id".to_string(), json!("*REDACTED*"));
            }
            // Python: `v.update(...)` on a non-dict -> AttributeError -> 500.
            None => {
                return error_response(&OctyError::internal(
                    "cached version entry is not a JSON object",
                ))
            }
        }
    }

    // Python: `current_version = self.versions[0]` -> IndexError -> 500.
    let current = match versions.first() {
        Some(current) => current.clone(),
        None => {
            return error_response(&OctyError::internal(
                "list index out of range (no cached versions for this application)",
            ))
        }
    };
    // Python: `current_version['id']` -> KeyError -> 500.
    let current_id = match current.get("id") {
        Some(id) => id.clone(),
        None => return error_response(&OctyError::internal("KeyError: 'id'")),
    };

    // `[x for x in versions if not (current['id'] == x.get('id'))]` — filters
    // *every* copy of the current id out (including index 0 itself).
    let previous: Vec<&Value> = versions
        .iter()
        .filter(|v| v.get("id").cloned().unwrap_or(Value::Null) != current_id)
        .collect();

    json_response(
        200,
        &json!({
            "request_meta": { "request_status": "Success", "message": "Versioning information found." },
            "application_type": application_type,
            "current_version": current,
            "previous_versions": previous,
        }),
    )
}
