//! Pure profile-matching/merging logic — port of the dataframe-shaping
//! private methods on `services/profile_identification.py::ProfileIdentification`
//! (`_score_profile`, `_apply_scores`, `_select_parent_profile`,
//! `_specify_child_profiles`, `_merge_segment_tags`,
//! `_parent_profiles_df_numerical_type_conversion`,
//! `_parent_profiles_df_to_formatted_json`).
//!
//! Kept free of `spin-sdk` so it stays natively unit-testable.
//!
//! ## A note on the pandas → per-profile simplification
//!
//! The Python implementation builds a `pandas.json_normalize` dataframe over
//! *all* surviving profiles, which flattens `profile_data.*` / `platform_info.*`
//! into columns that are the **union** of keys across every profile. Any
//! profile missing a given custom key gets `NaN` in that column; when the
//! dataframe is turned back into nested JSON
//! (`_parent_profiles_df_to_formatted_json`), `NaN` values under
//! `profile_data`/`platform_info` are simply *omitted* from the output.
//!
//! That round trip (flatten to the column union → drop NaN cells back out) is
//! behaviourally a no-op for any individual profile: a profile only ever
//! "loses" a key it never had in the first place. So this port operates
//! directly on each surviving profile's own JSON object — no dataframe, no
//! column union — which is observably identical output, without needing a
//! pandas-equivalent crate.

use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

