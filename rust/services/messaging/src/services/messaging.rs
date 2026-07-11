//! Port of `services/messaging.py` — `MessagingService`.

use octy_shared::errors::{ErrorReason, OctyError};
use octy_shared::utils::generate_uid;
use serde_json::Value;

use octy_spin::auth::AuthAccount;
use octy_spin::ctx::Ctx;

use crate::http_util::MsgError;
use crate::models::{CreateTemplates, DeleteTemplates, UpdateTemplates};
use crate::repos::templates as templates_repo;
use crate::repos::templates::TemplateBatchEntry;

/// The messaging service treats `b.a_id` from the JWT as a plain string.
pub fn account_id_str(account: &AuthAccount) -> String {
    account.account_oid().unwrap_or_default().to_string()
}

/// `assess_resource_limit` from `utils/utils.py` — the `li` blob is
/// `*`-separated as `profiles*items*event_types*events*segments*mes_templates`
/// (e.g. `50000*150*100*100000*25*50`); index 5 is the message-template limit.
fn assess_resource_limit(
    limits: &str,
    current_count: i64,
    requested: i64,
) -> Result<(bool, i64, i64), OctyError> {
    let resource_limit: i64 = limits
        .split('*')
        .nth(5)
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| OctyError::internal("invalid resource limits string in account configurations"))?;
    let remainder = resource_limit - current_count;
    if requested + current_count > resource_limit {
        Ok((false, resource_limit, remainder))
    } else {
        Ok((true, resource_limit, remainder))
    }
}

/// `MessagingService.get_templates`
pub async fn get_templates(
    ctx: &Ctx,
    account: &AuthAccount,
    identifiers: Option<Vec<String>>,
    cursor: i64,
) -> Result<(Vec<Value>, i64), OctyError> {
    let account_id = account_id_str(account);
    let help = ctx.config.opt_str("MESSAGING_EXTENDED_HELP").unwrap_or("").to_string();

    match identifiers {
        Some(ids) if cursor == 0 => {
            let (templates, total) =
                templates_repo::get_templates(ctx, &account_id, Some(&ids), 0).await?;
            if total < 1 {
                return Err(OctyError::new(
                    400,
                    "Invalid template identifier(s) provided",
                    vec![ErrorReason::new(
                        "No templates were found with the provided identifier(s)",
                        help,
                    )],
                ));
            }
            Ok((templates, total))
        }
        None => {
            let (templates, total) =
                templates_repo::get_templates(ctx, &account_id, None, cursor).await?;
            if templates.is_empty() {
                return Err(OctyError::new(
                    400,
                    "No templates found",
                    vec![ErrorReason::new(
                        "No templates found with the provided query parameters or pagination cursor exhausted",
                        help,
                    )],
                ));
            }
            Ok((templates, total))
        }
        // The Python fell off the end and returned None → 500 on unpack.
        _ => Err(OctyError::internal(
            "get_templates called with identifiers and a non-zero cursor",
        )),
    }
}

/// `MessagingService.create_templates`
pub async fn create_templates(
    ctx: &Ctx,
    account: &AuthAccount,
    templates: &CreateTemplates,
) -> Result<(Vec<Value>, Vec<Value>), MsgError> {
    let account_id = account_id_str(account);

    // KeyError → 500 in the Python when `li` missing.
    let limits = account
        .account_configurations
        .get("li")
        .and_then(Value::as_str)
        .ok_or_else(|| MsgError::internal("account_configurations missing 'li'"))?
        .to_string();

    let current_count = templates_repo::get_template_count(ctx, &account_id)
        .await
        .map_err(MsgError::Octy)?;
    let (ok, limit, remainder) =
        assess_resource_limit(&limits, current_count, templates.templates.len() as i64)
            .map_err(MsgError::Octy)?;
    if !ok {
        return Err(MsgError::Octy(OctyError::new(
            400,
            "Resource limit exceeded",
            vec![ErrorReason::new(
                format!(
                    "This request could not be completed as the number of templates sent with this request exceeds the allowed limit of : {limit}. This account can create another {remainder} templates."
                ),
                ctx.config.opt_str("RATE_LIMIT_EXTENDED_HELP").unwrap_or(""),
            )],
        )));
    }

    let batch: Vec<TemplateBatchEntry> = templates
        .templates
        .iter()
        .map(|t| TemplateBatchEntry {
            template_id: generate_uid("template"),
            account_id: account_id.clone(),
            friendly_name: t.friendly_name.clone(),
            template_type: t.template_type.clone(),
            title: t.title.clone(),
            content: t.content.clone(),
            default_values: t.default_values.clone(),
            metadata: t.metadata.clone(),
        })
        .collect();

    let (created, failed) = templates_repo::create_templates(ctx, &batch)
        .await
        .map_err(MsgError::Octy)?;

    if created.is_empty() {
        return Err(MsgError::Raw {
            code: 400,
            reason: "No templates created!".to_string(),
            errors: failed,
        });
    }

    Ok((created, failed))
}

/// `MessagingService.update_templates`
pub async fn update_templates(
    ctx: &Ctx,
    account: &AuthAccount,
    templates: &UpdateTemplates,
) -> Result<(Vec<Value>, Vec<Value>), MsgError> {
    let account_id = account_id_str(account);

    let batch: Vec<TemplateBatchEntry> = templates
        .templates
        .iter()
        .map(|t| TemplateBatchEntry {
            template_id: t.template_id.clone().unwrap_or_default(),
            account_id: account_id.clone(),
            friendly_name: t.friendly_name.clone(),
            template_type: t.template_type.clone(),
            title: t.title.clone(),
            content: t.content.clone(),
            default_values: t.default_values.clone(),
            metadata: t.metadata.clone(),
        })
        .collect();

    let (updated, failed) = templates_repo::update_templates(ctx, &batch).await?;

    if updated.is_empty() {
        return Err(MsgError::Raw {
            code: 400,
            reason: "No templates updated!".to_string(),
            errors: failed,
        });
    }

    Ok((updated, failed))
}

/// `MessagingService.delete_templates`
pub async fn delete_templates(
    ctx: &Ctx,
    account: &AuthAccount,
    templates: &DeleteTemplates,
) -> Result<(Vec<Value>, Vec<Value>), MsgError> {
    let account_id = account_id_str(account);
    let batch: Vec<(String, String)> = templates
        .template_ids
        .iter()
        .map(|tid| (tid.clone(), account_id.clone()))
        .collect();

    let (deleted, failed) = templates_repo::delete_templates(ctx, &batch)
        .await
        .map_err(MsgError::Octy)?;

    if deleted.is_empty() {
        return Err(MsgError::Raw {
            code: 400,
            reason: "No templates deleted!".to_string(),
            errors: failed,
        });
    }

    Ok((deleted, failed))
}

/// `MessagingService.delete_account_messaging_internal`.
///
/// NB: the Python route was broken end-to-end (constructor called with an
/// unexpected `account_id` kwarg; the repository methods it referenced —
/// `delete_messages` / `delete_templates_by_account_id` — did not exist), so
/// it always 500'd. This implements the *intended* behaviour: remove every
/// template owned by the account. (There is no persisted message content to
/// remove — generated messages are never stored.)
pub async fn delete_account_messaging_internal(
    ctx: &Ctx,
    account_id: &str,
) -> Result<bool, OctyError> {
    templates_repo::delete_account_templates(ctx, account_id).await
}
