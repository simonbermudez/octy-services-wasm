//! Port of `utils/utils.py` bits the worker needs, plus small helpers.

use octy_shared::errors::OctyError;
use serde_json::Value;
use spin_sdk::http::Method;
use uuid::Uuid;

/// `generate_uid` with the worker's formatting table — the shared
/// `octy_shared::utils::generate_uid` lacks the `hp-t-job` entry (22 chars,
/// `-` separator) that SageMaker job names rely on (max 32 chars).
pub fn generate_uid(prefix: &str) -> String {
    let (len, sep) = match prefix {
        "bucket" => (27usize, '-'),
        "training-job" | "hp-t-job" => (22, '-'),
        "notification" => (20, '-'),
        _ => (34, '_'),
    };
    let uuid = Uuid::new_v4().to_string();
    let tail: String = uuid.chars().take(len).collect();
    format!("{prefix}{sep}{tail}")
}

/// `_required_gb` — walks bytes→KB→MB→GB→TB in steps of 1000.
pub fn required_gb(num_bytes: f64) -> Result<i64, OctyError> {
    let step_unit = 1000.0f64;
    let mut n = num_bytes;
    for unit in ["bytes", "KB", "MB", "GB", "TB"] {
        if n < step_unit {
            eprintln!("{}{}", n as i64, unit);
            return Ok(match unit {
                "GB" => n as i64 + 1, // +1 to avoid a rounding memory discrepancy
                "bytes" | "KB" | "MB" => 1,
                "TB" => (n * 1000.0) as i64 + 1,
                _ => unreachable!(),
            });
        }
        n /= step_unit;
    }
    // Python returned None here; downstream boto3 would then fail.
    Err(OctyError::internal(
        "required_gb: dataset larger than 1000 TB",
    ))
}

/// CPython `sys.getsizeof(str)` approximation for ASCII strings (49 + len) —
/// the Python used it for file-size thresholds and billing.
pub fn py_str_sizeof(len: usize) -> i64 {
    49 + len as i64
}

/// `_get_size({'data': …, 'type': …})` approximation (dict + keys + values).
pub fn py_csv_object_sizeof(data_len: usize, type_len: usize) -> i64 {
    64 + py_str_sizeof(4) + py_str_sizeof(data_len) + py_str_sizeof(4) + py_str_sizeof(type_len)
}

/// GET with the `requests_retry_session` semantics (4 retries on 500/502/504,
/// error once exhausted; other statuses returned to the caller).
pub async fn http_get_with_retry(
    url: &str,
    headers: &[(&str, &str)],
) -> Result<(u16, Vec<u8>), OctyError> {
    let mut last_err = OctyError::internal(format!("request to {url} failed"));
    for _attempt in 0..=4 {
        match octy_spin::gateway::http_send(Method::Get, url, headers, None).await {
            Ok((status, body)) if !matches!(status, 500 | 502 | 504) => return Ok((status, body)),
            Ok((status, _)) => {
                last_err = OctyError::internal(format!("{url} returned status {status}"));
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// POST JSON with retry — thin alias over the shared helper so call sites
/// mirror `_send_http_request`.
pub async fn http_post_json_with_retry(
    url: &str,
    headers: &[(&str, &str)],
    body: &Value,
) -> Result<(u16, Vec<u8>), OctyError> {
    octy_spin::gateway::http_post_json_with_retry(url, headers, body).await
}

/// `str(datetime.now())`-style timestamp for webhook payloads.
pub fn py_now_str() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}