/// Python truthiness (`if value:`), used for the `authenticated_id_key`
/// presence filter (`d['profile_data'].get(self.authenticated_id_key)`).
pub fn is_truthy(v: Option<&Value>) -> bool {
    match v {
        None => false,
        Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// `profile['field'] != None` — present *and* not JSON null (missing keys
/// are treated the same as null; the profiles service always sets these
/// keys, so this only matters defensively).
fn present_not_null(v: Option<&Value>) -> bool {
    !matches!(v, None | Some(Value::Null))
}

/// Port of `utils.str_to_dt` **as used by this worker** — note this is a
/// different format than `octy_shared::utils::str_to_dt` (ISO-8601): the
/// profiles service formats `created_at`/`updated_at` with
/// `int_to_dt(..., as_str=True)`, which renders
/// `%a, %d %b %Y %H:%M:%S GMT` (e.g. `Thu, 09 Jul 2026 12:00:00 GMT`).
///
/// Divergence: the Python raises (crashing the job) on an unparseable
/// non-`None` string. This falls back to `now` instead — a defensive
/// simplification, not a behavioural port, since a malformed timestamp
/// from the profiles service would otherwise abort an entire merge job.
pub fn parse_profile_dt(s: Option<&str>, now: DateTime<Utc>) -> DateTime<Utc> {
    match s {
        None => now,
        Some(s) => NaiveDateTime::parse_from_str(s, "%a, %d %b %Y %H:%M:%S GMT")
            .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
            .unwrap_or(now),
    }
}

/// Port of `_score_profile`.
pub fn score_profile(profile: &Value, now: DateTime<Utc>) -> i64 {
    let mut score = 0i64;

    if present_not_null(profile.get("rfm_score")) {
        score += 1;
    }
    if present_not_null(profile.get("churn_probability")) {
        score += 1;
    }
    if profile.get("status").and_then(Value::as_str) == Some("active") {
        score += 5;
    }

    // `self.time_score_map`: for every (days-ago, points) threshold whose
    // cutoff date is on/after `created_at`, add `points`. Because this is a
    // plain (non-short-circuiting) loop over every entry — not an if/elif
    // chain — a profile older than all five thresholds picks up every one
    // of them (max +9); a very recent profile picks up none. (This inverts
    // the docstring's stated intent — "mature but not too mature" profiles
    // score highest — but it is what the Python does, so it is preserved.)
    let created_at = parse_profile_dt(profile.get("created_at").and_then(Value::as_str), now);
    for (days_ago, points) in [(90i64, 1i64), (60, 2), (30, 3), (20, 2), (10, 1)] {
        let cutoff = now - chrono::Duration::days(days_ago);
        if created_at > cutoff {
            continue; // profile is newer than this threshold: no points
        }
        score += points;
    }

    let updated_at = parse_profile_dt(profile.get("updated_at").and_then(Value::as_str), now);
    let delta_days = (now - updated_at).num_days();
    score += if delta_days <= 5 {
        5
    } else if delta_days <= 10 {
        4
    } else if delta_days <= 30 {
        3
    } else if delta_days <= 90 {
        2
    } else {
        1
    };

    let active_tag_count = profile
        .get("segment_tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter(|t| t.get("status").and_then(Value::as_str) == Some("active"))
                .count()
        })
        .unwrap_or(0);
    score += match active_tag_count {
        0 => 0,
        1 | 2 => 1,
        3..=7 => 2,
        8..=14 => 3,
        _ => 4,
    };

    score
}

/// A resolved merge group: one authenticated-id-key value with 2+ profiles,
/// a chosen parent, and the remaining children to be deleted.
pub struct MergeGroup {
    pub parent_profile_id: String,
    pub child_profile_ids: Vec<String>,
}

/// Port of `_group_profiles` + the scoring/parent/child columns built in
/// `_merge_profiles` + `_drop_null_child_profiles`.
///
/// `profiles` must already be filtered to only those with
/// `profile_data[authenticated_id_key]` set (truthy) — the caller's
/// `< 3 profiles` guard runs on that filtered set, matching
/// `_build_profiles_df`.
///
/// Groups where only a single profile shares the authenticated id (i.e. no
/// merge is needed) are dropped entirely — matching
/// `_drop_null_child_profiles`, which removes those profiles from
/// `self.profiles` outright (they are not sent as `profiles.cmd.update`,
/// since there is nothing to merge).
pub fn build_merge_groups(
    profiles: &[Value],
    authenticated_id_key: &str,
    now: DateTime<Utc>,
) -> Vec<MergeGroup> {
    // Preserve first-appearance order both across groups and within each
    // group — pandas' groupby-apply(list) preserves the original row order
    // within each group, which is what the stable parent-selection sort
    // below relies on for deterministic tie-breaking.
    let mut group_order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();

    for profile in profiles {
        let Some(key) = profile
            .get("profile_data")
            .and_then(|pd| pd.get(authenticated_id_key))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let profile_id = profile.get("profile_id").and_then(Value::as_str).unwrap_or_default();
        groups
            .entry(key.to_string())
            .or_insert_with(|| {
                group_order.push(key.to_string());
                Vec::new()
            })
            .push(profile_id.to_string());
    }

    let mut merge_groups = Vec::new();
    for key in group_order {
        let ids = groups.remove(&key).unwrap_or_default();
        if ids.len() < 2 {
            // Lone profile for this authenticated id: nothing to merge.
            continue;
        }

        // Stable sort descending by score (ties keep original/insertion
        // order — matches `sorted(scores, key=scores.get, reverse=True)`).
        let mut scored: Vec<(String, i64)> = ids
            .iter()
            .map(|id| {
                let profile = profiles
                    .iter()
                    .find(|p| p.get("profile_id").and_then(Value::as_str) == Some(id.as_str()))
                    .expect("profile_id present in grouped profiles");
                (id.clone(), score_profile(profile, now))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        let parent_profile_id = scored[0].0.clone();

        let child_profile_ids: Vec<String> =
            ids.into_iter().filter(|id| id != &parent_profile_id).collect();

        merge_groups.push(MergeGroup {
            parent_profile_id,
            child_profile_ids,
        });
    }

    merge_groups
}

/// Port of `_merge_segment_tags` (per group): union of active segment tags,
/// parent's own tags first, then each child's in order, first occurrence of
/// a `segment_id` wins. Output tags are reshaped to exactly
/// `{segment_id, segment_tag, status: "active"}` — matches `_append_tag`,
/// which drops every other field (`created_at`, `updated_at`, ...).
pub fn merge_segment_tags(parent: &Value, children: &[&Value]) -> Vec<Value> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    let mut consume = |tags_value: Option<&Value>| {
        let Some(tags) = tags_value.and_then(Value::as_array) else {
            return;
        };
        for tag in tags {
            if tag.get("status").and_then(Value::as_str) != Some("active") {
                continue;
            }
            let segment_id = tag.get("segment_id").and_then(Value::as_str).unwrap_or_default();
            if !seen.insert(segment_id.to_string()) {
                continue;
            }
            result.push(json!({
                "segment_id": segment_id,
                "segment_tag": tag.get("segment_tag").cloned().unwrap_or(Value::Null),
                "status": "active",
            }));
        }
    };

    consume(parent.get("segment_tags"));
    for child in children {
        consume(child.get("segment_tags"));
    }

    result
}

/// `(key, type_)` pairs from the account's `{account_id}_profile_key_types`
/// Redis set — `et['type_']` is the literal `str(type(value))` the Python
/// service stored (`"<class 'int'>"` / `"<class 'float'>"` / anything else).
pub type KeyTypes = [(String, String)];

/// Port of `_change_dtype`. Returns the value unchanged if no configured
/// type matches the field, or if the coercion fails the way Python's
/// `int()`/`float()` would raise `ValueError` (kept as the original value,
/// caught by the Python `except ValueError`).
///
/// Divergence: `int(None)` / `float(None)` raise `TypeError` in Python,
/// which is *not* caught by the `except ValueError` clause — an untyped
/// `null` custom field whose key happens to be registered as int/float
/// would crash the whole job. This port leaves `null` values unchanged
/// instead of replicating that crash.
pub fn change_dtype(field: &str, value: &Value, types: &KeyTypes) -> Value {
    for (key, type_) in types {
        if key != field {
            continue;
        }
        return match type_.as_str() {
            "<class 'int'>" => try_int(value),
            "<class 'float'>" => try_float(value),
            _ => continue,
        };
    }
    value.clone()
}

fn try_int(value: &Value) -> Value {
    match value {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                json!(i)
            } else if let Some(f) = n.as_f64() {
                json!(f.trunc() as i64)
            } else {
                value.clone()
            }
        }
        Value::String(s) => match s.trim().parse::<i64>() {
            Ok(i) => json!(i),
            Err(_) => value.clone(),
        },
        Value::Bool(b) => json!(if *b { 1 } else { 0 }),
        _ => value.clone(),
    }
}

