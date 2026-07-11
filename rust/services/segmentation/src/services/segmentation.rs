//! Port of `services/segmentation.py` — `SegmentValidatation` (sic) and
//! `SegmentationService`.
//!
//! NB on fidelity: the Python service called several async repository
//! methods without `await` (`get_segment_by_attr`, `get_event_types_by_name`,
//! `get_segments` in `delete_segments`, `delete_account_segments`), which
//! made those code paths crash with a 500 before doing any work. This port
//! implements the *intended* behavior (the awaited calls); every such
//! divergence is marked with a `PY-BUG` comment.

use octy_shared::errors::{ErrorReason, OctyError};
use octy_shared::utils::generate_uid;
use serde_json::{json, Value};

use octy_spin::auth::AuthAccount;
use octy_spin::ctx::Ctx;

use crate::models::{CreateSegment, UpdatePastSegementProfilesChild};
use crate::repos::segmentation as repo;

const EVENT_SEQUENCE_LIMIT: usize = 10;
const PAST_SEGMENT_SUB_TYPES: [i64; 4] = [1, 2, 3, 4];
const PAST_SEGMENT_INTERVALS: i64 = 2;
const LIVE_SEGMENT_SUB_TYPES: [i64; 2] = [1, 2];

fn seg_help(ctx: &Ctx) -> String {
    ctx.config
        .opt_str("SEGMENTATION_EXTENDED_HELP")
        .unwrap_or("")
        .to_string()
}

fn invalid(code: u16, description: &str, message: String, help: &str) -> OctyError {
    OctyError::new(code, description, vec![ErrorReason::new(message, help)])
}

/// Python `type(x)` display name for JSON values (`<class 'str'>` etc.).
fn py_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_f64() {
                "float"
            } else {
                "int"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

// ===========================================================================
// SegmentValidatation
// ===========================================================================

struct SegmentValidation<'a> {
    ctx: &'a Ctx,
    account_id: &'a str,
    segment: &'a CreateSegment,
    system_event_types: Vec<String>,
    help: String,
}

