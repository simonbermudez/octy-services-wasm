//! Port of `services/items.py` (`ItemsService`).

use octy_shared::errors::{ErrorReason, OctyError};
use octy_spin::auth::AuthAccount;
use octy_spin::ctx::Ctx;
use serde_json::{json, Value};

use crate::errors::ApiError;
use crate::models::{CreateItems, DeleteItems, UpdateItems};
use crate::repos::items as repo;
use crate::services::billing::BillingUnits;

pub struct ItemsService<'a> {
    ctx: &'a Ctx,
    account: Option<&'a AuthAccount>,
    billing: Option<BillingUnits>,
}

/// `utils.assess_resource_limit` — index 1 of the `li` limits string is the
/// items limit.
struct LimitCounts {
    limit: i64,
    remainder: i64,
}

fn assess_resource_limit(
    limits: &str,
    current_count: i64,
    requested: i64,
) -> Result<(bool, LimitCounts), OctyError> {
    let raw = limits
        .split('*')
        .nth(1)
        .ok_or_else(|| OctyError::internal("IndexError: list index out of range"))?;
    let resource_limit: i64 = raw.trim().parse().map_err(|_| {
        OctyError::internal(format!("invalid resource limit value: {raw}"))
    })?;
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

impl<'a> ItemsService<'a> {
    /// `ItemsService(account)` — the Python constructor eagerly built
    /// `BillingUnits` from `a_cf['a_t']` / `a_cf['a_c']` (KeyError → 500).
    pub fn new(ctx: &'a Ctx, account: &'a AuthAccount) -> Result<Self, OctyError> {
        let billing = BillingUnits::for_account(account, "items_data")?;
        Ok(Self {
            ctx,
            account: Some(account),
            billing: Some(billing),
        })
    }

    /// `ItemsService(None)` / `ItemsService(account_id=…)` — internal routes.
    pub fn internal(ctx: &'a Ctx) -> Self {
        Self {
            ctx,
            account: None,
            billing: None,
        }
    }

    fn items_help(&self) -> Result<&str, OctyError> {
        self.ctx.config.get_str("ITEMS_EXTENDED_HELP")
    }

    fn account(&self) -> &AuthAccount {
        self.account.expect("authenticated route")
    }

    pub async fn get_items(
        &mut self,
        item_ids: Option<&[String]>,
        cursor: i64,
    ) -> Result<(Vec<Value>, i64), ApiError> {
        let account_id = self.account().account_id.clone();

        if let Some(ids) = item_ids {
            if cursor == 0 {
                let items = repo::get_item_by_ids(self.ctx, ids, &account_id).await?;
                let count = items.len() as i64;
                if count < 1 {
                    return Err(OctyError::new(
                        400,
                        "Invalid item identifier(s) provided",
                        vec![ErrorReason::new(
                            "No items were found with the provided identifier(s)",
                            self.items_help()?,
                        )],
                    )
                    .into());
                }
                return Ok((items, count));
            }
        } else {
            let (items, total) =
                repo::get_items(self.ctx, &account_id, cursor, false, "all").await?;
            if items.len() < 1 {
                return Err(OctyError::new(
                    400,
                    "No items found",
                    vec![ErrorReason::new(
                        "No items found with the provided item identifier or pagination cursor exhausted",
                        self.items_help()?,
                    )],
                )
                .into());
            }
            return Ok((items, total));
        }

        // Python fell through returning None (item_ids with nonzero cursor
        // never happens — the router always passes cursor=0 with ids) and the
        // DTO then crashed with a TypeError → 500.
        Err(OctyError::internal("TypeError: cannot unpack non-iterable NoneType object").into())
    }

    pub async fn create_items(
        &mut self,
        items: CreateItems,
    ) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
        let account = self.account();
        let account_id = account.account_id.clone();

        // assess allowed limits
        let li = account
            .account_configurations
            .get("li")
            .and_then(Value::as_str)
            .ok_or_else(|| OctyError::internal("KeyError: 'li'"))?
            .to_string();
        let current_count = repo::get_item_count(self.ctx, &account_id).await?;
        let (res, counts) = assess_resource_limit(&li, current_count, items.items.len() as i64)?;
        if !res {
            let rate_help = self.ctx.config.get_str("RATE_LIMIT_EXTENDED_HELP")?;
            return Err(OctyError::new(
                400,
                "Resource limit exceeded",
                vec![ErrorReason::new(
                    format!(
                        "This request could not be completed as the number of items sent with this request exceeds the allowed limit of : {}. This account can create another {} items.",
                        counts.limit, counts.remainder
                    ),
                    rate_help,
                )],
            )
            .into());
        }

        let items_batch: Vec<Value> = items
            .items
            .iter()
            .map(|item| {
                json!({
                    "item_id": item.item_id,
                    "account_id": account_id,
                    "item_category": item.item_category,
                    "item_name": item.item_name,
                    "item_description": item.item_description,
                    "item_price": item.item_price,
                    "event_type": "charged",
                })
            })
            .collect();

        let (created, failed) = repo::create_items(self.ctx, &items_batch).await?;

        if created.len() < 1 {
            return Err(ApiError::raw(400, "No items created!", failed));
        }

        let billing = self.billing.as_mut().expect("billing on authenticated route");
        billing.track_data_units(&json!(created));
        billing.complete_data_units(self.ctx, "MB").await?;

        Ok((created, failed))
    }

    pub async fn update_items(
        &mut self,
        items: UpdateItems,
    ) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
        let account_id = self.account().account_id.clone();

        let items_batch: Vec<Value> = items
            .items
            .iter()
            .map(|item| {
                json!({
                    "item_id": item.item_id,
                    "account_id": account_id,
                    "item_category": item.item_category,
                    "item_name": item.item_name,
                    "item_description": item.item_description,
                    "item_price": item.item_price,
                    "status": item.status,
                    "event_type": "charged",
                })
            })
            .collect();

        let items_help = self.items_help()?.to_string();
        let (updated, failed) =
            repo::update_items(self.ctx, &items_batch, &account_id, &items_help).await?;

        if updated.len() < 1 {
            return Err(ApiError::raw(400, "No items updated!", failed));
        }

        let billing = self.billing.as_mut().expect("billing on authenticated route");
        billing.track_data_units(&json!(updated));
        billing.complete_data_units(self.ctx, "MB").await?;

        Ok((updated, failed))
    }

    pub async fn delete_items(
        &mut self,
        items: DeleteItems,
    ) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
        let account = self.account();
        let account_id = account.account_id.clone();

        let items_batch: Vec<Value> = items
            .items
            .iter()
            .map(|item| {
                json!({
                    "item_id": item,
                    "account_id": account_id,
                })
            })
            .collect();

        let (deleted, failed) = repo::delete_items(self.ctx, &items_batch, account).await?;

        if deleted.len() < 1 {
            return Err(ApiError::raw(400, "No items deleted!", failed));
        }
        Ok((deleted, failed))
    }

    // ---- INTERNAL ----

    pub async fn get_items_internal(
        &mut self,
        account_id: &str,
        cursor: i64,
        ids: bool,
        status: &str,
    ) -> Result<(Vec<Value>, i64), ApiError> {
        let (items, total) =
            repo::get_items(self.ctx, &json!(account_id), cursor, ids, status).await?;
        if items.len() < 1 {
            return Err(OctyError::new(
                400,
                "No items found",
                vec![ErrorReason::new(
                    "No items found or pagination cursor exhausted",
                    self.items_help()?,
                )],
            )
            .into());
        }
        Ok((items, total))
    }

    /// Delete all items for an account (account-deletion fan-out).
    pub async fn delete_account_items_internal(&mut self, account_id: &str) -> Result<bool, ApiError> {
        Ok(repo::delete_account_items_internal(account_id).await?)
    }
}
