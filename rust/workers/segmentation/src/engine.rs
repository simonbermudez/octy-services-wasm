//! Port of `services/segmentation_engine.py` — `PastSegmentation`,
//! `LiveSegmentation`, `PendingLiveSegmentation`.
//!
//! Orchestration (including several latent Python bugs, preserved
//! bug-for-bug per the porting brief) is documented inline. A summary is
//! also in the crate's final report.
//!
//! DIVERGENCE (applies throughout): Python fired AMQP publishes as detached
//! `loop.create_task(...)` (fire-and-forget on the shared asyncio loop). A
//! Spin HTTP component has no background loop outside the current request,
//! so every publish here is `await`ed inline before continuing. Externally
//! this is unobservable — the same messages are published, just
//! synchronously rather than from a background task.

use std::collections::HashMap;

use chrono::Utc;
use octy_shared::errors::OctyError;
use octy_spin::ctx::Ctx;
use octy_spin::gateway::http_post_json_with_retry;
use serde_json::{json, Value};

use crate::billing::{BillingUnits, ComputeMetric};
use crate::pyval::{json_eq, strconv_convert, v_is};
use crate::repository;

const GSDO_SIZE_LIMIT_BYTES: usize = 104_857_600; // 100 MB (see `cpython_list_sizeof`)

/// `sys.getsizeof(list)` measures the list's *pointer array*, not the size of
/// its contents (CPython 3.7, 64-bit: ~56 bytes + 8 bytes/slot). The Python
/// code used this as a proxy for "the grouped-operations payload is getting
/// too big for one AMQP message", but it isn't one: reaching the 100 MB
/// threshold requires roughly 13.1 million *elements*, regardless of their
/// content size. In practice the mid-loop flush this guards essentially
/// never fires; operations accumulate for the whole job and are flushed once
/// in `_exit_segmentation_process`. Preserved verbatim (bug-for-bug).
fn cpython_list_sizeof(len: usize) -> usize {
    56 + 8 * len
}

fn err_message(err: &OctyError) -> String {
    err.reasons
        .first()
        .map(|r| r.error_message.clone())
        .unwrap_or_else(|| err.error_description.clone())
}

async fn post_json_propagating(url: &str, headers: &[(&str, &str)], payload: &Value) -> Result<(), OctyError> {
    http_post_json_with_retry(url, headers, payload).await?;
    Ok(())
}

async fn post_json_best_effort(url: &str, payload: &Value) {
    if let Err(err) = http_post_json_with_retry(url, &[], payload).await {
        eprintln!("[segmentation-worker] webhook request to {url} failed: {err}");
    }
}

/// Job-service completion callback shared by all three job types.
async fn send_job_callback(
    ctx: &Ctx,
    account_id: &str,
    octy_job_id: &str,
    message: &str,
    status: &str,
    with_cursor_header: bool,
) -> Result<(), OctyError> {
    let base = ctx.config.get_str("OCTY_JOB_SERVICE_CLUSTER_IP")?;
    let url = format!("{base}/v1/internal/jobs/callback");
    let payload = json!({
        "account_id": account_id,
        "octy_job_id": octy_job_id,
        "message": message,
        "status": status,
    });
    let headers: &[(&str, &str)] = if with_cursor_header { &[("cursor", "0")] } else { &[] };
    post_json_propagating(&url, headers, &payload).await
}

// ---------------------------------------------------------------------
// Grouped Segmentation Database Operations (gsdo) — shared by all 3 jobs.
// ---------------------------------------------------------------------

struct Gsdo {
    account_id: String,
    operations: Vec<Value>,
}

impl Gsdo {
    fn new(account_id: &str) -> Self {
        Self {
            account_id: account_id.to_string(),
            operations: Vec::new(),
        }
    }

    /// `_add_sdo` — see `cpython_list_sizeof` for why the flush essentially
    /// never triggers mid-loop in practice.
    async fn add(&mut self, ctx: &Ctx, operation: Value) {
        let should_flush = cpython_list_sizeof(self.operations.len()) > GSDO_SIZE_LIMIT_BYTES;
        self.operations.push(operation);
        if should_flush {
            self.process(ctx).await;
        }
    }

    /// `_process_gsdo`.
    async fn process(&mut self, ctx: &Ctx) {
        if self.operations.is_empty() {
            return;
        }
        let payload = json!({ "account_id": self.account_id, "operations": self.operations });
        if let Err(err) = ctx.gateway.amqp_publish("grouped.segmentation.operations.cmd", &payload).await {
            eprintln!("[segmentation-worker] failed to publish grouped segmentation operations: {err}");
        }
        self.operations.clear();
    }
}

fn update_tag_op(tag: &Value, profile_id: &str, status: &str) -> Value {
    json!({
        "action": "update",
        "operation_payload": {
            "profile_id": profile_id,
            "segment_tags": [{
                "segment_id": tag.get("segment_id").cloned().unwrap_or(Value::Null),
                "segment_tag": tag.get("segment_tag").cloned().unwrap_or(Value::Null),
                "status": status,
            }]
        }
    })
}

/// NB: unlike `update_tag_op`, this reads `tag["segment_name"]` — callers
/// pass the *segment definition* here (which has `segment_name`), not a
/// profile's existing `segment_tags` entry (which has `segment_tag`).
fn create_tag_op(tag: &Value, profile_id: &str, status: &str) -> Value {
    json!({
        "action": "create",
        "operation_payload": {
            "profile_id": profile_id,
            "segment_tags": [{
                "segment_id": tag.get("segment_id").cloned().unwrap_or(Value::Null),
                "segment_tag": tag.get("segment_name").cloned().unwrap_or(Value::Null),
                "status": status,
            }]
        }
    })
}

