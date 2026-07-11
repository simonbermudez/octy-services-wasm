//! Route handlers — ports of `api/routers/admin.py` and `healthz.py`.
//!
//! Note on rate limits: the FastAPI service used slowapi (`60000/minute` on
//! versioning, `600/minute` on resources/format). Spin components are
//! stateless per request, so enforce those limits at the ingress layer.
//!
//! Note on auth: the FastAPI versioning and resources/format GET routes both
//! required Trusted App auth (client_id/client_secret); this handler doesn't
//! check it, so it must be enforced upstream (the webhook route below is the
//! exception — it authenticates itself via the GitHub HMAC signature).

use hmac::{Hmac, Mac};
use octy_shared::errors::{ErrorReason, OctyError};
use serde_json::{json, Value};
use sha1::Sha1;
use spin_sdk::http::{Params, Request, Response};

use crate::http_util::*;
use crate::repos::versioning as versioning_repository;
use octy_spin::ctx::Ctx;

type HmacSha1 = Hmac<Sha1>;

fn ctx_or_response() -> Result<Ctx, Response> {
    Ctx::load("admin").map_err(|e| error_response(&e))
}

/// K8s pod liveness/readiness probe target — unauthenticated by design.
pub async fn healthz(_req: Request, _params: Params) -> Response {
    json_response(200, &json!("OK"))
}

/// GET /v1/admin/application/versioning?app=api|cli
pub async fn version_info(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let app = query_param(&req, "app");
    let (cache_key, application_type) = match app.as_deref() {
        Some("api") => ("octy-services", "Octy API"),
        Some("cli") => ("octy-cli", "Octy CLI"),
        _ => {
            return error_response(&OctyError::new(
                400,
                "Invalid query string argument",
                vec![ErrorReason::new(
                    "invalid 'app' query parameter provided. Accepted values: 'api' or 'cli'",
                    "",
                )],
            ))
        }
    };

    match versioning_repository::get_cached_version_data(&ctx, cache_key) {
        Ok(versions) => versioning_dto(application_type, versions),
        Err(err) => error_response(&err),
    }
}

