//! Port of the RFM-specific helpers from `utils/utils.py` that are not
//! already covered by `octy_shared::utils` (whose `str_to_dt` targets a
//! different date format used by the account service).
//!
//! `generate_uid` is byte-identical across every Python service, so we reuse
//! `octy_shared::utils::generate_uid` rather than re-implementing it here.

use chrono::{DateTime, NaiveDateTime, Utc};

/// Port of `utils.utils.str_to_dt`:
/// `dt.strptime(dt_str, '%a, %d %b %Y %H:%M:%S GMT')`
/// (an RFC-1123-style HTTP date, as returned by the events service).
pub fn str_to_dt(dt_str: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(dt_str, "%a, %d %b %Y %H:%M:%S GMT")
        .ok()
        .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

/// Format a datetime the way `pandas.DataFrame.to_csv` renders a
/// whole-second `Timestamp` column (no microseconds present in our source
/// data, so this always matches pandas' default `%Y-%m-%d %H:%M:%S`).
pub fn dt_to_csv_field(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Port of `RFMAnalysis._required_gb`: convert a byte count into the number
/// of whole GB required for the SageMaker training volume (minimum 1 GB,
/// TB-scale values converted to their GB equivalent).
pub fn required_gb(mut num_bytes: f64) -> i64 {
    let step_unit = 1000.0_f64;
    let units = ["bytes", "KB", "MB", "GB", "TB"];
    for unit in units {
        if num_bytes < step_unit {
            return match unit {
                "GB" => (num_bytes + 1.0) as i64,
                "bytes" | "KB" | "MB" => 1,
                "TB" => ((num_bytes * 1000.0) + 1.0) as i64,
                _ => 1,
            };
        }
        num_bytes /= step_unit;
    }
    // num_bytes stayed >= step_unit through every unit (i.e. exceeds the TB
    // bucket too) — mirrors the Python function's implicit `None` return
    // becoming a `TypeError` further up; we clamp to the TB branch instead
    // to keep the Rust port total, since a silent None-propagation isn't
    // meaningful in Rust.
    ((num_bytes * 1000.0) + 1.0) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http_date() {
        let dt = str_to_dt("Wed, 01 Jan 2025 12:30:45 GMT").unwrap();
        assert_eq!(dt.format("%Y-%m-%d %H:%M:%S").to_string(), "2025-01-01 12:30:45");
    }

    #[test]
    fn required_gb_small_is_one() {
        assert_eq!(required_gb(500.0), 1);
        assert_eq!(required_gb(15_000_000.0), 1);
    }

    #[test]
    fn required_gb_rounds_up() {
        // 2.5 GB -> int(2.5)+1 = 3 (mirrors the Python int() truncation + 1)
        assert_eq!(required_gb(2_500_000_000.0), 3);
    }
}
