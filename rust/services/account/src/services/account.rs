//! Port of `services/account.py::AccountService`.

use octy_shared::errors::{ErrorReason, OctyError};
use octy_shared::models::CreateAccount;
use octy_shared::utils::generate_uid;
use serde_json::{json, Value};

use octy_spin::ctx::Ctx;
use octy_spin::gateway::http_post_json_with_retry;
use crate::repos::{account as account_repo, content, notifications};

/// `create_account` — create the Mongo document, provision + configure the S3
/// bucket, email the API keys, and enqueue the account's Octy jobs.
/// Returns the response payload consumed by `CreateAccountDTO`.
pub async fn create_account(ctx: &Ctx, account: &CreateAccount) -> Result<Value, OctyError> {
    let bucket_name = generate_uid("bucket");

    // NOTE (carried over from a Python TODO): the account row is written
    // before its bucket exists. If bucket creation/config fails below we
    // roll the account back, but there's a window where the account exists
    // without a usable bucket. Consider provisioning the bucket first.
    let (new_account, sk) = account_repo::create_account(ctx, account, &bucket_name).await?;
    let account_id = new_account["account_id"].as_str().unwrap_or_default().to_string();

    // Create and configure the bucket; on failure roll the account back.
    if !ctx.gateway.create_bucket(&bucket_name).await {
        account_repo::delete_account(ctx, &account_id).await.ok();
        return Err(OctyError::new(
            500,
            "Internal Server Error",
            vec![ErrorReason::new("Bucket could not be created.", "")],
        ));
    }
    if !ctx.gateway.configure_bucket(&bucket_name, &account_id).await {
        account_repo::delete_account(ctx, &account_id).await.ok();
        return Err(OctyError::new(
            500,
            "Internal Server Error",
            vec![ErrorReason::new("Bucket could not be configured", "")],
        ));
    }

    // Pre-create the folder skeleton (raw_data, training_job_data, templates,
    // models, ...) that downstream services (recommendations, churn, rfm,
    // ltv training jobs) expect to already exist in the account's bucket.
    for dir in ctx.config.get_array("BUCKET_REQUIRED_DIRS")? {
        if let Some(dir) = dir.as_str() {
            ctx.gateway.create_directory(&bucket_name, dir).await;
        }
    }

    // Email the API keys to the new account holder.
    let configurations = &new_account["account_configurations"];
    let contact_name = configurations["contact_name"].as_str().unwrap_or_default().to_string();
    let contact_email = configurations["contact_email_address"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let public_key = new_account["keys"]["public_key"].as_str().unwrap_or_default();

    let notification_sent = notifications::email(
        ctx,
        &new_account,
        notifications::EmailPayload {
            contact_email_address: contact_email.clone(),
            contact_name: contact_name.clone(),
            subject: content::ACCOUNT_SUBJECT.to_string(),
            body: content::account_body(
                &contact_name,
                ctx.config.get_str("DOCS_ROOT_URL")?,
                public_key,
                &sk,
            ),
        },
    )
    .await;

    // Enqueue the account's recurring Octy jobs.
    for job in ctx.config.get_array("OCTY_JOBS")? {
        ctx.gateway
            .amqp_publish(
                "octy.job.cmd.create",
                &json!({
                    "account_id": new_account["account_id"],
                    "job_meta": job["job_meta"],
                    "job_data": job["job_data"],
                }),
            )
            .await?;
    }

    Ok(json!({
        "account_id": new_account["account_id"],
        "account_name": new_account["account_name"],
        "account_type": configurations["account_type"],
        "account_currency": configurations["account_currency"],
        "contact_email_address": contact_email,
        "pk": public_key,
        "sk": sk,
        "notification_sent": notification_sent,
    }))
}

/// `delete_account` — remove the bucket, the Mongo document + Redis cache,
/// then fan the deletion out to every downstream service.
pub async fn delete_account(ctx: &Ctx, account_id: &str) -> Result<bool, OctyError> {
    let account = account_repo::get_account_by_account_id(ctx, account_id)
        .await?
        .ok_or_else(|| {
            OctyError::new(
                400,
                "Bad request",
                vec![ErrorReason::new(format!("No account found with id {account_id}"), "")],
            )
        })?;

    let bucket = account["bucket"].as_str().unwrap_or_default();
    if !ctx.gateway.delete_bucket(bucket).await {
        return Err(OctyError::new(
            500,
            "Internal Server Error",
            vec![ErrorReason::new("Bucket could not be deleted.", "")],
        ));
    }

    account_repo::delete_account(ctx, account_id).await?;

    let payload = json!({ "account_id": account_id });
    let cleanups: &[(&str, &str)] = &[
        ("EVENTS_SERVICE_CLUSTER_IP", "/v1/internal/events/delete"),
        ("PROFILES_SERVICE_CLUSTER_IP", "/v1/internal/profiles/delete"),
        ("OCTY_JOBS_SERVICE_CLUSTER_IP", "/v1/internal/jobs/delete"),
        ("ITEMS_SERVICE_CLUSTER_IP", "/v1/internal/items/delete"),
        ("RECOMMENDATION_SERVICE_CLUSTER_IP", "/v1/internal/recommendations/delete"),
        ("SEGMENTATION_SERVICE_CLUSTER_IP", "/v1/internal/segments/delete"),
        ("CHURN_PREDICTION_SERVICE_CLUSTER_IP", "/v1/internal/churn_prediction/delete"),
    ];
    for (config_key, path) in cleanups {
        let base = ctx.config.get_str(config_key)?;
        let url = format!("{base}{path}");
        http_post_json_with_retry(&url, &[("cursor", "0")], &payload)
            .await
            .map_err(|e| {
                OctyError::new(
                    500,
                    "Internal Server Error",
                    vec![ErrorReason::new(
                        format!("{path} cleanup failed: {e}"),
                        "",
                    )],
                )
            })?;
    }

    Ok(true)
}

/// `get_accounts_internal`.
pub async fn get_accounts_internal(
    ctx: &Ctx,
    account_ids: &[Value],
    cursor: i64,
) -> Result<(Vec<Value>, i64), OctyError> {
    let (accounts, total) = account_repo::get_accounts(ctx, account_ids, cursor).await?;
    if accounts.is_empty() {
        return Err(OctyError::new(
            400,
            "No accounts found",
            vec![ErrorReason::new(
                "No accounts found with provided params or pagination cursor exhausted",
                "",
            )],
        ));
    }
    Ok((accounts, total))
}
