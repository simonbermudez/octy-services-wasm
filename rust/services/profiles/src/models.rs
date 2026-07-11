//! Pydantic-equivalent request models, ported from
//! `api/routers/request_models/profiles.py` (HTTP request bodies) and
//! `data/models/profiles.py` / `data/models/segment_tags.py` (AMQP payloads).
//!
//! Both HTTP and AMQP variants converge on the same
//! [`ProfileUpdateInput`]/[`SegmentTagInput`] shape consumed by
//! `services::profiles`, mirroring how the Python `ProfilesService` accepted
//! either pydantic model interchangeably (attribute-compatible duck typing).

use serde::Deserialize;
use serde_json::Value;

use octy_shared::errors::OctyError;
use octy_shared::models::validation_error;

fn field_error(loc: &[&str], msg: impl Into<String>) -> Value {
    serde_json::json!({ "loc": loc, "msg": msg.into(), "type": "value_error" })
}

fn jsondecode_error(msg: impl std::fmt::Display) -> OctyError {
    validation_error(vec![serde_json::json!({
        "loc": ["body"], "msg": msg.to_string(), "type": "value_error.jsondecode"
    })])
}

/// Port of `disallow_null_values` — top-level values may not be `null` or an
/// empty/whitespace-only string.
fn disallow_null_values(value: &Value, attribute: &str, loc_prefix: &[&str]) -> Vec<Value> {
    let mut errors = Vec::new();
    let Some(obj) = value.as_object() else {
        return errors;
    };
    for (k, v) in obj {
        let invalid = match v {
            Value::Null => true,
            Value::String(s) => s.is_empty() || s.trim().is_empty(),
            _ => false,
        };
        if invalid {
            let mut loc = loc_prefix.to_vec();
            loc.push(attribute);
            errors.push(field_error(
                &loc,
                format!(
                    "Invalid {attribute} attribute provided. Null values or empty strings can not be provided as {attribute} values. Invalid key pair value: ({k} : {v})"
                ),
            ));
        }
    }
    errors
}

/// Port of `CreateProfilesChild.validate_customer_id` /
/// `UpdateProfilesChild.validate_customer_id` (identical body in both).
fn validate_customer_id(value: &str, loc: &[&str]) -> Option<Value> {
    if value.chars().count() > 60 || value.chars().count() < 1 {
        return Some(field_error(
            loc,
            "Customer identifiers must be at least 1 character long and less than 60 characters long.",
        ));
    }
    let disallowed = [',', '"', '\'', '.'];
    let found: Vec<char> = disallowed.iter().copied().filter(|c| value.contains(*c)).collect();
    if !found.is_empty() {
        return Some(field_error(
            loc,
            format!(
                "Illegal character(s) found in provided customer identifier : {:?}",
                found.iter().map(|c| c.to_string()).collect::<Vec<_>>()
            ),
        ));
    }
    None
}

const ALLOWED_STATUSES: &[&str] = &["active", "inactive", "churned"];

fn validate_status(value: &str, loc: &[&str]) -> Option<Value> {
    if !ALLOWED_STATUSES.contains(&value) {
        Some(field_error(
            loc,
            "Invalid status provided. Allowed statuses : 'active', 'inactive' or 'churned'",
        ))
    } else {
        None
    }
}

