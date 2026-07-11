//! Port of `data/models/segments.py` (pydantic request models for the two
//! AMQP job payloads). Loosely-typed inner fields (`Dict`, `Any`) stay
//! `serde_json::Value` so unknown/extra keys survive round-tripping, which
//! matches pydantic's default behaviour for `Dict`/`Any` fields.

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct AccountData {
    pub account_id: String,
    pub webhook_url: String,
    #[serde(default)]
    pub account_type: Option<String>,
    #[serde(default)]
    pub account_currency: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SegmentData {
    pub segmentation_type: String,
    #[serde(default)]
    pub segment_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProfileRef {
    pub profile_id: String,
    #[serde(default)]
    pub customer_id: Option<String>,
    #[serde(default)]
    pub profile_data: Option<Value>,
    #[serde(default)]
    pub platform_info: Option<Value>,
    #[serde(default)]
    pub has_charged: Option<bool>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub rfm_score: Option<i64>,
    #[serde(default)]
    pub rfm_segment_desc: Option<String>,
    #[serde(default)]
    pub churn_probability: Option<String>,
    #[serde(default)]
    pub ltv_prediction: Option<i64>,
    #[serde(default)]
    pub current_ltv: Option<i64>,
    #[serde(default)]
    pub segment_tags: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventData {
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub event_properties: Option<Value>,
    #[serde(default)]
    pub created_at: Option<Value>,
    pub profile: ProfileRef,
    #[serde(default)]
    pub event_timeframe: Option<i64>,
}

impl EventData {
    /// Mirrors Python's `event_obj.dict()` — the JSON object handed to the
    /// engine as `self.event`.
    pub fn as_value(&self) -> Value {
        serde_json::json!({
            "event_id": self.event_id,
            "event_type": self.event_type,
            "event_properties": self.event_properties,
            "created_at": self.created_at,
            "profile": {
                "profile_id": self.profile.profile_id,
                "customer_id": self.profile.customer_id,
                "profile_data": self.profile.profile_data,
                "platform_info": self.profile.platform_info,
                "has_charged": self.profile.has_charged,
                "status": self.profile.status,
                "rfm_score": self.profile.rfm_score,
                "rfm_segment_desc": self.profile.rfm_segment_desc,
                "churn_probability": self.profile.churn_probability,
                "ltv_prediction": self.profile.ltv_prediction,
                "current_ltv": self.profile.current_ltv,
                "segment_tags": self.profile.segment_tags,
            },
            "event_timeframe": self.event_timeframe,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PastJobData {
    pub segment_data: SegmentData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LiveJobData {
    pub segment_data: SegmentData,
    pub event_data: EventData,
    // Deserialized for parity with the Python model but never read by the
    // engine (the Python `handle_message` never inspects it either).
    #[serde(default)]
    #[allow(dead_code)]
    pub validation_job: bool,
    #[serde(default)]
    pub live_octy_job_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PastSegmentationJob {
    pub account_data: AccountData,
    pub job_data: PastJobData,
    pub octy_job_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LiveSegmentationJob {
    pub account_data: AccountData,
    pub job_data: LiveJobData,
    pub octy_job_id: String,
}
