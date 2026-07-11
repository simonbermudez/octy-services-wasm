//! Port of `segmentation_engine.py::PendingLiveSegmentation`.
//!
//! This job is only ever scheduled to run *after* a sub_type-2 segment's
//! action->inaction deadline has passed, so by the time it runs we can
//! assert the deadline is already behind us. It re-fetches events for the
//! deadline window; if no matching "inaction" event turns up, the pending
//! tag is promoted to active, otherwise it's deleted.

use super::*;

pub struct PendingLiveSegmentation<'a> {
    ctx: &'a Ctx,
    account_id: String,
    webhook_url: String,
    segment_id: String,
    profile_id: String,
    octy_job_id: String,
    live_octy_job_id: String,
    event_timeframe: i64,
    gsdo: Gsdo,
    profile: Option<Value>,
    segment: Option<Value>,
    found_past_inaction_events: Vec<Value>,
}

impl<'a> PendingLiveSegmentation<'a> {
    pub fn new(
        ctx: &'a Ctx,
        account_id: String,
        webhook_url: String,
        segment_id: String,
        profile_id: String,
        octy_job_id: String,
        live_octy_job_id: String,
        event_timeframe: i64,
    ) -> Self {
        let gsdo = Gsdo::new(&account_id);
        Self {
            ctx,
            account_id,
            webhook_url,
            segment_id,
            profile_id,
            octy_job_id,
            live_octy_job_id,
            event_timeframe: event_timeframe + 1,
            gsdo,
            profile: None,
            segment: None,
            found_past_inaction_events: Vec::new(),
        }
    }

    async fn delete_tag(&mut self, tag: &Value, profile_id: &str) {
        self.gsdo.add(self.ctx, delete_tag_op(tag, profile_id)).await;
    }

    /// `_send_webhook_request` — defined in the Python source but never
    /// called anywhere in `PendingLiveSegmentation`; ported for parity.
    #[allow(dead_code)]
    async fn send_webhook_request(&self, segment: &Value) {
        eprintln!("[segmentation-worker] Sending webhook [new Live segment tag created] -- {}", Utc::now());
        let profile_id = self.profile.as_ref().and_then(|p| p.get("profile_id")).cloned().unwrap_or(Value::Null);
        let payload = json!({
            "subject": "Live segment tag created",
            "body": {
                "profile_id": profile_id,
                "segment": {
                    "segment_id": segment.get("segment_id").cloned().unwrap_or(Value::Null),
                    "segment_tag": segment.get("segment_name").cloned().unwrap_or(Value::Null),
                }
            },
            "date_time": Utc::now().to_string(),
        });
        post_json_best_effort(&self.webhook_url, &payload).await;
    }

    /// `_update_live_segment_tag` (reachable). Loops over *every* matching
    /// tag (not just the first), so if a profile somehow carries more than
    /// one active/pending tag for this segment, the webhook below fires once
    /// per tag — preserved verbatim. Uses the raising (job-callback-style)
    /// POST, so a webhook failure here aborts the rest of `run()` even
    /// though the tag-update operations were already enqueued into `gsdo`
    /// (and will still be flushed by `exit_segmentation_process`).
    async fn update_live_segment_tag(&mut self, status: &str) -> Result<(), OctyError> {
        let segment_id = self.segment.as_ref().and_then(|s| s.get("segment_id")).cloned().unwrap_or(Value::Null);
        let profile = self.profile.clone().unwrap_or(Value::Null);
        let profile_id = profile.get("profile_id").and_then(Value::as_str).unwrap_or("").to_string();
        let tags = tags_matching(&profile, &segment_id, &["active", "pending"]);

        for tag in &tags {
            let tag_status = tag.get("status").and_then(Value::as_str).unwrap_or("");
            if tag_status == "active" && status == "active" {
                // no-op, matches Python's `pass`
            } else {
                self.gsdo.add(self.ctx, update_tag_op(tag, &profile_id, status)).await;
            }
            if status == "active" {
                eprintln!("[segmentation-worker] Sending webhook [new Live segment tag created] -- {}", Utc::now());
                let payload = json!({
                    "subject": "Live segment tag created",
                    "body": {
                        "profile_id": profile_id,
                        "segment": {
                            "segment_id": tag.get("segment_id").cloned().unwrap_or(Value::Null),
                            "segment_tag": tag.get("segment_tag").cloned().unwrap_or(Value::Null),
                        }
                    },
                    "date_time": Utc::now().to_string(),
                });
                post_json_propagating(&self.webhook_url, &[], &payload).await?;
            }
        }
        Ok(())
    }

    /// `_delete_live_segment_tag` (reachable). Return value unused by the caller.
    async fn delete_live_segment_tag(&mut self) {
        let segment_id = self.segment.as_ref().and_then(|s| s.get("segment_id")).cloned().unwrap_or(Value::Null);
        let profile = self.profile.clone().unwrap_or(Value::Null);
        let profile_id = profile.get("profile_id").and_then(Value::as_str).unwrap_or("").to_string();
        let tags = tags_matching(&profile, &segment_id, &["active", "pending"]);
        if tags.is_empty() {
            eprintln!("[segmentation-worker] No tag found for segment and profile.. skipping");
            return;
        }
        for tag in &tags {
            self.delete_tag(tag, &profile_id).await;
        }
    }