fn delete_tag_op(tag: &Value, profile_id: &str) -> Value {
    json!({
        "action": "delete",
        "operation_payload": {
            "profile_id": profile_id,
            "segment_tags": [{ "segment_id": tag.get("segment_id").cloned().unwrap_or(Value::Null) }]
        }
    })
}

// ---------------------------------------------------------------------
// Event-sequence matching — shared by PastSegmentation and
// PendingLiveSegmentation (`_event_sequence_analysis` / `_event_map_analysis`).
// ---------------------------------------------------------------------

/// Python computed a `time_stamp` per matched event but no caller ever reads
/// it afterwards (verified across all three classes) — this port omits it.
/// This also sidesteps a latent format mismatch in the Python `str_to_dt`
/// (it parses the RFC-1123-ish `'%a, %d %b %Y %H:%M:%S GMT'` shape, not the
/// ISO-8601 `created_at` the event-service actually returns; had the value
/// been used, this would have thrown for every real event).
struct EventMapEntry {
    found: bool,
    action_inaction: String,
}

/// If a segment definition supplies no `event_properties`, the client is
/// assumed to want to segment on event type alone. When properties *are*
/// supplied, the event only needs to contain that key:value subset — extra
/// keys on the actual event are ignored, it's not an exact-match comparison.
fn event_properties_match(required: &Value, actual_event_properties: &Value) -> bool {
    match required {
        Value::Null => true,
        Value::Object(map) => map
            .iter()
            .all(|(k, v)| actual_event_properties.get(k).map_or(false, |av| json_eq(av, v))),
        _ => true,
    }
}

/// Port of both `PastSegmentation._event_sequence_analysis` and
/// `PendingLiveSegmentation._event_sequence_analysis`.
///
/// The Python implementations thread a per-event-sequence-event
/// `found`/`break`/`continue` state machine across every candidate event,
/// but (a) `seg_events_prop_map` is fully overwritten on every type-matching
/// event, so no state actually carries across events, and (b) once `found`
/// flips true it is never reset. The net, externally observable effect is
/// exactly: "did any candidate event of this type satisfy the required
/// event_properties (or need none)?" — implemented directly here.
/// `inaction_only`, when set, restricts consideration to
/// `action_inaction == "inaction"` sequence entries (the PendingLive variant
/// only ever populated its `events_map` with those).
fn event_sequence_analysis(
    sequence: &[Value],
    candidate_events: &[Value],
    inaction_only: bool,
) -> (HashMap<String, EventMapEntry>, bool) {
    let mut events_map: HashMap<String, EventMapEntry> = HashMap::new();
    for ev_seq in sequence {
        let action_inaction = ev_seq.get("action_inaction").and_then(Value::as_str).unwrap_or("").to_string();
        if inaction_only && action_inaction != "inaction" {
            continue;
        }
        let event_type = ev_seq.get("event_type").and_then(Value::as_str).unwrap_or("").to_string();
        events_map
            .entry(event_type)
            .or_insert(EventMapEntry { found: false, action_inaction });
    }

    let mut invalid_event_sequence = true;
    for ev_seq in sequence {
        let action_inaction = ev_seq.get("action_inaction").and_then(Value::as_str).unwrap_or("");
        if inaction_only && action_inaction != "inaction" {
            continue;
        }
        let event_type = ev_seq.get("event_type").and_then(Value::as_str).unwrap_or("");
        let required_props = ev_seq.get("event_properties").cloned().unwrap_or(Value::Null);
        let matched = candidate_events.iter().any(|event| {
            event.get("event_type").and_then(Value::as_str) == Some(event_type)
                && event_properties_match(&required_props, &event.get("event_properties").cloned().unwrap_or(Value::Null))
        });
        if matched {
            invalid_event_sequence = false;
            if let Some(entry) = events_map.get_mut(event_type) {
                entry.found = true;
            }
        }
    }
    (events_map, invalid_event_sequence)
}

fn event_map_analysis(events_map: &HashMap<String, EventMapEntry>) -> bool {
    let mut meets_criteria = true;
    for entry in events_map.values() {
        match entry.action_inaction.as_str() {
            "inaction" if entry.found => meets_criteria = false,
            "action" if !entry.found => meets_criteria = false,
            _ => {}
        }
    }
    meets_criteria
}

fn filter_profile_events(events: &[Value], profile_id: &str) -> Vec<Value> {
    events
        .iter()
        .filter(|e| e.get("profile_id").and_then(Value::as_str) == Some(profile_id))
        .cloned()
        .collect()
}

fn tags_matching(profile: &Value, segment_id: &Value, statuses: &[&str]) -> Vec<Value> {
    profile
        .get("segment_tags")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|st| {
            json_eq(st.get("segment_id").unwrap_or(&Value::Null), segment_id)
                && statuses
                    .iter()
                    .any(|s| v_is(st.get("status").unwrap_or(&Value::Null), s))
        })
        .collect()
}

mod live;
mod past;
mod pending_live;

pub use live::LiveSegmentation;
pub use past::PastSegmentation;
pub use pending_live::PendingLiveSegmentation;
