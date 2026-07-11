//! Small helpers ported from `profiles/utils/utils.py` that are not already
//! covered by `octy_shared::utils` (generic across every service).

use octy_shared::utils::int_to_dt;
use serde_json::Value;

/// Port of `assess_resource_limit`.
///
/// `limits` is the `*`-delimited limits string carried in the fat JWT's
/// `a_cf.li` claim (`profiles*items*event_types*events*segments*mes_templates`).
/// Returns `(within_limit, counts)`.
pub fn assess_resource_limit(limits: &str, current_count: i64, requested: i64) -> (bool, Value) {
    let resource_limit: i64 = limits
        .split('*')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let remainder = resource_limit - current_count;
    let exceeded_by = requested - remainder;

    if requested + current_count > resource_limit {
        (
            false,
            serde_json::json!({
                "limit": resource_limit,
                "count_before": current_count,
                "count_after": current_count,
                "remainder": remainder,
                "exceeded_by": exceeded_by,
            }),
        )
    } else {
        (
            true,
            serde_json::json!({
                "limit": resource_limit,
                "count_before": current_count,
                "count_after": current_count + requested,
                "remainder": remainder - requested,
                "exceeded_by": exceeded_by,
            }),
        )
    }
}

/// Port of `int_to_dt(dt_int, as_str=True)` — epoch-millis to the
/// `'%a, %d %b %Y %H:%M:%S GMT'` formatted string used for `created_at` /
/// `updated_at` / `merged_at` in API responses.
///
/// NOTE: the Python implementation calls `datetime.fromtimestamp` (server
/// **local** time) but formats the literal suffix `"GMT"` regardless of the
/// server's actual timezone. Containers run with `TZ=UTC`, so local time and
/// UTC coincide in practice; this port always uses UTC.
pub fn int_to_dt_str(millis: i64) -> Option<String> {
    int_to_dt(millis).map(|dt| dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
}

/// Port of `validate_arg_format` (rfm query-string parsing): `"int-int"`.
/// Returns `(ok, values)`.
pub fn validate_arg_format(arg: &str) -> (bool, Vec<i64>) {
    if !arg.contains('-') {
        return (false, vec![]);
    }
    if arg.matches('-').count() > 1 {
        return (false, vec![]);
    }
    let mut values = Vec::new();
    for score in arg.split('-') {
        if score.is_empty() {
            continue;
        }
        match score.parse::<i64>() {
            Ok(v) => values.push(v),
            Err(_) => return (false, vec![]),
        }
    }
    (true, values)
}

/// Dedupe while preserving order (`dict.fromkeys`), dropping empty entries,
/// trimming leading/trailing whitespace on each identifier.
pub fn dedupe_identifiers(raw: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for part in raw.split(',') {
        let trimmed = part.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.clone()) {
            out.push(trimmed);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_limit_within_bounds() {
        let (ok, counts) = assess_resource_limit("100*50*10*1000*5*5", 10, 20);
        assert!(ok);
        assert_eq!(counts["count_after"], 30);
        assert_eq!(counts["remainder"], 70);
    }

    #[test]
    fn resource_limit_exceeded() {
        let (ok, counts) = assess_resource_limit("100*50*10*1000*5*5", 90, 20);
        assert!(!ok);
        assert_eq!(counts["limit"], 100);
        assert_eq!(counts["remainder"], 10);
    }

    #[test]
    fn arg_format_parses_range() {
        assert_eq!(validate_arg_format("1-5"), (true, vec![1, 5]));
        assert_eq!(validate_arg_format("no-dash-here"), (false, vec![]));
        assert_eq!(validate_arg_format("nodash"), (false, vec![]));
    }

    #[test]
    fn dedupe_identifiers_trims_and_filters() {
        assert_eq!(
            dedupe_identifiers(" a , b ,a,, c"),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }
}
