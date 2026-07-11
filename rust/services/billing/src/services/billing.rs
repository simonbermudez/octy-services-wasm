//! Port of `services/billing.py::BillingService`.

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use octy_shared::errors::OctyError;
use serde_json::{json, Value};

use octy_spin::ctx::Ctx;

use crate::models::UnitsChild;
use crate::repos::billing as billing_repo;
use crate::repos::billing::BillingFilters;

/// `dt.strptime(s, '%Y-%m-%d')` — naive midnight, treated as UTC (the Python
/// containers run with TZ=UTC). A malformed date raised an uncaught
/// `ValueError` in the Python route → generic 500, mirrored here.
fn parse_created_at(raw: &str) -> Result<DateTime<Utc>, OctyError> {
    let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
        OctyError::internal(format!(
            "time data '{raw}' does not match format '%Y-%m-%d'"
        ))
    })?;
    Ok(Utc.from_utc_datetime(&date.and_time(NaiveTime::MIN)))
}

/// `calculate_persist_billable_units` — price each unit from `Config['UNITS']`
/// and bulk-persist. `[toxic]::`-prefixed errors mark poison messages the
/// AMQP layer must reject without requeueing.
pub async fn calculate_persist_billable_units(
    ctx: &Ctx,
    billable_units: &[UnitsChild],
) -> Result<(), OctyError> {
    let unit_types = ctx.config.get_array("UNITS")?;

    let mut units: Vec<Value> = Vec::with_capacity(billable_units.len());
    for unit in billable_units {
        let unit_type = unit_types
            .iter()
            .find(|u| u.get("unit_type").and_then(Value::as_str) == Some(unit.unit_type.as_str()))
            .ok_or_else(|| {
                OctyError::internal(format!(
                    "[toxic]:: Unknown unit type provided : {}",
                    unit.unit_type
                ))
            })?;

        let metrics = unit_type
            .get("metrics")
            .and_then(Value::as_array)
            .ok_or_else(|| OctyError::internal("config UNITS entry missing metrics"))?;
        let metric = metrics
            .iter()
            .find(|m| m.get("name").and_then(Value::as_str) == Some(unit.metric.as_str()))
            .ok_or_else(|| {
                OctyError::internal(format!(
                    "[toxic]:: Unknown metric provided : {} for unit type: {}",
                    unit.metric, unit.unit_type
                ))
            })?;

        let all_costs = metric
            .get("costs")
            .ok_or_else(|| OctyError::internal("config UNITS metric missing costs"))?;

        // costs[account_currency] with a fallback to GBP (KeyError branch); a
        // missing GBP entry was an uncaught KeyError → non-toxic (requeued).
        let (costs, currency) = match all_costs.get(&unit.account_currency) {
            Some(costs) => (costs, unit.account_currency.as_str()),
            None => (
                all_costs.get("GBP").ok_or_else(|| {
                    OctyError::internal(format!(
                        "no costs for currency {} and no GBP fallback",
                        unit.account_currency
                    ))
                })?,
                "GBP",
            ),
        };

        let fee = costs.get(&unit.account_type).ok_or_else(|| {
            OctyError::internal(format!(
                "[toxic]:: Unknown account type provided : {}",
                unit.account_type
            ))
        })?;

        // fee * quantity — int * int stays int (like Python); float fees
        // produce a float total.
        let (cost_per_unit, total_cost) = if let Some(f) = fee.as_i64() {
            (json!(f), json!(f * unit.quantity))
        } else if let Some(f) = fee.as_f64() {
            (json!(f), json!(f * unit.quantity as f64))
        } else {
            return Err(OctyError::internal(format!(
                "non-numeric cost configured for account type {}",
                unit.account_type
            )));
        };

        units.push(json!({
            "account_id": unit.account_id,
            "account_type": unit.account_type,
            "process_name": unit.process_name,
            "unit_type": unit.unit_type,
            "metric": unit.metric,
            "quantity": unit.quantity,
            "cost_per_unit": cost_per_unit,
            "total_cost": total_cost,
            "currency": currency,
        }));
    }

    billing_repo::create_billable_units_ref(ctx, &units).await
}

/// Query parameters for `get_billable_units` (already split/deduped lists).
#[derive(Debug, Default)]
pub struct GetBillableUnitsParams {
    pub account_ids: Option<Vec<String>>,
    pub account_types: Option<Vec<String>>,
    pub unit_types: Option<Vec<String>>,
    pub metrics: Option<Vec<String>>,
    pub process_names: Option<Vec<String>>,
    pub cost_upper_range: Option<i64>,
    pub cost_lower_range: Option<i64>,
    pub currencies: Option<Vec<String>>,
    pub created_at_upper_range: Option<String>,
    pub created_at_lower_range: Option<String>,
}

/// `get_billable_units` — build the filters dict and delegate to the repo.
/// Python truthiness quirks preserved: empty lists, empty strings and a
/// 0-valued cost range are all treated as "no filter".
pub async fn get_billable_units(
    ctx: &Ctx,
    params: GetBillableUnitsParams,
    cursor: i64,
) -> Result<(Vec<Value>, i64), OctyError> {
    fn non_empty(list: Option<Vec<String>>) -> Option<Vec<String>> {
        list.filter(|l| !l.is_empty())
    }
    fn truthy_int(v: Option<i64>) -> Option<i64> {
        v.filter(|v| *v != 0)
    }
    fn non_empty_str(v: Option<String>) -> Option<String> {
        v.filter(|s| !s.is_empty())
    }

    let mut filters = BillingFilters {
        account_ids: non_empty(params.account_ids),
        account_types: non_empty(params.account_types),
        unit_types: non_empty(params.unit_types),
        metrics: non_empty(params.metrics),
        process_names: non_empty(params.process_names),
        cost_upper_range: truthy_int(params.cost_upper_range),
        cost_lower_range: truthy_int(params.cost_lower_range),
        currencies: non_empty(params.currencies),
        ..Default::default()
    };
    if let Some(raw) = non_empty_str(params.created_at_upper_range) {
        filters.created_at_upper_range = Some(parse_created_at(&raw)?);
    }
    if let Some(raw) = non_empty_str(params.created_at_lower_range) {
        filters.created_at_lower_range = Some(parse_created_at(&raw)?);
    }

    billing_repo::filter_billable_units(ctx, &filters, cursor).await
}

/// `delete_account_billing_internal` — delete all billable units for the account.
pub async fn delete_account_billing_internal(ctx: &Ctx, account_id: &str) -> Result<bool, OctyError> {
    billing_repo::delete_account_billable_units(ctx, account_id).await
}