// ---------------------------------------------------------------------
// Create profiles (HTTP: POST /v1/retention/profiles/create)
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct CreateProfilesChildRaw {
    customer_id: String,
    profile_data: Value,
    platform_info: Value,
    has_charged: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct CreateProfilesRaw {
    profiles: Vec<CreateProfilesChildRaw>,
}

#[derive(Debug, Clone)]
pub struct CreateProfilesChild {
    pub customer_id: String,
    pub profile_data: Value,
    pub platform_info: Value,
    pub has_charged: bool,
}

#[derive(Debug, Clone)]
pub struct CreateProfiles {
    pub profiles: Vec<CreateProfilesChild>,
}

impl CreateProfiles {
    pub fn from_json(body: &[u8], max_create_profiles: i64) -> Result<Self, OctyError> {
        let raw: CreateProfilesRaw =
            serde_json::from_slice(body).map_err(jsondecode_error)?;

        let mut errors = Vec::new();
        if raw.profiles.len() as i64 > max_create_profiles {
            errors.push(field_error(
                &["body", "profiles"],
                format!(
                    "You can only create up to {max_create_profiles} profiles per request. For larger uploads, please use the octy cli."
                ),
            ));
        }

        let mut profiles = Vec::with_capacity(raw.profiles.len());
        for (i, child) in raw.profiles.into_iter().enumerate() {
            let idx = i.to_string();
            if let Some(e) = validate_customer_id(&child.customer_id, &["body", "profiles", &idx, "customer_id"]) {
                errors.push(e);
            }
            errors.extend(disallow_null_values(
                &child.profile_data,
                "profile_data",
                &["body", "profiles", &idx],
            ));
            errors.extend(disallow_null_values(
                &child.platform_info,
                "platform_info",
                &["body", "profiles", &idx],
            ));
            profiles.push(CreateProfilesChild {
                customer_id: child.customer_id,
                profile_data: child.profile_data,
                platform_info: child.platform_info,
                has_charged: child.has_charged,
            });
        }

        if errors.is_empty() {
            Ok(Self { profiles })
        } else {
            Err(validation_error(errors))
        }
    }
}

// ---------------------------------------------------------------------
// Segment tags (shared shape; AMQP variant carries an optional `status`)
// ---------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SegmentTagInput {
    pub segment_id: String,
    pub segment_tag: String,
    pub status: Option<String>,
}

// ---------------------------------------------------------------------
// Canonical update-profile input consumed by services::profiles
// ---------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ProfileUpdateInput {
    pub profile_id: String,
    pub customer_id: Option<String>,
    pub profile_data: Option<Value>,
    pub platform_info: Option<Value>,
    pub has_charged: Option<bool>,
    pub status: Option<String>,
    pub rfm_score: Option<i64>,
    pub rfm_segment_desc: Option<String>,
    pub churn_probability: Option<String>,
    pub ltv_prediction: Option<i64>,
    pub current_ltv: Option<i64>,
    pub segment_tags: Option<Vec<SegmentTagInput>>,
}

// ---- HTTP variant: api/routers/request_models/profiles.py::UpdateProfiles ----
// All base fields are required; segment tags carry no `status`.

#[derive(Debug, Clone, Deserialize)]
struct HttpSegmentTagsRaw {
    segment_id: String,
    segment_tag: String,
}

#[derive(Debug, Clone, Deserialize)]
struct UpdateProfilesChildHttpRaw {
    profile_id: String,
    customer_id: String,
    profile_data: Value,
    platform_info: Value,
    has_charged: bool,
    status: String,
    #[serde(default)]
    rfm_score: Option<i64>,
    #[serde(default)]
    rfm_segment_desc: Option<String>,
    #[serde(default)]
    churn_probability: Option<String>,
    #[serde(default)]
    ltv_prediction: Option<i64>,
    #[serde(default)]
    current_ltv: Option<i64>,
    #[serde(default)]
    segment_tags: Option<Vec<HttpSegmentTagsRaw>>,
}

#[derive(Debug, Clone, Deserialize)]
struct UpdateProfilesHttpRaw {
    profiles: Vec<UpdateProfilesChildHttpRaw>,
}

pub struct UpdateProfilesHttp {
    pub profiles: Vec<ProfileUpdateInput>,
}

impl UpdateProfilesHttp {
    pub fn from_json(body: &[u8], max_update_delete_profiles: i64) -> Result<Self, OctyError> {
        let raw: UpdateProfilesHttpRaw = serde_json::from_slice(body).map_err(jsondecode_error)?;

        let mut errors = Vec::new();
        if raw.profiles.len() as i64 > max_update_delete_profiles {
            errors.push(field_error(
                &["body", "profiles"],
                format!("You can only update up to {max_update_delete_profiles} profiles per request."),
            ));
        }

        let mut profiles = Vec::with_capacity(raw.profiles.len());
        for (i, child) in raw.profiles.into_iter().enumerate() {
            let idx = i.to_string();
            if let Some(e) = validate_customer_id(&child.customer_id, &["body", "profiles", &idx, "customer_id"]) {
                errors.push(e);
            }
            errors.extend(disallow_null_values(
                &child.profile_data,
                "profile_data",
                &["body", "profiles", &idx],
            ));
            errors.extend(disallow_null_values(
                &child.platform_info,
                "platform_info",
                &["body", "profiles", &idx],
            ));
            if let Some(e) = validate_status(&child.status, &["body", "profiles", &idx, "status"]) {
                errors.push(e);
            }
            profiles.push(ProfileUpdateInput {
                profile_id: child.profile_id,
                customer_id: Some(child.customer_id),
                profile_data: Some(child.profile_data),
                platform_info: Some(child.platform_info),
                has_charged: Some(child.has_charged),
                status: Some(child.status),
                rfm_score: child.rfm_score,
                rfm_segment_desc: child.rfm_segment_desc,
                churn_probability: child.churn_probability,
                ltv_prediction: child.ltv_prediction,
                current_ltv: child.current_ltv,
                segment_tags: child.segment_tags.map(|tags| {
                    tags.into_iter()
                        .map(|t| SegmentTagInput {
                            segment_id: t.segment_id,
                            segment_tag: t.segment_tag,
                            status: None,
                        })
                        .collect()
                }),
            });
        }

        if errors.is_empty() {
            Ok(Self { profiles })
        } else {
            Err(validation_error(errors))
        }
    }
}

