//! Port of `services/events.py` (`EventsService`).

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use octy_shared::ejson::legacy_date;
use octy_shared::utils::generate_uid;
use serde_json::{json, Map, Value};

use octy_spin::auth::AuthAccount;
use octy_spin::ctx::Ctx;

use crate::http_util::{ApiError, GMT_FMT};
use crate::models::{BatchCreateEvents, CreateEvent};
use crate::repos::{event_types as event_types_repo, events as events_repo};
use crate::services::billing::BillingUnits;
use crate::services::{account_id_str, account_limits, assess_resource_limit};

/// `_verify_event` outcome: `Ok(event_type_id)` (a JSON value — `null` for
/// unmatched system event types, quirk kept) or `Err((err_message,
/// err_description))` = `err_msg[0] / err_msg[1]` from the Python.
type VerifyResult = Result<Value, (String, String)>;

fn resource_limit_error(ctx: &Ctx, limit: i64, remainder: i64, resource: &str) -> ApiError {
    ApiError::reason(
        400,
        "Resource limit exceeded",
        format!(
            "This request could not be completed as the number of {resource} sent with this request exceeds the allowed limit of : {limit}. This account can create another {remainder} {resource}."
        ),
        ctx.config.opt_str("RATE_LIMIT_EXTENDED_HELP").unwrap_or(""),
    )
}

fn events_help(ctx: &Ctx) -> String {
    ctx.config.opt_str("EVENTS_EXTENDED_HELP").unwrap_or("").to_string()
}

/// `create_event` — single event creation (POST /v1/retention/events/create).
pub async fn create_event(
    ctx: &Ctx,
    account: &AuthAccount,
    event: &CreateEvent,
) -> Result<Value, ApiError> {
    let account_id = account_id_str(account)?;
    // Constructed in `EventsService.__init__` — validates the a_t/a_c claims.
    // (`process_name='items_data'` is what the Python passes — quirk kept.)
    let mut billing = BillingUnits::new(account, &account_id, "items_data")?;

    let (latest_events, event_count) =
        events_repo::get_events_meta(ctx, &account_id, &[event.event_type.clone()]).await?;

    let limits = account_limits(account)?;
    let (count_res, counts) = assess_resource_limit(&limits, event_count, 1, 3)?;
    if !count_res {
        return Err(resource_limit_error(ctx, counts.limit, counts.remainder, "events"));
    }

    let latest_event = latest_events.first();

    // verify provided profile id exists
    let (valid_profiles, invalid_profiles) =
        events_repo::get_profile_ids(ctx, &account_id, &[event.profile_id.clone()]).await?;
    if !invalid_profiles.is_empty() || valid_profiles.is_empty() {
        return Err(ApiError::reason(
            400,
            "Invalid event data provided",
            "Unknown profile_id supplied with this event instance",
            events_help(ctx),
        ));
    }

    // verify event
    let event_type_id = match verify_event(
        ctx,
        account,
        &account_id,
        &event.event_type,
        &event.event_properties,
        latest_event,
        &event.profile_id,
        &invalid_profiles,
    )
    .await?
    {
        Ok(id) => id,
        Err((err_message, err_description)) => {
            // The 'server error' branch of the Python is unreachable (no
            // verify error message contains it) — always a 400 here.
            return Err(ApiError::reason(400, err_description, err_message, events_help(ctx)));
        }
    };

    let event_id = generate_uid("event");
    let created_event = json!({
        "event_id": event_id,
        "profile_id": event.profile_id,
        "event_type_id": event_type_id,
        "event_type": event.event_type,
        "event_properties": event.event_properties,
    });

    events_repo::create_event(ctx, &account_id, &created_event).await?;

    billing.track_data_units(&created_event);
    billing.complete_data_units(ctx, "MB").await?;

    // Only create an octy job for this event if the event_type is part of an
    // active live segment event sequence.
    let segments = events_repo::get_live_segment_definitions(ctx, &account_id).await?;
    let mut segment_event_types: Vec<String> = Vec::new();
    for segment in &segments {
        let sequence = segment
            .get("event_sequence")
            .and_then(Value::as_array)
            .ok_or_else(|| ApiError::internal("KeyError: segment['event_sequence']"))?;
        for ev in sequence {
            let et = ev
                .get("event_type")
                .and_then(Value::as_str)
                .ok_or_else(|| ApiError::internal("KeyError: event_sequence event['event_type']"))?;
            if !segment_event_types.iter().any(|s| s == et) {
                segment_event_types.push(et.to_string());
            }
        }
    }

    if segment_event_types.iter().any(|s| s == &event.event_type) {
        ctx.gateway
            .amqp_publish(
                "octy.job.cmd.create",
                &json!({
                    "account_id": account_id,
                    "job_meta": {
                        "job_type": "seg",
                        "amqp_routing_key": "live.segmentation.cmd.run",
                        "required_permissions": ["seg"],
                        "required_configurations": {
                            "account_attributes": [
                                "account_configurations.webhook_url"
                            ],
                            "algorithm_configuration_idxs": []
                        },
                        "desired_runs": 1,
                        "time_interval": 0,
                        "fail_threshold": 10
                    },
                    "job_data": {
                        "segment_data": {
                            "segmentation_type": "live"
                        },
                        "validation_job": false,
                        "event_data": {
                            "event_id": event_id,
                            "event_type_id": created_event["event_type_id"],
                            "event_type": event.event_type,
                            "event_properties": event.event_properties,
                            "profile": valid_profiles[0]
                        }
                    }
                }),
            )
            .await
            .map_err(ApiError::from)?;
    }

    Ok(created_event)
}