impl<'a> SegmentValidation<'a> {
    fn new(ctx: &'a Ctx, account_id: &'a str, segment: &'a CreateSegment) -> Result<Self, OctyError> {
        let mut system_event_types = Vec::new();
        for et in ctx.config.get_array("SYSTEM_EVENT_TYPES_MAP")? {
            system_event_types.push(
                et.get("event_type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            );
        }
        let help = seg_help(ctx);
        Ok(Self {
            ctx,
            account_id,
            segment,
            system_event_types,
            help,
        })
    }

    // Shared validations ---

    fn v_num_of_events(&self) -> Result<(), OctyError> {
        if self.segment.event_sequence.len() > EVENT_SEQUENCE_LIMIT {
            return Err(invalid(
                400,
                "Invalid event sequence provided.",
                format!(
                    "The event sequence provided exceeds the maximun limit of {EVENT_SEQUENCE_LIMIT} events"
                ),
                &self.help,
            ));
        }
        Ok(())
    }

    fn v_segment_type(&self) -> Result<(), OctyError> {
        if self.segment.segment_type != "live" && self.segment.segment_type != "past" {
            return Err(invalid(
                400,
                "Invalid segment subtype provided.",
                "segment_type must be type of 'live' or 'past'".to_string(),
                &self.help,
            ));
        }
        Ok(())
    }

    /// PY-BUG: the Python never awaited `get_segment_by_attr` (and passed a
    /// pydantic model where a dict was expected), so this check 500'd
    /// whenever it ran. Ported as intended.
    async fn v_segment_duplicates(&self) -> Result<(), OctyError> {
        let existing = repo::get_segment_by_attr(self.ctx, self.account_id, self.segment).await?;
        if let Some(existing) = existing {
            if existing.get("segment_name").and_then(Value::as_str)
                == Some(self.segment.segment_name.as_str())
            {
                return Err(invalid(
                    400,
                    "Duplicate segment name provided.",
                    format!(
                        "Segment with provided name: {} already exists. If you have recently deleted a segment with this name, you must wait up to 72 hours before you are able to create another segment with this name.",
                        self.segment.segment_name
                    ),
                    &self.help,
                ));
            }
            return Err(invalid(
                400,
                "Duplicate segment type, sub type and event sequence provided.",
                format!(
                    "Segment with provided type: {}, sub type: {} with an identical event sequence, segment_timeframe and profile properties already exists.",
                    self.segment.segment_type, self.segment.segment_sub_type
                ),
                &self.help,
            ));
        }
        Ok(())
    }

    async fn v_event_sequence_event_types(&self) -> Result<(), OctyError> {
        let mut provided_custom: Vec<String> = Vec::new();
        for event in &self.segment.event_sequence {
            if !self.system_event_types.contains(&event.event_type) {
                provided_custom.push(event.event_type.clone());
            }
        }

        // PY-BUG: not awaited in the Python (unpacking a coroutine → 500 on
        // every create). Ported as intended.
        let (found_event_types, _not_found) =
            repo::get_event_types_by_name(self.ctx, self.account_id, &provided_custom).await?;

        let system_map = self.ctx.config.get_array("SYSTEM_EVENT_TYPES_MAP")?;

        for event in &self.segment.event_sequence {
            if self.system_event_types.contains(&event.event_type) {
                let system_event = system_map
                    .iter()
                    .find(|key| {
                        key.get("event_type").and_then(Value::as_str)
                            == Some(event.event_type.as_str())
                    })
                    .cloned()
                    .unwrap_or(Value::Null);
                if let Some(props) = &event.event_properties {
                    let system_props = system_event
                        .get("event_properties")
                        .and_then(Value::as_object)
                        .ok_or_else(|| {
                            OctyError::internal(format!(
                                "SYSTEM_EVENT_TYPES_MAP entry for '{}' has no event_properties dict",
                                event.event_type
                            ))
                        })?;
                    for (k, v) in props {
                        match system_props.get(k) {
                            None => {
                                return Err(invalid(
                                    400,
                                    &format!(
                                        "Invalid event provided within the event sequence of this request. The system event type '{}' does not have a key named '{}' in it's event_properties attribute.",
                                        event.event_type, k
                                    ),
                                    "Invalid event provided.".to_string(),
                                    &self.help,
                                ))
                            }
                            Some(expected) => {
                                if py_type_name(v) != py_type_name(expected) {
                                    return Err(invalid(
                                        400,
                                        &format!(
                                            "Invalid event provided within the event sequence of this request. The system event type '{}' event_properties key '{}' value must be of type : <class '{}'>",
                                            event.event_type,
                                            k,
                                            py_type_name(expected)
                                        ),
                                        "Invalid event provided.".to_string(),
                                        &self.help,
                                    ));
                                }
                            }
                        }
                    }
                }
                continue;
            }

            let custom_event = found_event_types.iter().find(|key| {
                key.get("event_type").and_then(Value::as_str) == Some(event.event_type.as_str())
            });
            match custom_event {
                Some(custom) => {
                    if let Some(props) = &event.event_properties {
                        let custom_props = custom
                            .get("event_properties")
                            .and_then(Value::as_object)
                            .cloned()
                            .unwrap_or_default();
                        for k in props.keys() {
                            if !custom_props.contains_key(k) {
                                return Err(invalid(
                                    400,
                                    &format!(
                                        "Invalid event provided within the event sequence of this request. The custom event type '{}' does not have a key named '{}' in it's event_properties attribute.",
                                        event.event_type, k
                                    ),
                                    "Invalid event provided.".to_string(),
                                    &self.help,
                                ));
                            }
                        }
                    }
                }
                None => {
                    return Err(invalid(
                        400,
                        &format!(
                            "Invalid event provided within the event sequence of this request. Event '{}' does not exist, with provided event_properties.",
                            event.event_type
                        ),
                        "Invalid event provided.".to_string(),
                        &self.help,
                    ))
                }
            }
        }
        Ok(())
    }

    fn duplicate_events_error(&self) -> OctyError {
        invalid(
            400,
            "Invalid event sequence provided.",
            "Duplicate events with matching event properties found in event sequence.".to_string(),
            &self.help,
        )
    }

    /// Literal port of `_v_event_sequence_duplicates`, including its quirks:
    /// property values are pooled across *all* duplicated event types, and
    /// the final uniqueness check only inspects the last property key seen.
    fn v_event_sequence_duplicates(&self) -> Result<(), OctyError> {
        let mut duplicates: Vec<&str> = Vec::new();
        let mut events_list: Vec<&str> = Vec::new();
        for event in &self.segment.event_sequence {
            let et = event.event_type.as_str();
            if events_list.contains(&et) {
                if !duplicates.contains(&et) {
                    duplicates.push(et);
                }
            } else {
                events_list.push(et);
            }
        }

        if duplicates.is_empty() {
            return Ok(());
        }

        let mut event_prop_map: std::collections::HashMap<String, Vec<&Value>> =
            std::collections::HashMap::new();
        let mut event_prop_list: Vec<&str> = Vec::new();
        let mut event_prop_none_list: Vec<&str> = Vec::new();
        let mut key_ = String::new();

        for event in &self.segment.event_sequence {
            if !duplicates.contains(&event.event_type.as_str()) {
                continue;
            }
            match &event.event_properties {
                None => {
                    if !event_prop_none_list.contains(&event.event_type.as_str()) {
                        event_prop_none_list.push(event.event_type.as_str());
                    } else {
                        return Err(self.duplicate_events_error());
                    }
                }
                Some(props) => {
                    for (prop, value) in props {
                        key_ = prop.clone();
                        if !event_prop_list.contains(&prop.as_str()) {
                            event_prop_map.insert(prop.clone(), vec![value]);
                            event_prop_list.push(prop.as_str());
                        } else if let Some(values) = event_prop_map.get_mut(prop) {
                            values.push(value);
                        }
                    }
                }
            }
        }

        // Python: `event_prop_map_dict[key_]` — a KeyError here surfaced as 500.
        let values = event_prop_map
            .get(&key_)
            .ok_or_else(|| OctyError::internal(format!("KeyError: '{key_}'")))?;
        let mut unique: Vec<&&Value> = Vec::new();
        for v in values {
            if !unique.contains(&v) {
                unique.push(v);
            }
        }
        if unique.len() != values.len() {
            return Err(self.duplicate_events_error());
        }
        Ok(())
    }

    // Past segment validations ---

    fn v_past_subtype(&self) -> Result<(), OctyError> {
        if !PAST_SEGMENT_SUB_TYPES.contains(&self.segment.segment_sub_type) {
            return Err(invalid(
                400,
                "Invalid segment provided.",
                "Past segments must have a sub type of either 1,2,3 or 4".to_string(),
                &self.help,
            ));
        }
        Ok(())
    }

    fn v_past_timeframe(&self) -> Result<(), OctyError> {
        if self.segment.segment_timeframe < PAST_SEGMENT_INTERVALS
            || self.segment.segment_timeframe > 365
        {
            return Err(invalid(
                400,
                "Invalid segment provided.",
                format!(
                    "Past segments must have a timeframe of more than {PAST_SEGMENT_INTERVALS} days and less than 365."
                ),
                &self.help,
            ));
        }
        Ok(())
    }

    fn v_past_profile_properties(&self) -> Result<(), OctyError> {
        let name = &self.segment.profile_property_name;
        let value_provided = self
            .segment
            .profile_property_value
            .as_ref()
            .map(|v| !v.is_null())
            .unwrap_or(false);

        if name.is_some() && !value_provided {
            return Err(invalid(
                400,
                "Invalid segment provided.",
                "profile_property_value must be provided with profile_property_name".to_string(),
                &self.help,
            ));
        }
        if name.is_none() && value_provided {
            return Err(invalid(
                400,
                "Invalid segment provided.",
                "profile_property_name must be provided with profile_property_value".to_string(),
                &self.help,
            ));
        }

        if self.segment.segment_sub_type == 3 || self.segment.segment_sub_type == 4 {
            if name.is_none() || !value_provided {
                return Err(invalid(
                    400,
                    "Invalid segment provided.",
                    "Past segments with a sub type 3 or 4 must have both profile_property_name and profile_property_value parameters provided".to_string(),
                    &self.help,
                ));
            }
        }
        if self.segment.segment_sub_type == 1 || self.segment.segment_sub_type == 2 {
            if name.is_some() || value_provided {
                return Err(invalid(
                    400,
                    "Invalid segment provided.",
                    "Past segments with a sub type 1 or 2 must not have either profile_property_name or profile_property_value parameters provided".to_string(),
                    &self.help,
                ));
            }
        }
        Ok(())
    }

    fn v_past_event_sequence_event_timeframes(&self) -> Result<(), OctyError> {
        for event in &self.segment.event_sequence {
            if event.exp_timeframe != 0 {
                return Err(invalid(
                    400,
                    "Invalid event provided.",
                    "The 'exp_timeframe' parameter within each 'event_sequence'>>'event' object MUST be set to 0 if the 'segment_type' parameter is set to 'past'".to_string(),
                    &self.help,
                ));
            }
        }
        Ok(())
    }

    fn first_event(&self) -> Result<&crate::models::EventSequenceEvent, OctyError> {
        // Python indexed `event_sequence[0]` — an empty sequence raised
        // IndexError → 500.
        self.segment
            .event_sequence
            .first()
            .ok_or_else(|| OctyError::internal("list index out of range"))
    }

    async fn v_past_event_sequence(&self) -> Result<(), OctyError> {
        if self.first_event()?.action_inaction != "action" {
            return Err(invalid(
                400,
                "Invalid event sequence provided.",
                "The first event 'action_inaction' parameter in a segments event sequence must be of type 'action'".to_string(),
                &self.help,
            ));
        }

        let last_idx = self.segment.event_sequence.len() - 1;
        if self.segment.segment_sub_type == 2 || self.segment.segment_sub_type == 4 {
            if self.segment.event_sequence[last_idx].action_inaction != "inaction" {
                return Err(invalid(
                    400,
                    "Invalid event sequence provided.",
                    "The last event 'action_inaction' parameter in segments with sub type 2 or 4 must be of type 'inaction'".to_string(),
                    &self.help,
                ));
            }
            let inactions = self
                .segment
                .event_sequence
                .iter()
                .filter(|e| e.action_inaction == "inaction")
                .count();
            if inactions > 1 {
                return Err(invalid(
                    400,
                    "Invalid event sequence provided.",
                    "Segments can contain no more than one single 'inaction' event in their event sequence.".to_string(),
                    &self.help,
                ));
            }
        } else if self
            .segment
            .event_sequence
            .iter()
            .any(|e| e.action_inaction == "inaction")
        {
            return Err(invalid(
                400,
                "Invalid event sequence provided.",
                "Sub type 1 & 3 segments can not contain inaction events in their event sequence.".to_string(),
                &self.help,
            ));
        }

        self.v_event_sequence_event_types().await?;
        self.v_event_sequence_duplicates()?;
        self.v_past_event_sequence_event_timeframes()
    }

    // Live segment validations ---

    fn v_live_subtype(&self) -> Result<(), OctyError> {
        if !LIVE_SEGMENT_SUB_TYPES.contains(&self.segment.segment_sub_type) {
            return Err(invalid(
                400,
                "Invalid segment provided.",
                "Live segments must have a sub type of either 1 or 2".to_string(),
                &self.help,
            ));
        }
        Ok(())
    }

    fn v_live_timeframe(&self) -> Result<(), OctyError> {
        if self.segment.segment_timeframe != 0 {
            return Err(invalid(
                400,
                "Invalid segment provided.",
                "When creating a 'live-segment' definition, the 'segment_timeframe' parameter must have a value of 0".to_string(),
                &self.help,
            ));
        }
        Ok(())
    }

    fn v_live_profile_properties(&self) -> Result<(), OctyError> {
        let value_provided = self
            .segment
            .profile_property_value
            .as_ref()
            .map(|v| !v.is_null())
            .unwrap_or(false);
        if self.segment.profile_property_name.is_some() || value_provided {
            return Err(invalid(
                400,
                "Invalid segment provided.",
                "profile_property_name or profile_property_value protperties must not be provided when creating a live segment".to_string(),
                &self.help,
            ));
        }
        Ok(())
    }

    fn v_live_event_sequence_event_timeframes(&self) -> Result<(), OctyError> {
        let num = self.segment.event_sequence.len();
        let last_idx = num - 1;
        for (idx, event) in self.segment.event_sequence.iter().enumerate() {
            if idx != num - 1 && event.exp_timeframe < 2 {
                return Err(invalid(
                    400,
                    "Invalid event provided.",
                    "The 'exp_timeframe' parameter within the first 'event_sequence'>>'event' object MUST be set to '2' or more (minutes).".to_string(),
                    &self.help,
                ));
            }
            if self.segment.event_sequence[last_idx].exp_timeframe > 0 {
                return Err(invalid(
                    400,
                    "Invalid event provided.",
                    "The 'exp_timeframe' parameter within the last 'event_sequence'>>'event' object MUST be set to 0.".to_string(),
                    &self.help,
                ));
            }
        }
        Ok(())
    }

    async fn v_live_event_sequence(&self) -> Result<(), OctyError> {
        if self.first_event()?.action_inaction != "action" {
            return Err(invalid(
                400,
                "Invalid event sequence provided.",
                "The first event 'action_inaction' parameter in a segments event sequence must be of type 'action'".to_string(),
                &self.help,
            ));
        }

        let actions = self
            .segment
            .event_sequence
            .iter()
            .filter(|e| e.action_inaction == "action")
            .count();
        if actions > 1 {
            return Err(invalid(
                400,
                "Invalid event sequence provided.",
                "Live segments can contain no more than one single 'action' event in their event sequence.".to_string(),
                &self.help,
            ));
        }

        let last_idx = self.segment.event_sequence.len() - 1;
        if self.segment.segment_sub_type == 2 {
            if self.segment.event_sequence[last_idx].action_inaction != "inaction" {
                return Err(invalid(
                    400,
                    "Invalid event sequence provided.",
                    "The last event 'action_inaction' parameter in segments with sub type 2 , must be of type 'inaction'".to_string(),
                    &self.help,
                ));
            }
        } else if self
            .segment
            .event_sequence
            .iter()
            .any(|e| e.action_inaction == "inaction")
        {
            return Err(invalid(
                400,
                "Invalid event sequence provided.",
                "Sub type 1 segments can not contain inaction events in their event sequence.".to_string(),
                &self.help,
            ));
        }

        self.v_event_sequence_event_types().await?;
        self.v_event_sequence_duplicates()?;
        self.v_live_event_sequence_event_timeframes()
    }

    // Core method ---

    async fn validate(&self) -> Result<(), OctyError> {
        self.v_segment_type()?;
        self.v_num_of_events()?;

        if self.segment.segment_type == "past" {
            self.v_past_subtype()?;
            self.v_past_timeframe()?;
            self.v_past_profile_properties()?;
            self.v_past_event_sequence().await?;
        } else if self.segment.segment_type == "live" {
            self.v_live_subtype()?;
            self.v_live_timeframe()?;
            self.v_live_profile_properties()?;
            self.v_live_event_sequence().await?;
        }

        self.v_segment_duplicates().await
    }
}

// ===========================================================================
// utils.assess_resource_limit — `li` (from the auth JWT) is an
// asterisk-delimited string: profiles*items*event_types*events*segments*mes_templates
// e.g. "50000*150*100*100000*25*50"; segment limit is index 4.
// ===========================================================================

fn assess_resource_limit(
    limits: &str,
    current_count: i64,
    requested: i64,
) -> Result<(bool, i64, i64), OctyError> {
    let resource_limit: i64 = limits
        .split('*')
        .nth(4)
        .ok_or_else(|| OctyError::internal("list index out of range"))?
        .parse()
        .map_err(|e| OctyError::internal(format!("invalid resource limit: {e}")))?;
    let remainder = resource_limit - current_count;
    if requested + current_count > resource_limit {
        Ok((false, resource_limit, remainder))
    } else {
        Ok((true, resource_limit, remainder - requested))
    }
}

// ===========================================================================
// SegmentationService
// ===========================================================================

pub async fn get_segments(
    ctx: &Ctx,
    account_id: &str,
    identifiers: Option<&[String]>,
    cursor: Option<i64>,
    status: &str,
    segment_type: &str,
    internal: bool,
) -> Result<(Vec<Value>, i64), OctyError> {
    if let (Some(identifiers), Some(0)) = (identifiers, cursor) {
        let (segments, total) =
            repo::get_segment_by_identifiers(ctx, identifiers, account_id).await?;
        if total < 1 {
            return Err(invalid(
                400,
                "Invalid segment identifier(s) provided",
                "No segments were found with the provided identifier(s)".to_string(),
                &seg_help(ctx),
            ));
        }
        return Ok((segments, total));
    }

    if let (None, Some(cursor)) = (identifiers, cursor) {
        let (segments, total) =
            repo::get_segments(ctx, account_id, segment_type, status, cursor, internal).await?;
        if segments.is_empty() {
            return Err(invalid(
                400,
                "No segments found",
                "No segments found with the provided segment identifier or pagination cursor exhausted".to_string(),
                &seg_help(ctx),
            ));
        }
        return Ok((segments, total));
    }

    // The Python fell off the end of the if/elif and returned None,
    // which crashed the caller (500). Unreachable through the routers.
    Err(OctyError::internal(
        "cannot unpack non-iterable NoneType object",
    ))
}

async fn create_segment_ref(
    ctx: &Ctx,
    account_id: &str,
    segment: &CreateSegment,
) -> Result<Value, OctyError> {
    let event_sequence: Vec<Value> = segment
        .event_sequence
        .iter()
        .map(|es| es.to_dict())
        .collect();
    let new_segment = json!({
        "segment_id": generate_uid("segment"),
        "account_id": account_id,
        "segment_name": segment.segment_name,
        "segment_type": segment.segment_type,
        "segment_sub_type": segment.segment_sub_type,
        "segment_timeframe": segment.segment_timeframe,
        "event_sequence": event_sequence,
        "profile_property_name": segment.profile_property_name,
        "profile_property_value": segment.profile_property_value_or_null(),
        "segment_status": if segment.segment_type == "live" { "created" } else { "processing" },
    });
    repo::create_segment(ctx, &new_segment).await?;
    Ok(new_segment)
}

/// Returns `(created_segment, message)`.
pub async fn create_segment(
    ctx: &Ctx,
    account: &AuthAccount,
    segment: &CreateSegment,
) -> Result<(Value, String), OctyError> {
    let account_id = account.account_oid().unwrap_or_default().to_string();

    // Assess allowed limits (`account_configurations['li']`; a missing key
    // was a KeyError → 500 in the Python).
    let limits = account.account_configurations["li"]
        .as_str()
        .ok_or_else(|| OctyError::internal("KeyError: 'li'"))?;
    let current_count = repo::get_segment_count(ctx, &account_id).await?;
    let (ok, limit, remainder) = assess_resource_limit(limits, current_count, 1)?;
    if !ok {
        return Err(invalid(
            400,
            "Resource limit exceeded",
            format!(
                "This request could not be completed as this request exceeds the allowed number of segments : {limit}. This account can create another {remainder} segment(s)."
            ),
            ctx.config.opt_str("RATE_LIMIT_EXTENDED_HELP").unwrap_or(""),
        ));
    }

    SegmentValidation::new(ctx, &account_id, segment)?
        .validate()
        .await?;

    // LIVE SEGMENTATION
    if segment.segment_type == "live" {
        let seg = create_segment_ref(ctx, &account_id, segment).await?;
        return Ok((seg, "Segment created".to_string()));
    }

    // PAST SEGMENTATION
    let seg = create_segment_ref(ctx, &account_id, segment).await?;
    ctx.gateway
        .amqp_publish(
            "octy.job.cmd.create",
            &json!({
                "account_id": account_id,
                // (sic) 'alt_dentifier' — typo kept from the Python payload.
                "alt_dentifier": seg["segment_id"],
                "job_meta": {
                    "job_type": "seg",
                    "amqp_routing_key": "past.segmentation.cmd.run",
                    "required_permissions": ["seg"],
                    "required_configurations": {
                        "account_attributes": [
                            "account_configurations.webhook_url",
                            "account_configurations.account_type",
                            "account_configurations.account_currency",
                        ],
                        "algorithm_configuration_idxs": []
                    },
                    "desired_runs": 0,
                    "time_interval": ctx.config.get("PAST_SEGMENTATION_JOB_INTERVAL")?,
                    "fail_threshold": 0
                },
                "job_data": {
                    "segment_data": {
                        "segmentation_type": "past",
                        "segment_id": seg["segment_id"]
                    }
                }
            }),
        )
        .await?;
    // Response reflects engine initialisation, not completion.
    Ok((seg, "Segmentation process initiated".to_string()))
}

/// AMQP `segment.profiles.cmd.update` — merge child profiles into parents.
pub async fn update_past_segment_profiles(
    ctx: &Ctx,
    account_id: &str,
    profiles: &[UpdatePastSegementProfilesChild],
) -> Result<(), OctyError> {
    let child_to_parent = |profile_id: &str| -> Option<&str> {
        profiles
            .iter()
            .find(|p| p.child_profiles.iter().any(|c| c == profile_id))
            .map(|p| p.parent_profile.as_str())
    };

    let all_child_profile_ids: Vec<String> = profiles
        .iter()
        .flat_map(|p| p.child_profiles.iter().cloned())
        .collect();

    let segments =
        repo::get_past_segments_by_profile_ids(ctx, account_id, &all_child_profile_ids).await?;

    for segment in segments {
        let current: Vec<Value> = segment
            .get("profile_ids")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mapped: Vec<Value> = current
            .into_iter()
            .map(|profile| {
                profile
                    .as_str()
                    .and_then(child_to_parent)
                    .map(|parent| json!(parent))
                    .unwrap_or(profile)
            })
            .collect();

        // list(dict.fromkeys(...)) — dedupe, first occurrence wins.
        let mut updated: Vec<Value> = Vec::new();
        for id in mapped {
            if !updated.contains(&id) {
                updated.push(id);
            }
        }

        repo::update_past_segment_profile_ids(
            ctx,
            account_id,
            segment.get("_id").unwrap_or(&Value::Null),
            &updated,
        )
        .await?;
    }
    Ok(())
}

/// Returns `(deleted_segments, failed_to_delete)`.
pub async fn delete_segments(
    ctx: &Ctx,
    account_id: &str,
    segment_ids: &[String],
) -> Result<(Vec<Value>, Vec<Value>), OctyError> {
    let mut deleted_segments: Vec<Value> = Vec::new();
    let mut failed_to_delete: Vec<Value> = Vec::new();

    // Deduplicate segment_ids (order preserved).
    let mut de_duped: Vec<&String> = Vec::new();
    for id in segment_ids {
        if !de_duped.contains(&id) {
            de_duped.push(id);
        }
    }

    // PY-BUG: not awaited in the Python; with the intended await the
    // subsequent `len(segments) < 1` check was over the (list, total) tuple
    // and could never fire, so it is not ported.
    let (segments, _total) = repo::get_segments(ctx, account_id, "all", "active", 0, false).await?;

    for segment_id in de_duped {
        let exists = segments
            .iter()
            .any(|s| s.get("segment_id").and_then(Value::as_str) == Some(segment_id.as_str()));
        if !exists {
            failed_to_delete.push(json!({
                "segment_id": segment_id,
                "error_message": "No active segment definitions found with this segment_id"
            }));
        } else {
            deleted_segments.push(json!({ "segment_id": segment_id }));
        }
    }

    if deleted_segments.is_empty() {
        return Err(invalid(
            400,
            "Invalid segment id provided",
            "No segments found with provided segment_ids".to_string(),
            &seg_help(ctx),
        ));
    }

    // Delete the segment definitions.
    repo::delete_segments(ctx, account_id, &deleted_segments).await?;

    // Delete all segment tags associated with the deleted segments.
    ctx.gateway
        .amqp_publish(
            "segment.tags.cmd.update.delete",
            &json!({
                "account_id": account_id,
                "action": "delete",
                "segment_ids": deleted_segments
            }),
        )
        .await?;

    // Remove the past-segmentation jobs from the octy-job service task list.
    let ids: Vec<Value> = deleted_segments
        .iter()
        .map(|seg| seg["segment_id"].clone())
        .collect();
    ctx.gateway
        .amqp_publish(
            "octy.job.cmd.delete",
            &json!({
                "account_id": account_id,
                "octy_job_ids": null,
                "alt_identifiers": ids
            }),
        )
        .await?;

    Ok((deleted_segments, failed_to_delete))
}

/// Delete all segments associated with an account (internal fan-out from the
/// account service).
///
/// PY-BUG: the Python returned the un-awaited coroutine, which crashed the
/// JSON response (500) and never deleted anything. Ported as intended.
pub async fn delete_account_segmentations_internal(
    ctx: &Ctx,
    account_id: &str,
) -> Result<bool, OctyError> {
    repo::delete_account_segments(ctx, account_id).await
}
