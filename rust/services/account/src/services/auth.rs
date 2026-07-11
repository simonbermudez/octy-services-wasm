//! Port of `services/auth.py::AuthService`.

use octy_shared::errors::{ErrorReason, OctyError};
use octy_shared::utils::basic_auth_parse;
use serde_json::json;
use spin_sdk::http::Request;

use octy_spin::ctx::Ctx;
use octy_spin::http_util::client_addr;
use crate::repos::{account as account_repo, auth as auth_repo, content, notifications};

fn auth_error(ctx: &Ctx, message: &str) -> OctyError {
    OctyError::new(
        401,
        "Authentication failed",
        vec![ErrorReason::new(
            message,
            ctx.config.opt_str("AUTH_EXTENDED_HELP").unwrap_or(""),
        )],
    )
}

/// `validate_auth_request_headers` — presence + `pk_…`/`sk_…` format checks.
/// Returns the parsed `(pk, sk)` so the caller doesn't re-parse the header.
pub fn validate_auth_request_headers(ctx: &Ctx, req: &Request) -> Result<(String, String), OctyError> {
    let Some(token) = req.header("authorization").and_then(|v| v.as_str()) else {
        return Err(OctyError::new(
            400,
            "Missing header",
            vec![ErrorReason::new(
                "[Authorization] : [Basic ...] header must be provided in request headers.",
                ctx.config.opt_str("AUTH_EXTENDED_HELP").unwrap_or(""),
            )],
        ));
    };

    let (ok, pk, sk) = basic_auth_parse(token);
    if !ok {
        return Err(auth_error(
            ctx,
            "Please provide public and secret keys, encoded as a basic authorization token, within the 'Authorization' header of this request.",
        ));
    }
    if pk.is_empty() {
        return Err(auth_error(
            ctx,
            "Please provide your Octy public key (username), encoded as a basic authorization token, within the Authorization header of this request.",
        ));
    }
    if sk.is_empty() {
        return Err(auth_error(
            ctx,
            "Please provide your Octy secret key (password), encoded as a basic authorization token, within the Authorization header of this request.",
        ));
    }

    let re_pk = regex_lite::Regex::new(r"^pk_[a-zA-Z0-9]").unwrap();
    let re_sk = regex_lite::Regex::new(r"^sk_[a-zA-Z0-9]").unwrap();
    if !re_pk.is_match(&pk) || !re_sk.is_match(&sk) {
        return Err(auth_error(ctx, "Invalid public_key or secret_key provided"));
    }

    Ok((pk, sk))
}

/// `authenticatation` — refresh the cache, verify the keys, mint the fat JWT.
pub async fn authenticate(ctx: &Ctx, req: &Request, pk: &str, sk: &str) -> Result<String, OctyError> {
    account_repo::refresh_account_data_cache(ctx, pk).await?;

    let (valid_pk, valid_sk, account) = auth_repo::verify_account_keys(ctx, pk, sk)?;
    if !valid_pk || !valid_sk {
        log_failed_auth(ctx, req, valid_pk, pk).await;
        return Err(auth_error(ctx, "Invalid public_key or secret_key provided"));
    }

    let account = account.expect("account present when both keys valid");
    auth_repo::generate_authorization_token(ctx, &account)
}

/// `_log_failed_auth` — record the attempt (only when the public key was
/// valid) and alert the account holder past the failed-attempt threshold.
async fn log_failed_auth(ctx: &Ctx, req: &Request, valid_pk: bool, pk: &str) {
    if !valid_pk {
        return;
    }

    let user_agent = req
        .header("user-agent")
        .and_then(|v| v.as_str())
        .unwrap_or("Not supplied")
        .to_string();
    let (server_name, server_port) = client_addr(req);

    let failed_attempt = json!({
        "public_key": pk,
        "user_agent": user_agent,
        "server_name": server_name,
        "server_port": server_port,
        "request_type": req.method().to_string(),
    });

    let attempts = auth_repo::log_failed_auth(ctx, &failed_attempt).await;

    let limit = ctx
        .config
        .get_i64("FAILED_AUTH_ATTEMPT_LIMIT")
        .unwrap_or(i64::MAX);
    if (attempts.len() as i64) <= limit {
        return;
    }

    let Ok(Some(account)) = ctx
        .gateway
        .find_one("tbl_accounts", json!({ "keys.public_key": pk }))
        .await
    else {
        return;
    };

    let configurations = &account["account_configurations"];
    let support_email = ctx.config.opt_str("SUPPORT_EMAIL").unwrap_or("support@octy.ai");
    notifications::email(
        ctx,
        &account,
        notifications::EmailPayload {
            contact_email_address: configurations["contact_email_address"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            contact_name: configurations["contact_name"].as_str().unwrap_or_default().to_string(),
            subject: content::AUTH_SECURITY_WARNING_SUBJECT.to_string(),
            body: content::auth_security_warning_body(limit, support_email),
        },
    )
    .await;
}