/// `batch_create_events` → `(ret_valid_events, invalid_events)`.
pub async fn batch_create_events(
    ctx: &Ctx,
    account: &AuthAccount,
    events: &BatchCreateEvents,
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let account_id = account_id_str(account)?;
    let mut billing = BillingUnits::new(account, &account_id, "items_data")?;

    let mut event_types: Vec<String> = Vec::new();
    let mut profile_ids: Vec<String> = Vec::new();
    for event in &events.events {
        if !event_types.contains(&event.event_type) {
            event_types.push(event.event_type.clone());
        }
        profile_ids.push(event.profile_id.clone());
    }

    let (latest_events, event_count) =
        events_repo::get_events_meta(ctx, &account_id, &event_types).await?;

    let limits = account_limits(account)?;
    let (res, counts) =
        assess_resource_limit(&limits, event_count, events.events.len() as i64, 3)?;
    if !res {
        return Err(resource_limit_error(ctx, counts.limit, counts.remainder, "events"));
    }

    // verify provided profile ids exist
    let (valid_profiles, invalid_profiles) =
        events_repo::get_profile_ids(ctx, &account_id, &profile_ids).await?;
    if valid_profiles.is_empty() {
        return Err(ApiError::reason(
            400,
            "Invalid event data provided",
            "No valid profile_id(s) were supplied with event instance(s)",
            events_help(ctx),
        ));
    }

    let mut valid_events: Vec<Value> = Vec::new(); // created_at as {"$date": …} for Mongo
    let mut ret_valid_events: Vec<Value> = Vec::new(); // created_at as GMT string for the DTO
    let mut invalid_events: Vec<Value> = Vec::new();

    for event in &events.events {
        // if created_at is provided, ensure the format is correct
        let created_at: DateTime<Utc> = match &event.created_at {
            None => Utc::now(),
            Some(s) if s.is_empty() => Utc::now(),
            Some(s) => match NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
                Ok(naive) => Utc.from_utc_datetime(&naive),
                Err(_) => {
                    invalid_events.push(json!({
                        "event_type": event.event_type,
                        "event_properties": event.event_properties,
                        "profile_id": event.profile_id,
                        "error_message": "Incorrect date format supplied, should be YYYY-MM-DD HH:MM:SS",
                    }));
                    continue;
                }
            },
        };

        let latest_event = latest_events
            .iter()
            .find(|key| key.get("event_type").and_then(Value::as_str) == Some(&event.event_type));

        // verify event
        let event_type_id = match verify_event(
            ctx,
            account,
            &account_id,
            &event.event_type,
            &event.event_properties,
            latest_event,
            &event.profile_id,
            &invalid_profiles,
        )
        .await?
        {
            Ok(id) => id,
            Err((err_message, _)) => {
                invalid_events.push(json!({
                    "event_type": event.event_type,
                    "event_properties": event.event_properties,
                    "profile_id": event.profile_id,
                    "error_message": err_message,
                }));
                continue;
            }
        };

        let event_id = generate_uid("event");
        valid_events.push(json!({
            "event_id": event_id,
            "profile_id": event.profile_id,
            "event_type_id": event_type_id,
            "event_type": event.event_type,
            "event_properties": event.event_properties,
            "created_at": legacy_date(created_at),
        }));
        ret_valid_events.push(json!({
            "event_id": event_id,
            "profile_id": event.profile_id,
            "event_type_id": event_type_id,
            "event_type": event.event_type,
            "event_properties": event.event_properties,
            "created_at": created_at.format(GMT_FMT).to_string(),
        }));
    }

    if valid_events.is_empty() {
        return Err(ApiError::octy(
            400,
            "Invalid events data provided. No events were created.",
            invalid_events,
        ));
    }

    // billed before the insert, exactly like the Python
    billing.track_data_units(&json!(ret_valid_events));
    billing.complete_data_units(ctx, "MB").await?;

    events_repo::batch_create_events(ctx, &account_id, &valid_events).await?;

    Ok((ret_valid_events, invalid_events))
}

