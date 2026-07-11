//! Port of `data/repositories/implementation/versioning_repository.py`.
//!
//! Release metadata is cached in Redis **sets** (one set per repository name,
//! members are JSON blobs) in **db 0** — same keys as the Python service, so
//! both implementations can share the cache during the migration.

use std::cmp::Ordering;

use octy_shared::errors::OctyError;
use octy_shared::utils::generate_uid;
use octy_spin::ctx::Ctx;
use serde_json::{json, Value};

/// `db_context.py` opened redis with `db=0`.
const REDIS_DB: u32 = 0;

fn redis_conn(ctx: &Ctx) -> Result<spin_sdk::redis::Connection, OctyError> {
    spin_sdk::redis::Connection::open(&ctx.redis_address(REDIS_DB)?)
        .map_err(|e| OctyError::internal(format!("redis connect failed: {e:?}")))
}

/// Missing-key accessor mirroring Python `data['key']` (KeyError -> 500).
fn require<'v>(data: &'v Value, key: &str) -> Result<&'v Value, OctyError> {
    data.get(key)
        .ok_or_else(|| OctyError::internal(format!("KeyError: '{key}'")))
}

/// Port of `cache_version_data(data, repository_name)` — `data` is the
/// `release` object of the GitHub webhook payload.
pub fn cache_version_data(ctx: &Ctx, data: &Value, repository_name: &str) -> Result<(), OctyError> {
    let tag = require(data, "tag_name")?
        .as_str()
        // Python: `'beta' in <non-str>` -> TypeError -> 500.
        .ok_or_else(|| OctyError::internal("release 'tag_name' is not a string"))?;

    // version_int: the tag with a fixed character set stripped, cast to int.
    // The Python loops iterate over the *strings* "v . - b e t a" /
    // "v . - a l p h a", so the stripped sets include the space character.
    let mut version_int_str = tag.to_string();
    if tag.contains("beta") {
        version_int_str.retain(|c| !"v .-beta".contains(c));
    } else if tag.contains("alpha") {
        version_int_str.retain(|c| !"v .-alpha".contains(c));
    }
    // Quirk preserved: stable tags are int()-ed *unstripped*, so a stable
    // "v1.2.3" raised ValueError -> 500 in Python too.
    let version_int: i64 = version_int_str.trim().parse().map_err(|_| {
        OctyError::internal(format!(
            "invalid literal for int() with base 10: '{version_int_str}'"
        ))
    })?;

    let version_data = json!({
        "id": generate_uid(&format!("{repository_name}-release")),
        "release_id": require(data, "id")?,
        "version_tag": tag,
        "version_name": require(data, "name")?,
        "version_int": version_int,
        "change_log": require(data, "body")?,
        "assets": require(data, "assets")?,
        "published_at": require(data, "published_at")?,
        // Python used naive `dt.now()` (container TZ = UTC in deployment).
        "updated_at": chrono::Utc::now().format("%m-%d-%YT%H:%M:%S").to_string(),
    });

    let member = serde_json::to_string(&version_data)
        .map_err(|e| OctyError::internal(format!("could not serialize version data: {e}")))?;

    redis_conn(ctx)?
        .sadd(repository_name, &[member])
        .map_err(|e| OctyError::internal(format!("redis sadd failed: {e:?}")))?;
    Ok(())
}

/// Value ordering for the sort keys (`updated_at` strings / `version_int`
/// numbers) — Python compared them natively via `operator.itemgetter`.
fn cmp_json(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::String(x), Value::String(y)) => x.cmp(y),
        (Value::Number(x), Value::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        _ => Ordering::Equal,
    }
}

/// Port of the inner `sort_versions`: stable sort by `updated_at` desc, then
/// stable sort by `version_int` desc (two passes, like the Python).
fn sort_versions(mut versions: Vec<Value>) -> Result<Vec<Value>, OctyError> {
    for version in &versions {
        // itemgetter raised KeyError -> 500 when the fields were missing.
        require(version, "updated_at")?;
        require(version, "version_int")?;
    }
    versions.sort_by(|a, b| cmp_json(&b["updated_at"], &a["updated_at"]));
    versions.sort_by(|a, b| cmp_json(&b["version_int"], &a["version_int"]));
    Ok(versions)
}

/// Port of `get_cached_version_data(key)` — returns stable releases first,
/// then betas, then alphas, each group sorted newest-first.
pub fn get_cached_version_data(ctx: &Ctx, key: &str) -> Result<Vec<Value>, OctyError> {
    let members = redis_conn(ctx)?
        .smembers(key)
        .map_err(|e| OctyError::internal(format!("redis smembers failed: {e:?}")))?;

    let mut alpha_versions = Vec::new();
    let mut beta_versions = Vec::new();
    let mut stable_versions = Vec::new();

    for member in members {
        let version: Value = serde_json::from_str(&member)
            .map_err(|e| OctyError::internal(format!("invalid cached version JSON: {e}")))?;
        let tag = version
            .get("version_tag")
            .and_then(Value::as_str)
            // Python: KeyError / `'alpha' in <non-str>` TypeError -> 500.
            .ok_or_else(|| OctyError::internal("KeyError: 'version_tag'"))?
            .to_string();

        if tag.contains("alpha") {
            alpha_versions.push(version);
        } else if tag.contains("beta") {
            beta_versions.push(version);
        } else {
            stable_versions.push(version);
        }
    }

    let mut sorted_versions = Vec::new();
    sorted_versions.extend(sort_versions(stable_versions)?);
    sorted_versions.extend(sort_versions(beta_versions)?);
    sorted_versions.extend(sort_versions(alpha_versions)?);
    Ok(sorted_versions)
}
