//! Port of the `ItemPrice` class from `services/template_engine.py` plus the
//! `currencies` PyPI package's `Currency(...).get_money_format(...)` for the
//! codes Octy accepts (`Config['ALLOWED_CURRENCIES']`).
//!
//! Kept bug-for-bug: the constructor divides the raw item price by 100 and
//! `format()` divides by 100 *again* (`int(self.amount)/100`), exactly like
//! the Python.

use serde_json::Value;

use crate::http_util::MsgError;

/// `str(float)` the way Python renders it (integral floats keep a `.0`).
pub fn py_float_str(f: f64) -> String {
    if f.is_finite() && f == f.trunc() && f.abs() < 1e16 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

/// `money_format` strings from the `currencies` package (Shopify-derived).
/// Unknown codes raise `KeyError` in Python → generic 500 here.
fn money_format_template(code: &str) -> Option<&'static str> {
    Some(match code {
        "USD" | "AUD" | "CAD" | "NZD" | "SGD" | "HKD" | "TWD" | "ARS" | "CLP" | "COP" => {
            "${amount}"
        }
        "MXN" => "$ {amount}",
        "EUR" => "€{amount}",
        "GBP" => "£{amount}",
        "JPY" | "CNY" => "¥{amount}",
        "INR" => "Rs. {amount}",
        "CHF" => "SFr. {amount}",
        "SEK" => "{amount} kr",
        "NOK" => "kr {amount}",
        "DKK" => "{amount}kr",
        "ZAR" => "R {amount}",
        "BRL" => "R$ {amount}",
        "PLN" => "{amount} zl",
        "RUB" => "{amount} руб",
        "TRY" => "{amount}TL",
        "AED" => "Dhs. {amount}",
        "ILS" => "{amount} NIS",
        "KRW" => "₩{amount}",
        "THB" => "{amount} ฿",
        "IDR" => "Rp {amount}",
        "MYR" => "RM{amount} MYR",
        "PHP" => "₱{amount}",
        "CZK" => "{amount} Kč",
        "HUF" => "{amount} Ft",
        "RON" => "{amount} lei",
        "BGN" => "{amount} лв",
        "HRK" => "{amount} kn",
        "UAH" => "₴{amount}",
        "VND" => "{amount}₫",
        "PEN" => "S/. {amount}",
        "EGP" => "LE {amount}",
        "NGN" => "₦{amount}",
        "KES" => "KSh{amount}",
        "SAR" => "{amount} SR",
        "QAR" => "QR {amount}",
        _ => return None,
    })
}

pub fn get_money_format(code_upper: &str, amount: f64) -> Result<String, MsgError> {
    let template = money_format_template(code_upper).ok_or_else(|| {
        MsgError::internal(format!("KeyError: unknown currency code '{code_upper}'"))
    })?;
    Ok(template.replace("{amount}", &py_float_str(amount)))
}

/// Python `round(x, 2)` — banker's rounding at two decimal places.
fn py_round2(x: f64) -> f64 {
    (x * 100.0).round_ties_even() / 100.0
}

/// `ItemPrice(params, amount, currency_rates).format()`.
///
/// * `params` — the request data value `profile_id::currency_from::currency_to::discount`
/// * `raw_amount` — the item's `item_price` attribute (cents)
/// * `currency_rates` — the latest `tbl_currency_rates` doc's `rates` value
///   (may be absent — the Python then dies with a `TypeError`, so a missing
///   value here is a 500 on the conversion path only).
pub fn item_price_format(
    params: &str,
    raw_amount: f64,
    currency_rates: Option<&Value>,
) -> Result<String, MsgError> {
    let mut amount = raw_amount / 100.0;
    let parts: Vec<&str> = params.split("::").collect();
    let discount: i64 = parts
        .get(3)
        .and_then(|p| p.trim().parse::<i64>().ok())
        .ok_or_else(|| MsgError::internal("invalid item_price discount parameter"))?;
    if discount > 0 {
        amount -= (discount as f64 / 100.0) * amount;
    }

    let currency_from = *parts
        .get(1)
        .ok_or_else(|| MsgError::internal("IndexError: item_price currency_from"))?;
    let currency_to = *parts
        .get(2)
        .ok_or_else(|| MsgError::internal("IndexError: item_price currency_to"))?;

    // `int(self.amount) / 100` — truncation, then the second /100 (quirk kept).
    let base_amount = amount.trunc() / 100.0;

    if currency_from == currency_to {
        get_money_format(&currency_from.to_uppercase(), base_amount)
    } else {
        let converted = currency_conversion(currency_from, currency_to, base_amount, currency_rates)?;
        get_money_format(&currency_to.to_uppercase(), converted)
    }
}

/// `_currency_conversion` — looks up `rates[currency_to]['rates'][currency_from]`
/// and multiplies (direction preserved from the Python).
fn currency_conversion(
    currency_from: &str,
    currency_to: &str,
    amount: f64,
    currency_rates: Option<&Value>,
) -> Result<f64, MsgError> {
    if currency_from == currency_to {
        return Ok(py_round2(amount));
    }
    // Missing rates / codes crash with TypeError in the Python → 500.
    let rates = currency_rates
        .ok_or_else(|| MsgError::internal("TypeError: currency rates unavailable"))?;
    let base = rates
        .get(currency_to)
        .ok_or_else(|| MsgError::internal("TypeError: no rates for target currency"))?;
    let exchange = base
        .get("rates")
        .and_then(|r| r.get(currency_from))
        .and_then(Value::as_f64)
        .ok_or_else(|| MsgError::internal("TypeError: no exchange rate for source currency"))?;
    Ok(py_round2(amount * exchange))
}