/// `get_latest_checkout_info_submmited_event` (sic).
pub async fn get_latest_checkout_info_submmited_event(
    _ctx: &Ctx,
    account: &AuthAccount,
    checkout_id: &str,
) -> Result<Value, ApiError> {
    let account_id = account_id_str(account)?;
    // EventsService(current_account) also constructs BillingUnits, so the
    // a_t/a_c claim validation applies to this route too.
    let _billing = BillingUnits::new(account, &account_id, "items_data")?;

    let event = events_repo::get_latest_checkout_info_submmited_event(&account_id, checkout_id).await?;
    event.ok_or_else(|| {
        ApiError::reason(400, "No event found", "No event found with provided params", "")
    })
}

/// `get_events` (POST /v1/internal/events).
pub async fn get_events(
    ctx: &Ctx,
    account_id: &str,
    timeframe: i64,
    cursor: i64,
    event_sequence_event: Option<&Value>,
    profile_ids: Option<&Vec<String>>,
    event_type: Option<&str>,
) -> Result<(Vec<Value>, i64), ApiError> {
    let (events, total) = events_repo::get_events(
        ctx,
        account_id,
        timeframe,
        cursor,
        event_sequence_event,
        profile_ids,
        event_type,
    )
    .await?;
    if events.is_empty() {
        return Err(ApiError::reason(
            400,
            "No events found",
            "No events found with provided params or pagination cursor exhausted",
            "",
        ));
    }
    Ok((events, total))
}

/// `delete_account_events_internal` (POST /v1/internal/events/delete).
///
/// Divergence: the Python called `eventTypesRepository.delete_account_event_types`
/// without awaiting the coroutine, so custom event types were silently never
/// deleted; the port performs the intended deletion.
pub async fn delete_account_events_internal(_ctx: &Ctx, account_id: &str) -> Result<bool, ApiError> {
    let result = async {
        events_repo::delete_account_events(account_id).await?;
        event_types_repo::delete_account_event_types(account_id).await?;
        Ok::<(), ApiError>(())
    }
    .await;

    match result {
        Ok(()) => Ok(true),
        Err(err) => {
            eprintln!("[events-service] delete_account_events_internal failed: {err}");
            Err(ApiError::reason(
                500,
                "Server error",
                "An error occurred while attempting to delete events for this account. Please try again later.",
                "",
            ))
        }
    }
}

/// `isinstance(new_value, type(old_value))` over JSON values, with the Python
/// numeric-tower quirks (`bool` is an `int` subclass; `int` is not a `float`).
fn py_isinstance(new_value: &Value, old_value: &Value) -> bool {
    match old_value {
        Value::String(_) => new_value.is_string(),
        Value::Bool(_) => new_value.is_boolean(),
        Value::Number(n) => {
            if n.is_f64() {
                // type(old) is float — ints/bools are not floats
                new_value.as_f64().is_some() && new_value.as_i64().is_none() && !new_value.is_boolean()
            } else {
                // type(old) is int — isinstance(True, int) is True
                new_value.is_i64() || new_value.is_u64() || new_value.is_boolean()
            }
        }
        Value::Array(_) => new_value.is_array(),
        Value::Object(_) => new_value.is_object(),
        Value::Null => new_value.is_null(),
    }
}

