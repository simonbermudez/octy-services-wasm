//! Port of `data/repositories/implementation/notifications_repository.py`.
//! Mailjet is a plain HTTPS API, called directly from the component via
//! Spin outbound HTTP; the notification reference lands in Mongo through the
//! data gateway.

use base64::Engine;
use chrono::Utc;
use octy_shared::ejson::legacy_date;
use octy_shared::errors::OctyError;
use octy_shared::utils::generate_uid;
use serde_json::{json, Value};
use spin_sdk::http::Method;

use octy_spin::ctx::Ctx;
use octy_spin::gateway::http_send;

const COLLECTION: &str = "tbl_notifications";
const MAILJET_SEND_URL: &str = "https://api.mailjet.com/v3.1/send";

pub struct EmailPayload {
    pub contact_email_address: String,
    pub contact_name: String,
    pub subject: String,
    pub body: String,
}

fn mailjet_message(ctx: &Ctx, payload: &EmailPayload, to_email: &str, notification_id: &str) -> Result<Value, OctyError> {
    Ok(json!({
        "Messages": [{
            "From": { "Email": ctx.config.get_str("SUPPORT_EMAIL")?, "Name": "Octy.ai" },
            "To": [{ "Email": to_email, "Name": payload.contact_name }],
            "Subject": payload.subject,
            "TextPart": payload.body,
            "HTMLPart": "",
            "CustomID": notification_id,
        }]
    }))
}

async fn mailjet_send(ctx: &Ctx, message: &Value) -> Result<u16, OctyError> {
    let auth = base64::engine::general_purpose::STANDARD.encode(format!(
        "{}:{}",
        ctx.secrets.get_str("MAIL_JET_API_KEY")?,
        ctx.secrets.get_str("MAIL_JET_API_SECRET")?
    ));
    let (status, _) = http_send(
        Method::Post,
        MAILJET_SEND_URL,
        &[
            ("content-type", "application/json"),
            ("authorization", &format!("Basic {auth}")),
        ],
        Some(serde_json::to_vec(message).expect("serializable json")),
    )
    .await?;
    Ok(status)
}

/// `NotificationsRepository.email` — sends to the account contact and to
/// ops@octy.ai, then records the notification. Returns `did_succeed`.
pub async fn email(ctx: &Ctx, account: &Value, payload: EmailPayload) -> bool {
    let notification_id = generate_uid("notification");

    let did_succeed = async {
        let to_client = mailjet_message(ctx, &payload, &payload.contact_email_address, &notification_id)?;
        let to_octy = mailjet_message(ctx, &payload, "ops@octy.ai", &notification_id)?;
        let client_status = mailjet_send(ctx, &to_client).await?;
        let octy_status = mailjet_send(ctx, &to_octy).await?;
        Ok::<bool, OctyError>(client_status == 200 && octy_status == 200)
    }
    .await
    .unwrap_or(false);

    create_notification_ref(
        ctx,
        account,
        &json!(payload.body),
        "email",
        &payload.contact_email_address,
        &notification_id,
        did_succeed,
    )
    .await;

    did_succeed
}

async fn create_notification_ref(
    ctx: &Ctx,
    account: &Value,
    content: &Value,
    notification_type: &str,
    destination: &str,
    notification_id: &str,
    did_succeed: bool,
) {
    // Failures are swallowed, mirroring the Python try/except + sentry capture.
    let _ = ctx
        .gateway
        .insert_one(
            COLLECTION,
            json!({
                "notification_id": notification_id,
                "account_id": account.get("account_id").cloned().unwrap_or(Value::Null),
                "notification_content": content.to_string(),
                "notification_type": notification_type,
                "destination": destination,
                "did_succeed": did_succeed,
                "created_at": legacy_date(Utc::now()),
            }),
        )
        .await;
}