/// Constant-time equality, port of `hmac.compare_digest`.
fn compare_digest(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// POST /v1/admin/application/versioning/hook — GitHub release webhook
/// (form-encoded delivery: `payload=<json>`), authenticated with the
/// HMAC-SHA1 `X-Hub-Signature` header.
pub async fn version_info_hook(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let signature = match header_str(&req, "x-hub-signature") {
        Some(sig) => sig.to_string(),
        None => {
            return error_response(&OctyError::new(
                400,
                "Invalid headers provided",
                vec![ErrorReason::new("", "")],
            ))
        }
    };

    let secret = match ctx.secrets.get_str("GITHUB_WH_SECRET") {
        Ok(secret) => secret.to_string(),
        Err(err) => return error_response(&err),
    };

    // HMAC-SHA1 over the raw body, hex digest prefixed "sha1=" like GitHub.
    let mut mac = HmacSha1::new_from_slice(secret.as_bytes()).expect("hmac accepts any key size");
    mac.update(req.body());
    let digest = format!("sha1={}", hex_lower(&mac.finalize().into_bytes()));

    if !compare_digest(digest.as_bytes(), signature.as_bytes()) {
        return error_response(&OctyError::new(
            401,
            "Authentication failed",
            vec![ErrorReason::new(
                "Invalid hook secret provided with this request.",
                "",
            )],
        ));
    }

    // Python: parse_qs(body.decode('utf8').replace("'", '"'))['payload'][0]
    // (quirk preserved: single quotes are rewritten across the whole body
    // *before* the query-string parse).
    let body_str = match String::from_utf8(req.body().to_vec()) {
        Ok(s) => s,
        Err(e) => return error_response(&OctyError::internal(format!("UnicodeDecodeError: {e}"))),
    };
    let body_str = body_str.replace('\'', "\"");
    let payload_str = match url::form_urlencoded::parse(body_str.as_bytes())
        .find(|(key, _)| key == "payload")
        .map(|(_, value)| value.into_owned())
    {
        Some(payload) => payload,
        // Python: KeyError: 'payload' -> 500.
        None => return error_response(&OctyError::internal("KeyError: 'payload'")),
    };
    let wh_payload: Value = match serde_json::from_str(&payload_str) {
        Ok(payload) => payload,
        Err(e) => {
            return error_response(&OctyError::internal(format!(
                "invalid webhook payload JSON: {e}"
            )))
        }
    };

    // Python: wh_payload['repository']['name'] -> KeyError -> 500;
    // `'cli' in <non-str>` -> TypeError -> 500.
    let repository_name = match wh_payload
        .get("repository")
        .and_then(|r| r.get("name"))
        .and_then(Value::as_str)
    {
        Some(name) => name.to_string(),
        None => {
            return error_response(&OctyError::internal(
                "KeyError: webhook payload missing repository.name",
            ))
        }
    };

    let action = wh_payload.get("action");
    if repository_name.contains("cli") {
        // Only cache a cli release once all required assets have been
        // published against it (`edited` event). Missing `action` was
        // swallowed (`except KeyError: pass`) and fell through to caching.
        if let Some(action) = action {
            if action != &json!("edited") {
                return json_response(200, &json!(200));
            }
        }
    } else {
        // Other repos: only cache on 'created' or 'edited'. Quirk preserved:
        // a missing `action` raised KeyError -> 500 on this branch.
        match action {
            Some(a) if a == &json!("created") || a == &json!("edited") => {}
            Some(_) => return json_response(200, &json!(200)),
            None => return error_response(&OctyError::internal("KeyError: 'action'")),
        }
    }

    let repositories = match ctx.config.get_array("REPOSITORIES") {
        Ok(repositories) => repositories.clone(),
        Err(err) => return error_response(&err),
    };

    if repositories.iter().any(|r| r.as_str() == Some(&repository_name)) {
        eprintln!("Caching release for repository {repository_name}");
        let release = match wh_payload.get("release") {
            Some(release) => release,
            None => return error_response(&OctyError::internal("KeyError: 'release'")),
        };
        if let Err(err) = versioning_repository::cache_version_data(&ctx, release, &repository_name)
        {
            return error_response(&err);
        }
        // The Python handler returned None here -> HTTP 200 with body `null`.
        // (In practice it 500'd *after* caching because it `await`ed the sync
        // redis client's int return; the Rust port implements the intent.)
        json_response(200, &Value::Null)
    } else {
        error_response(&OctyError::new(
            400,
            "Invalid repo name provided",
            vec![ErrorReason::new(
                "Not interested in releases on this repository",
                "",
            )],
        ))
    }
}

/// Compact a JSON document exactly like FastAPI's `JSONResponse`
/// (`json.dumps(..., separators=(",", ":"))`) while preserving the key order
/// of the source file: strip all whitespace outside string literals.
fn minify_json(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_string = false;
    let mut escaped = false;
    for c in raw.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

/// GET /v1/admin/application/resources/format?type=events|items|profiles
pub async fn resource_format(req: Request, _params: Params) -> Response {
    let ctx = match ctx_or_response() {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };

    let resource_type = query_param(&req, "type");
    let resource_type = match resource_type.as_deref() {
        Some(t @ ("events" | "items" | "profiles")) => t.to_string(),
        _ => {
            return error_response(&OctyError::new(
                400,
                "Invalid query string argument",
                vec![ErrorReason::new(
                    "invalid 'type' query parameter provided. Accepted values: 'events' or 'items' or 'profiles'",
                    "",
                )],
            ))
        }
    };

    let dir = match ctx.config.get_str("RESOURCE_FORMAT_EXAMPLES_DIR") {
        Ok(dir) => dir.to_string(),
        Err(err) => return error_response(&err),
    };
    let path = format!("{dir}{resource_type}.json");
    println!("{path}"); // the Python handler print()s the path

    // The configured dir is relative ("data/repositories/..."); the files are
    // mounted at "/data/repositories/..." in the component — try both.
    let raw = std::fs::read_to_string(&path)
        .or_else(|_| std::fs::read_to_string(format!("/{}", path.trim_start_matches('/'))));
    let raw = match raw {
        Ok(raw) => raw,
        // Python: FileNotFoundError -> 500.
        Err(e) => {
            return error_response(&OctyError::internal(format!(
                "could not read resource format file {path}: {e}"
            )))
        }
    };

    // Python json.loads()-ed the file (invalid JSON -> 500) and re-serialized
    // it compactly; minifying the raw text keeps the original key order.
    if let Err(e) = serde_json::from_str::<Value>(&raw) {
        return error_response(&OctyError::internal(format!(
            "invalid resource format JSON in {path}: {e}"
        )));
    }

    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(minify_json(&raw).into_bytes())
        .build()
}

pub async fn fallback(_req: Request, _params: Params) -> Response {
    not_found()
}
