//! Outbound HTTP — ports of `utils.requests_retry_session` call sites in
//! `services/profile_identification.py` and
//! `data/repositories/implementation/profiles_iden_repository.py`.
//!
//! These hit the `profiles` and `octy-jobs` services directly (service DNS,
//! not through `octy-data-gateway` — only Mongo/AMQP/S3 need the gateway),
//! reusing `octy_spin::gateway::http_post_json_with_retry` for the
//! 500/502/504 retry behaviour of `requests_retry_session`.

use octy_shared::errors::OctyError;
use octy_spin::gateway::http_post_json_with_retry;
use serde_json::{json, Value};

/// Port of `profilesIdenRepository.get_profiles`: paginate
/// `POST {PROFILE_SERVICE_CLUSTER_IP}/v1/internal/profiles` via the `cursor`
/// header until the service returns a non-200 (exhausted / no more pages —
/// same terminator the Python used).
pub async fn get_profiles(profile_service_url: &str, account_id: &str, status: &str) -> Result<Vec<Value>, OctyError> {
    let url = format!("{profile_service_url}/v1/internal/profiles?ids=false&status={status}");
    let body = json!({ "account_id": account_id, "profiles": [], "get_all": true });

    let mut profiles = Vec::new();
    let mut cursor: i64 = 0;
    loop {
        let cursor_str = cursor.to_string();
        let (status_code, resp_body) =
            http_post_json_with_retry(&url, &[("cursor", &cursor_str)], &body).await?;
        if status_code != 200 {
            break;
        }
        let parsed: Value = serde_json::from_slice(&resp_body)
            .map_err(|e| OctyError::internal(format!("invalid profiles response: {e}")))?;
        let count = parsed["request_meta"]["count"].as_i64().unwrap_or(0);
        let page = parsed["profiles"].as_array().cloned().unwrap_or_default();
        if page.is_empty() {
            // Safety valve against a stuck cursor (count not advancing);
            // the Python relied solely on the non-200 terminator.
            break;
        }
        profiles.extend(page);
        cursor += count;
        if count <= 0 {
            break;
        }
    }
    Ok(profiles)
}

/// Port of `_send_http_request` — the job-service completion callback.
/// Unlike the webhook call, failures here propagate (the Python re-raised).
pub async fn post_job_callback(
    job_service_url: &str,
    account_id: &str,
    octy_job_id: &str,
    message: &str,
    status: &str,
) -> Result<(), OctyError> {
    let url = format!("{job_service_url}/v1/internal/jobs/callback");
    let body = json!({
        "account_id": account_id,
        "octy_job_id": octy_job_id,
        "message": message,
        "status": status,
    });
    // The Python sends a `cursor: 0` header on this call too (an artifact of
    // reusing `_send_http_request` for both paginated and non-paginated
    // calls); kept for parity, though the job service ignores it here.
    http_post_json_with_retry(&url, &[("cursor", "0")], &body).await?;
    Ok(())
}

/// Port of `_send_http_account_webhook_request` — best-effort; the Python
/// caught and logged every exception here instead of propagating it.
pub async fn post_webhook_best_effort(webhook_url: &str, payload: &Value) {
    if let Err(err) = http_post_json_with_retry(webhook_url, &[], payload).await {
        eprintln!("[profile-identification-worker] webhook POST to {webhook_url} failed: {err}");
    }
}
