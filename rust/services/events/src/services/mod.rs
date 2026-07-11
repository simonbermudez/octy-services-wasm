pub mod billing;
pub mod event_types;
pub mod events;

use octy_spin::auth::AuthAccount;
use serde_json::Value;

use crate::http_util::ApiError;

/// The account id the Python code read off the decoded JWT (`b.a_id`),
/// reusing the same `{"$oid": …}` / plain-string handling as the account
/// service (`AuthAccount::account_oid`).
pub fn account_id_str(account: &AuthAccount) -> Result<String, ApiError> {
    account
        .account_oid()
        .map(String::from)
        .ok_or_else(|| ApiError::internal("account_id claim is not a string"))
}

pub struct LimitCounts {
    pub limit: i64,
    pub remainder: i64,
}

/// Port of `utils.assess_resource_limit` — `limits` is the packed string from
/// the JWT (`"50000*150*100*100000*25*50"`), `resource_key` selects the slot.
/// Malformed limits raised IndexError/ValueError → generic 500 in Python.
pub fn assess_resource_limit(
    limits: &str,
    current_count: i64,
    requested: i64,
    resource_key: usize,
) -> Result<(bool, LimitCounts), ApiError> {
    let part = limits
        .split('*')
        .nth(resource_key)
        .ok_or_else(|| ApiError::internal(format!("IndexError: resource limit slot {resource_key}")))?;
    let resource_limit: i64 = part
        .trim()
        .parse()
        .map_err(|_| ApiError::internal(format!("ValueError: invalid resource limit '{part}'")))?;
    let remainder = resource_limit - current_count;

    if requested + current_count > resource_limit {
        Ok((
            false,
            LimitCounts {
                limit: resource_limit,
                remainder,
            },
        ))
    } else {
        Ok((
            true,
            LimitCounts {
                limit: resource_limit,
                remainder: remainder - requested,
            },
        ))
    }
}

/// `self.account.account_configurations['li']` — KeyError → generic 500.
pub fn account_limits(account: &AuthAccount) -> Result<String, ApiError> {
    account
        .account_configurations
        .get("li")
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| ApiError::internal("KeyError: account_configurations['li']"))
}