fn try_float(value: &Value) -> Value {
    match value {
        Value::Number(n) => n.as_f64().map(|f| json!(f)).unwrap_or_else(|| value.clone()),
        Value::String(s) => match s.trim().parse::<f64>() {
            Ok(f) => json!(f),
            Err(_) => value.clone(),
        },
        Value::Bool(b) => json!(if *b { 1.0 } else { 0.0 }),
        _ => value.clone(),
    }
}

/// Port of the tail of `_merge_profiles` that shapes a surviving parent
/// profile for the `profiles.cmd.update` message: drop `created_at` /
/// `updated_at`, force `rfm_score` to an int (default `0`), coerce
/// `profile_data.*` / `platform_info.*` per the account's registered key
/// types, and install the merged segment tags.
pub fn format_parent_profile(profile: &Value, merged_segment_tags: Vec<Value>, key_types: &KeyTypes) -> Value {
    let mut map: Map<String, Value> = match profile {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };

    map.remove("created_at");
    map.remove("updated_at");

    let rfm_score = match map.get("rfm_score") {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)).unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse::<i64>().unwrap_or(0),
        _ => 0,
    };
    map.insert("rfm_score".to_string(), json!(rfm_score));

    for field_name in ["profile_data", "platform_info"] {
        let converted = match map.get(field_name) {
            Some(Value::Object(obj)) => {
                let mut converted = Map::new();
                for (k, v) in obj {
                    converted.insert(k.clone(), change_dtype(k, v, key_types));
                }
                Value::Object(converted)
            }
            _ => json!({}),
        };
        map.insert(field_name.to_string(), converted);
    }

    map.insert("segment_tags".to_string(), Value::Array(merged_segment_tags));

    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 9, 12, 0, 0).unwrap()
    }

    fn profile(id: &str, auth_value: &str, overrides: Value) -> Value {
        let mut base = json!({
            "profile_id": id,
            "customer_id": format!("cust-{id}"),
            "profile_data": { "email": auth_value },
            "platform_info": {},
            "rfm_score": Value::Null,
            "churn_probability": Value::Null,
            "status": "active",
            "created_at": "Thu, 09 Jul 2026 12:00:00 GMT",
            "updated_at": "Thu, 09 Jul 2026 12:00:00 GMT",
            "segment_tags": [],
        });
        if let (Value::Object(base_map), Value::Object(over_map)) = (&mut base, overrides) {
            for (k, v) in over_map {
                base_map.insert(k, v);
            }
        }
        base
    }

    #[test]
    fn is_truthy_matches_python() {
        assert!(!is_truthy(None));
        assert!(!is_truthy(Some(&Value::Null)));
        assert!(!is_truthy(Some(&json!(""))));
        assert!(!is_truthy(Some(&json!(0))));
        assert!(!is_truthy(Some(&json!(false))));
        assert!(is_truthy(Some(&json!("x"))));
        assert!(is_truthy(Some(&json!(1))));
    }

    #[test]
    fn scores_active_status_and_rfm_present() {
        let p = profile(
            "p1",
            "a@b.com",
            json!({ "rfm_score": 10, "churn_probability": "low", "status": "active" }),
        );
        // rfm(+1) + churn(+1) + active(+5) + created 0d old (0 pts, newer than
        // all thresholds) + updated 0d old (+5) + 0 active tags (+0) = 12
        assert_eq!(score_profile(&p, now()), 12);
    }

    #[test]
    fn scores_old_profile_higher_on_age_thresholds() {
        let old = profile(
            "p1",
            "a@b.com",
            json!({ "created_at": "Mon, 01 Jan 2024 12:00:00 GMT", "updated_at": "Mon, 01 Jan 2024 12:00:00 GMT" }),
        );
        // status active (+5) + all 5 age thresholds triggered (1+2+3+2+1=9)
        // + updated >90d ago (+1) = 15
        assert_eq!(score_profile(&old, now()), 15);
    }

    #[test]
    fn build_merge_groups_picks_highest_scorer_and_keeps_lone_profiles_out() {
        let profiles = vec![
            profile("p1", "shared@x.com", json!({ "status": "churned" })),
            profile("p2", "shared@x.com", json!({ "status": "active" })), // higher score
            profile("p3", "lonely@x.com", json!({})),
        ];
        let groups = build_merge_groups(&profiles, "email", now());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].parent_profile_id, "p2");
        assert_eq!(groups[0].child_profile_ids, vec!["p1".to_string()]);
    }

    #[test]
    fn stable_tie_break_keeps_first_seen_profile_as_parent() {
        let profiles = vec![
            profile("p1", "shared@x.com", json!({})),
            profile("p2", "shared@x.com", json!({})),
        ];
        let groups = build_merge_groups(&profiles, "email", now());
        assert_eq!(groups[0].parent_profile_id, "p1");
    }

    #[test]
    fn merge_segment_tags_dedupes_by_segment_id_parent_wins() {
        let parent = json!({
            "segment_tags": [
                { "segment_id": "s1", "segment_tag": "vip", "status": "active" },
                { "segment_id": "s2", "segment_tag": "churn-risk", "status": "inactive" },
            ]
        });
        let child = json!({
            "segment_tags": [
                { "segment_id": "s1", "segment_tag": "vip-child-stale", "status": "active" },
                { "segment_id": "s3", "segment_tag": "new-tag", "status": "active" },
            ]
        });
        let merged = merge_segment_tags(&parent, &[&child]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0]["segment_id"], "s1");
        assert_eq!(merged[0]["segment_tag"], "vip"); // parent's value wins
        assert_eq!(merged[1]["segment_id"], "s3");
    }

    #[test]
    fn change_dtype_coerces_registered_keys_only() {
        let types = vec![("age".to_string(), "<class 'int'>".to_string())];
        assert_eq!(change_dtype("age", &json!("42"), &types), json!(42));
        assert_eq!(change_dtype("age", &json!("not-a-number"), &types), json!("not-a-number"));
        assert_eq!(change_dtype("other", &json!("42"), &types), json!("42"));
    }

    #[test]
    fn format_parent_profile_drops_dates_and_defaults_rfm() {
        let p = profile("p1", "a@b.com", json!({}));
        let formatted = format_parent_profile(&p, vec![], &[]);
        assert!(formatted.get("created_at").is_none());
        assert!(formatted.get("updated_at").is_none());
        assert_eq!(formatted["rfm_score"], json!(0));
        assert_eq!(formatted["profile_data"]["email"], json!("a@b.com"));
    }
}
