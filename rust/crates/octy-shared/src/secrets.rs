//! Port of `account/app_secrets.py` — same base64-encoded JSON blob contract
//! (e.g. the `ACCOUNT_SECRETS` environment variable).

use crate::config::Config;
use crate::errors::OctyError;

/// Secrets share the exact loading semantics of [`Config`]; the distinction is
/// kept so call sites read the same as the Python (`Secrets['DB_URI']` vs
/// `Config['ENV']`).
pub type Secrets = Config;

pub fn secrets_from_encoded(encoded: &str) -> Result<Secrets, OctyError> {
    Config::from_encoded(encoded)
}