/// Port of `EventsService._verify_event`.
#[allow(clippy::too_many_arguments)]
async fn verify_event(
    ctx: &Ctx,
    account: &AuthAccount,
    account_id: &str,
    event_type: &str,
    event_properties: &Map<String, Value>,
    latest_event: Option<&Value>,
    profile_id: &str,
    ivps: &[Value],
) -> Result<VerifyResult, ApiError> {
    let mut event_type_id = Value::Null;

    // Verify profile_id is valid
    if ivps
        .iter()
        .any(|v| v.as_str() == Some(profile_id) || *v == json!(profile_id))
    {
        return Ok(Err((
            "Unknown profile_id supplied with this event instance".to_string(),
            "Invalid event data provided".to_string(),
        )));
    }

    let system_event_types: Vec<String> = ctx
        .config
        .get_array("SYSTEM_EVENT_TYPES")
        .map_err(ApiError::from)?
        .iter()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect();

    if system_event_types.iter().any(|s| s == event_type) {
        if event_type == "charged" {
            // AMQP call to set the customer profile 'has_charged' = True
            // (published before the property checks, exactly like the Python)
            ctx.gateway
                .amqp_publish(
                    "profiles.cmd.update",
                    &json!({
                        "account_id": account_id,
                        "profiles": [
                            { "profile_id": profile_id, "has_charged": true }
                        ]
                    }),
                )
                .await
                .map_err(ApiError::from)?;

            event_type_id = json!(event_type);

            let mut payment_method: Option<&Value> = None;
            let mut item_id: Option<&Value> = None;
            for (k, v) in event_properties {
                // every provided value must be a string
                if !v.is_string() {
                    return Ok(Err((
                        "Event type 'charged'. The values provided for the 'payment_method' and 'item_id' event properties must be of type string. This charge has been logged against the customers profile but will not be used in any training jobs.".to_string(),
                        "Invalid event data provided".to_string(),
                    )));
                }
                let len = v.as_str().map(|s| s.chars().count()).unwrap_or(0);
                if k == "payment_method" && len > 1 {
                    payment_method = Some(v);
                } else if k == "item_id" && len > 1 {
                    item_id = Some(v);
                }
            }
            if payment_method.is_none() || item_id.is_none() {
                return Ok(Err((
                    "Events of type 'charged' must be provided with 'payment_method' and 'item_id' parameters within the event_properties. This charge has been logged against the customers profile but will not be used in any training jobs.".to_string(),
                    "Invalid event data provided".to_string(),
                )));
            }
        } else if event_type == "churned" {
            // AMQP call to set the customer profile status = 'churned'
            ctx.gateway
                .amqp_publish(
                    "profiles.cmd.update",
                    &json!({
                        "account_id": account_id,
                        "profiles": [
                            { "profile_id": profile_id, "status": "churned" }
                        ]
                    }),
                )
                .await
                .map_err(ApiError::from)?;

            event_type_id = json!(event_type);
        } else if event_type == "complaint" {
            let mut channel: Option<&Value> = None;
            for (k, v) in event_properties {
                if !v.is_string() {
                    return Ok(Err((
                        "Event type 'complaint'. The values provided for the 'channel' event property must be of type string.".to_string(),
                        "Invalid event data provided".to_string(),
                    )));
                }
                let len = v.as_str().map(|s| s.chars().count()).unwrap_or(0);
                if k == "channel" && len > 1 {
                    channel = Some(v);
                }
            }
            if channel.is_none() {
                return Ok(Err((
                    "Events of type 'complaint' must be provided with a 'channel' parameter within the event_properties.".to_string(),
                    "Invalid event data provided".to_string(),
                )));
            }
            event_type_id = json!(event_type);
        }
        // other system event types: event_type_id stays null (Python quirk)
    } else {
        // not a system event type — must be an existing custom event type
        let custom_event_type =
            event_types_repo::get_event_type_by_name(ctx, account_id, event_type).await?;
        let Some(custom_event_type) = custom_event_type else {
            return Ok(Err((
                "Unknown event type supplied with this request.".to_string(),
                "Invalid event_type.".to_string(),
            )));
        };

        event_type_id = custom_event_type["event_type_id"].clone();

        let event_instance_exists = latest_event.is_some();

        // map used to assess provided event properties (keys and value types)
        struct RequiredProperty {
            property: Value,
            provided: bool,
            property_type_match: bool,
        }
        let mut required_event_properties_map: Vec<RequiredProperty> = Vec::new();
        // NB: sticky across iterations, exactly like the Python local
        let mut property_type_match = true;

        let expected_props = custom_event_type
            .get("event_properties")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for ep in &expected_props {
            let ep_key = match ep.as_str() {
                Some(s) => s.to_string(),
                None => ep.to_string(),
            };
            if event_instance_exists {
                // assess required data type (KeyError on either side → pass)
                if let Some(new_value) = event_properties.get(&ep_key) {
                    if let Some(old_value) = latest_event
                        .and_then(|le| le.get("event_properties"))
                        .and_then(|props| props.get(&ep_key))
                    {
                        if !py_isinstance(new_value, old_value) {
                            property_type_match = false;
                        }
                    }
                }
            }
            required_event_properties_map.push(RequiredProperty {
                property: ep.clone(),
                provided: event_properties.contains_key(&ep_key),
                property_type_match,
            });
        }

        // evaluate map
        for rep in &required_event_properties_map {
            if !rep.provided {
                let property = match rep.property.as_str() {
                    Some(s) => s.to_string(),
                    None => rep.property.to_string(),
                };
                return Ok(Err((
                    format!("Please provide all required event properties key value pairs for this event type. Missing event property key : '{property}'"),
                    "Invalid event data provided".to_string(),
                )));
            }
            if !rep.property_type_match {
                return Ok(Err((
                    "Invalid data types specified for one or more of the provided event properties.".to_string(),
                    "Invalid event data provided".to_string(),
                )));
            }
        }
    }

    // validate item identifiers against this account's algorithm configurations
    let algo_confs = account
        .algorithm_configurations
        .as_array()
        .cloned()
        .unwrap_or_default();
    for algo_conf in &algo_confs {
        let Some(config) = algo_conf.get("config_json") else {
            continue; // KeyError → continue
        };
        // len(config) — TypeError for un-sized values surfaced as a 500
        let config_len = match config {
            Value::Object(m) => m.len(),
            Value::Array(a) => a.len(),
            Value::String(s) => s.chars().count(),
            _ => return Err(ApiError::internal("TypeError: len() of config_json")),
        };
        if config_len == 0 {
            continue;
        }

        // iid is built before the event_type comparison — a missing
        // algorithm_name raised KeyError → 500
        let algorithm_name = algo_conf
            .get("algorithm_name")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::internal("KeyError: algo_conf['algorithm_name']"))?;
        let iid = format!("{algorithm_name}_item_identifier");

        // config['event_type'] — KeyError → 500
        let config_event_type = config
            .get("event_type")
            .ok_or_else(|| ApiError::internal("KeyError: config_json['event_type']"))?;

        if config_event_type.as_str() == Some(event_type) {
            // try: item_identifier = event_properties[config[iid]]
            let identifier_key = config.get(&iid).and_then(Value::as_str);
            let item_identifier = identifier_key.and_then(|key| event_properties.get(key));
            match item_identifier {
                Some(value) => {
                    if value.is_null() || value.as_str() == Some("") {
                        let a = identifier_key.unwrap_or_default();
                        return Ok(Err((
                            format!("event_properties -- {a} can not contain a null value as this event is a primary event type."),
                            "Invalid event data provided".to_string(),
                        )));
                    }
                }
                None => {
                    // KeyError branch
                    if algorithm_name == "rec" {
                        let e = match &event_type_id {
                            Value::String(s) => s.clone(),
                            Value::Null => "None".to_string(),
                            other => other.to_string(),
                        };
                        return Ok(Err((
                            format!("The event type: '{e}' is currently set as this accounts recommendations event type. Please supply the rec_item_identifier key. ex. 'item_id' with a relevant value within the event_properties."),
                            "Invalid event data provided".to_string(),
                        )));
                    }
                }
            }
        }
    }

    Ok(Ok(event_type_id))
}