    async fn get_profile(&mut self) -> Result<(), OctyError> {
        let profiles = repository::get_profiles_by_id(self.ctx, &self.account_id, &[self.profile_id.clone()]).await?;
        if profiles.is_empty() {
            return Err(OctyError::internal(format!(
                "No profiles associated with this account, or existing profiles have conducted no event instances. Account ID : {}",
                self.account_id
            )));
        }
        self.profile = Some(profiles[0].clone());
        Ok(())
    }

    async fn get_segment(&mut self) -> Result<(), OctyError> {
        let live_segments = repository::get_segment_definitions(self.ctx, &self.account_id, None, Some(&self.segment_id)).await?;
        if live_segments.is_empty() {
            return Err(OctyError::internal(format!("No LIVE segments associated with this account. Account ID : {}", self.account_id)));
        }
        self.segment = Some(live_segments[0].clone());
        Ok(())
    }

    /// `_get_past_inaction_events`. Calls `repository::get_events` with a
    /// non-empty `profile_ids` — which, per the documented bug in
    /// `repository::get_events`, means the `profile_ids` filter is *dropped*
    /// from the outbound request. So this in practice queries inaction
    /// events for the whole account/timeframe/event-type, not scoped to
    /// `self.profile_id` — and nothing downstream re-filters by profile
    /// either. Preserved bug-for-bug; flagged prominently in the port report.
    async fn get_past_inaction_events(&mut self) -> Result<(), OctyError> {
        let sequence = self.segment.as_ref().and_then(|s| s.get("event_sequence")).and_then(Value::as_array).cloned().unwrap_or_default();
        for ev in &sequence {
            if ev.get("action_inaction").and_then(Value::as_str) == Some("inaction") {
                let events = repository::get_events(self.ctx, &self.account_id, self.event_timeframe, ev, Some(&[self.profile_id.clone()])).await?;
                self.found_past_inaction_events.extend(events);
            }
        }
        Ok(())
    }

    async fn delete_octy_jobs(&self) {
        let payload = json!({
            "account_id": self.account_id,
            "octy_job_ids": [self.octy_job_id, self.live_octy_job_id],
            "alt_identifiers": Value::Null,
        });
        if let Err(err) = self.ctx.gateway.amqp_publish("octy.job.cmd.delete", &payload).await {
            eprintln!("[segmentation-worker] failed to publish octy.job.cmd.delete: {err}");
        }
    }

    async fn exit_segmentation_process(&mut self, message: &str, status: &str) {
        eprintln!("[segmentation-worker] {message} -- {}", Utc::now());
        self.gsdo.process(self.ctx).await;
        if let Err(err) = send_job_callback(self.ctx, &self.account_id, &self.octy_job_id, message, status, false).await {
            eprintln!("[segmentation-worker] CRITICAL: error occurred when attempting to exit segmentation process. {err}");
        }
    }

    /// PYTHON BUG (preserved bug-for-bug): the two `if` blocks below
    /// (`invalid_event_sequence` and `meets_criteria`) are sequential, not
    /// mutually exclusive — there is no `elif`/early-return between them.
    /// When `invalid_event_sequence` is true, `meets_criteria` (computed
    /// from the same, all-not-found `events_map`) is *also* true, so both
    /// branches fire: `update_live_segment_tag("active")` and
    /// `delete_octy_jobs()` each get called **twice**, and the "new Live
    /// segment tag created" webhook is sent twice.
    async fn try_run(&mut self) -> Result<(), OctyError> {
        self.get_profile().await?;
        self.get_segment().await?;
        self.get_past_inaction_events().await?;

        if !self.found_past_inaction_events.is_empty() {
            let sequence = self.segment.as_ref().and_then(|s| s.get("event_sequence")).and_then(Value::as_array).cloned().unwrap_or_default();
            let (events_map, invalid_event_sequence) = event_sequence_analysis(&sequence, &self.found_past_inaction_events, true);
            if invalid_event_sequence {
                // No valid inaction event occurred within the defined timeframe.
                self.update_live_segment_tag("active").await?;
                self.delete_octy_jobs().await;
            }
            let meets_criteria = event_map_analysis(&events_map);
            if meets_criteria {
                // No valid inaction event occurred within the defined timeframe.
                self.update_live_segment_tag("active").await?;
                self.delete_octy_jobs().await;
            } else {
                // A valid inaction event did occur within the timeframe.
                self.delete_live_segment_tag().await;
            }
        } else {
            // No inaction events at all were found for this profile -> the
            // deadline is behind us and nothing ever fired, so activate.
            self.update_live_segment_tag("active").await?;
            self.delete_octy_jobs().await;
        }

        Ok(())
    }

    pub async fn run(mut self) {
        match self.try_run().await {
            Ok(()) => self.exit_segmentation_process("pending live segmentation complete.", "success").await,
            Err(err) => {
                let message = err_message(&err);
                self.exit_segmentation_process(&message, "failed").await;
            }
        }
    }
}