// ---- AMQP variant: data/models/profiles.py::UpdateProfiles ----
// Every field but `profile_id` is optional (partial update from internal
// processes: rfm/churn/segmentation workers).

#[derive(Debug, Clone, Deserialize)]
struct AmqpSegmentTagsRaw {
    segment_id: String,
    segment_tag: String,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct UpdateProfilesChildAmqpRaw {
    profile_id: String,
    #[serde(default)]
    customer_id: Option<String>,
    #[serde(default)]
    profile_data: Option<Value>,
    #[serde(default)]
    platform_info: Option<Value>,
    #[serde(default)]
    has_charged: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    rfm_score: Option<i64>,
    #[serde(default)]
    rfm_segment_desc: Option<String>,
    #[serde(default)]
    churn_probability: Option<String>,
    #[serde(default)]
    ltv_prediction: Option<i64>,
    #[serde(default)]
    current_ltv: Option<i64>,
    #[serde(default)]
    segment_tags: Option<Vec<AmqpSegmentTagsRaw>>,
}

#[derive(Debug, Clone, Deserialize)]
struct UpdateProfilesAmqpRaw {
    account_id: String,
    profiles: Vec<UpdateProfilesChildAmqpRaw>,
}

#[derive(Debug)]
pub struct UpdateProfilesAmqp {
    pub account_id: String,
    pub profiles: Vec<ProfileUpdateInput>,
}

impl UpdateProfilesAmqp {
    pub fn from_json(body: &[u8]) -> Result<Self, OctyError> {
        let raw: UpdateProfilesAmqpRaw = serde_json::from_slice(body).map_err(jsondecode_error)?;

        let mut errors = Vec::new();
        let mut profiles = Vec::with_capacity(raw.profiles.len());
        for (i, child) in raw.profiles.into_iter().enumerate() {
            let idx = i.to_string();
            // pydantic only runs the validator when the (optional) field is
            // explicitly provided.
            if let Some(status) = &child.status {
                if let Some(e) = validate_status(status, &["profiles", &idx, "status"]) {
                    errors.push(e);
                }
            }
            profiles.push(ProfileUpdateInput {
                profile_id: child.profile_id,
                customer_id: child.customer_id,
                profile_data: child.profile_data,
                platform_info: child.platform_info,
                has_charged: child.has_charged,
                status: child.status,
                rfm_score: child.rfm_score,
                rfm_segment_desc: child.rfm_segment_desc,
                churn_probability: child.churn_probability,
                ltv_prediction: child.ltv_prediction,
                current_ltv: child.current_ltv,
                segment_tags: child.segment_tags.map(|tags| {
                    tags.into_iter()
                        .map(|t| SegmentTagInput {
                            segment_id: t.segment_id,
                            segment_tag: t.segment_tag,
                            status: t.status,
                        })
                        .collect()
                }),
            });
        }

        if errors.is_empty() {
            Ok(Self {
                account_id: raw.account_id,
                profiles,
            })
        } else {
            Err(validation_error(errors))
        }
    }
}

// ---------------------------------------------------------------------
// Delete profiles
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct DeleteProfilesHttpRaw {
    profiles: Vec<String>,
}

pub struct DeleteProfilesHttp {
    pub profiles: Vec<String>,
}

impl DeleteProfilesHttp {
    pub fn from_json(body: &[u8], max_update_delete_profiles: i64) -> Result<Self, OctyError> {
        let raw: DeleteProfilesHttpRaw = serde_json::from_slice(body).map_err(jsondecode_error)?;
        if raw.profiles.len() as i64 > max_update_delete_profiles {
            return Err(validation_error(vec![field_error(
                &["body", "profiles"],
                format!("You can only delete up to {max_update_delete_profiles} profiles per request."),
            )]));
        }
        Ok(Self { profiles: raw.profiles })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteProfilesAmqp {
    pub account_id: String,
    pub profiles: Vec<String>,
}

// ---------------------------------------------------------------------
// Internal endpoints
// ---------------------------------------------------------------------

fn default_tag_statuses() -> Option<Vec<String>> {
    Some(vec!["active".to_string()])
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetProfilesInternal {
    pub account_id: String,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default = "default_tag_statuses")]
    pub tag_statuses: Option<Vec<String>>,
    pub get_all: bool,
}

impl GetProfilesInternal {
    pub fn from_json(body: &[u8]) -> Result<Self, OctyError> {
        serde_json::from_slice(body).map_err(jsondecode_error)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteAccountProfiles {
    pub account_id: String,
}

impl DeleteAccountProfiles {
    pub fn from_json(body: &[u8]) -> Result<Self, OctyError> {
        serde_json::from_slice(body).map_err(jsondecode_error)
    }
}

// ---------------------------------------------------------------------
// Segment tag AMQP messages (data/models/segment_tags.py)
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct SegmentId {
    pub segment_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SegmentIdUpdateDelete {
    pub account_id: String,
    pub action: String,
    pub segment_ids: Vec<SegmentId>,
}

impl SegmentIdUpdateDelete {
    pub fn from_json(body: &[u8]) -> Result<Self, OctyError> {
        serde_json::from_slice(body).map_err(jsondecode_error)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupedSegmentationDatabaseOperations {
    pub account_id: String,
    pub operations: Vec<Value>,
}

impl GroupedSegmentationDatabaseOperations {
    pub fn from_json(body: &[u8]) -> Result<Self, OctyError> {
        serde_json::from_slice(body).map_err(jsondecode_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_profiles_validates_customer_id_and_limit() {
        let body = serde_json::json!({
            "profiles": [
                {"customer_id": "a,b", "profile_data": {"age": 1}, "platform_info": {"os": "ios"}, "has_charged": false}
            ]
        });
        let err = CreateProfiles::from_json(&serde_json::to_vec(&body).unwrap(), 100).unwrap_err();
        assert_eq!(err.code, 422);
        assert_eq!(err.reasons.len(), 1);
    }

    #[test]
    fn create_profiles_rejects_null_values() {
        let body = serde_json::json!({
            "profiles": [
                {"customer_id": "cust1", "profile_data": {"age": null}, "platform_info": {"os": "ios"}, "has_charged": false}
            ]
        });
        let err = CreateProfiles::from_json(&serde_json::to_vec(&body).unwrap(), 100).unwrap_err();
        assert_eq!(err.code, 422);
        assert_eq!(err.reasons.len(), 1);
    }

    #[test]
    fn create_profiles_accepts_valid_body() {
        let body = serde_json::json!({
            "profiles": [
                {"customer_id": "cust1", "profile_data": {"age": 30}, "platform_info": {"os": "ios"}, "has_charged": false}
            ]
        });
        let parsed = CreateProfiles::from_json(&serde_json::to_vec(&body).unwrap(), 100).unwrap();
        assert_eq!(parsed.profiles.len(), 1);
    }

    #[test]
    fn amqp_update_skips_validator_when_status_absent() {
        let body = serde_json::json!({
            "account_id": "acc1",
            "profiles": [{"profile_id": "p1"}]
        });
        let parsed = UpdateProfilesAmqp::from_json(&serde_json::to_vec(&body).unwrap()).unwrap();
        assert_eq!(parsed.profiles.len(), 1);
        assert!(parsed.profiles[0].status.is_none());
    }

    #[test]
    fn amqp_update_rejects_bad_status_when_present() {
        let body = serde_json::json!({
            "account_id": "acc1",
            "profiles": [{"profile_id": "p1", "status": "bogus"}]
        });
        let err = UpdateProfilesAmqp::from_json(&serde_json::to_vec(&body).unwrap()).unwrap_err();
        assert_eq!(err.code, 422);
    }
}
