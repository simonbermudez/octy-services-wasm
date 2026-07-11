//! Port of `segmentation_engine.py::LiveSegmentation`.
//!
//! Two Python methods on this class — `_delete_live_segment_tag` and
//! `_update_live_segment_tag` — are unreachable dead code: their only caller,
//! `_segment_type_two_pending_tag_analysis`, is itself never invoked (its
//! call site in `run()` is commented out). Both also reference
//! `self.segment`, an attribute this class never assigns anywhere else,
//! which would raise `AttributeError` on the (never-taken) call path, and
//! `_update_live_segment_tag`'s webhook payload reads `tag['segment_name']`
//! on a dict shape (`profile.segment_tags` entries) that only ever has
//! `segment_tag` — a second latent bug on the same dead path. All three are
//! ported below for completeness (`#[allow(dead_code)]`, `segment` taken as
//! an explicit parameter instead of a phantom `self` attribute) but are not
//! wired into `run()`, matching production behaviour exactly.

use super::*;

pub struct LiveSegmentation<'a> {
    ctx: &'a Ctx,
    account_id: String,
    webhook_url: String,
    octy_job_id: String,
    event: Value,
    gsdo: Gsdo,
    live_validation_octy_job_time_buffer: i64,
    es_event_property_map: HashMap<String, bool>,
    profile: Option<Value>,
}

impl<'a> LiveSegmentation<'a> {
    pub fn new(ctx: &'a Ctx, account_id: String, webhook_url: String, octy_job_id: String, event: Value) -> Self {
        let gsdo = Gsdo::new(&account_id);
        Self {
            ctx,
            account_id,
            webhook_url,
            octy_job_id,
            event,
            gsdo,
            live_validation_octy_job_time_buffer: 2,
            es_event_property_map: HashMap::new(),
            profile: None,
        }
    }

    async fn delete_tag(&mut self, tag: &Value, profile_id: &str) {
        self.gsdo.add(self.ctx, delete_tag_op(tag, profile_id)).await;
    }

    async fn create_tag(&mut self, tag: &Value, profile_id: &str, status: &str) {
        self.gsdo.add(self.ctx, create_tag_op(tag, profile_id, status)).await;
    }

    #[allow(dead_code)]
    async fn update_tag(&mut self, tag: &Value, profile_id: &str, status: &str) {
        self.gsdo.add(self.ctx, update_tag_op(tag, profile_id, status)).await;
    }

    /// `_delete_live_segment_tag` — dead code, see module docs.
    #[allow(dead_code)]
    async fn delete_live_segment_tag(&mut self, segment: &Value) {
        let profile = self.profile.clone().unwrap_or(Value::Null);
        let segment_id = segment.get("segment_id").cloned().unwrap_or(Value::Null);
        let tags = tags_matching(&profile, &segment_id, &["active", "pending"]);
        let profile_id = profile.get("profile_id").and_then(Value::as_str).unwrap_or("").to_string();
        for tag in &tags {
            self.delete_tag(tag, &profile_id).await;
        }
    }

    /// `_update_live_segment_tag` — dead code, see module docs. `tag['segment_name']`
    /// is preserved as a `.get()` (yields `null` instead of Python's KeyError).
    #[allow(dead_code)]
    async fn update_live_segment_tag(&mut self, segment: &Value, status: &str) -> Result<(), OctyError> {
        let profile = self.profile.clone().unwrap_or(Value::Null);
        let segment_id = segment.get("segment_id").cloned().unwrap_or(Value::Null);
        let profile_id = profile.get("profile_id").and_then(Value::as_str).unwrap_or("").to_string();
        let tag = profile
            .get("segment_tags")
            .and_then(Value::as_array)
            .and_then(|tags| tags.iter().find(|st| json_eq(st.get("segment_id").unwrap_or(&Value::Null), &segment_id) && v_is(st.get("status").unwrap_or(&Value::Null), "pending")))
            .cloned();
        let Some(tag) = tag else { return Ok(()) };
        self.update_tag(&tag, &profile_id, status).await;
        if status == "active" {
            eprintln!("[segmentation-worker] Sending webhook [new Live segment tag created] -- {}", Utc::now());
            let payload = json!({
                "subject": "Live segment tag created",
                "body": {
                    "profile_id": profile_id,
                    "segment": {
                        "segment_id": tag.get("segment_id").cloned().unwrap_or(Value::Null),
                        "segment_tag": tag.get("segment_name").cloned().unwrap_or(Value::Null),
                    }
                },
                "date_time": Utc::now().to_string(),
            });
            post_json_propagating(&self.webhook_url, &[], &payload).await?;
        }
        Ok(())
    }

    /// `_create_live_validation_octy_job` (reachable).
    async fn create_live_validation_octy_job(&self, segment_event: &Value, segment_id: &str) {
        let exp_timeframe = segment_event.get("exp_timeframe").and_then(Value::as_i64).unwrap_or(0);
        let time_interval = exp_timeframe + self.live_validation_octy_job_time_buffer;
        let profile_id = self.profile.as_ref().and_then(|p| p.get("profile_id")).cloned().unwrap_or(Value::Null);
        let payload = json!({
            "account_id": self.account_id,
            "job_meta": {
                "job_type": "pending-live",
                "amqp_routing_key": "live.segmentation.cmd.run",
                "required_permissions": ["seg"],
                "required_configurations": {
                    "account_attributes": ["account_configurations.webhook_url"],
                    "algorithm_configuration_idxs": []
                },
                "desired_runs": 1,
                "time_interval": time_interval,
                "fail_threshold": 10
            },
            "job_data": {
                "segment_data": { "segmentation_type": "pending-live", "segment_id": segment_id },
                "event_data": { "profile": { "profile_id": profile_id }, "event_timeframe": 5 },
                "validation_job": true,
                "live_octy_job_id": self.octy_job_id
            }
        });
        if let Err(err) = self.ctx.gateway.amqp_publish("octy.job.cmd.create", &payload).await {
            eprintln!("[segmentation-worker] failed to publish octy.job.cmd.create: {err}");
        }
        eprintln!("[segmentation-worker] Creating new octy-job for this profile and segment with a time interval of {time_interval} minutes");
    }

    /// `_get_profile`.
    async fn get_profile(&mut self) -> Result<(), OctyError> {
        let profile_id = self.event.get("profile").and_then(|p| p.get("profile_id")).and_then(Value::as_str).unwrap_or("").to_string();
        let profiles = repository::get_profiles_by_id(self.ctx, &self.account_id, &[profile_id]).await?;
        if profiles.is_empty() {
            return Err(OctyError::internal(format!(
                "No profiles associated with this account, or existing profiles have conducted no event instances. Account ID : {}",
                self.account_id
            )));
        }
        self.profile = Some(profiles[0].clone());
        Ok(())
    }

    /// `_event_sequence_event_property_analysis`. Always returns `false`
    /// (Python's `invalid_event_sequence_event` is initialized `False` and
    /// never reassigned in either branch — a no-op guard preserved verbatim).
    fn event_sequence_event_property_analysis(&mut self, event_sequence_event: &Value) -> bool {
        let required = event_sequence_event.get("event_properties").cloned().unwrap_or(Value::Null);
        if required.is_null() {
            eprintln!("[segmentation-worker] No event properties required for this event sequence event");
            return false;
        }
        eprintln!("[segmentation-worker] Event properties ARE required for this event sequence event. Assessing provided event properties...");
        if let Value::Object(map) = &required {
            for (k, v) in map {
                let actual = self.event.get("event_properties").and_then(|p| p.get(k));
                let matched = actual.map_or(false, |av| json_eq(av, v));
                if actual.is_none() {
                    eprintln!("[segmentation-worker] Required key {k} not present in event event properties, continuing...");
                }
                self.es_event_property_map.insert(k.clone(), matched);
            }
        }
        false
    }

    /// `_es_event_property_map_analysis`.
    fn es_event_property_map_analysis(&self) -> bool {
        self.es_event_property_map.values().any(|found| !*found)
    }

    /// `_create_live_segment_tag`. Returns `(did_succeed, tag_status)`;
    /// `did_succeed` is always `true` here since tag-op enqueueing cannot
    /// fail synchronously in this port (the Python version could only
    /// return `false` from an unexpected exception).
    async fn create_live_segment_tag(&mut self, segment: &Value, status: &str) -> (bool, Option<String>) {
        let segment_id = segment.get("segment_id").cloned().unwrap_or(Value::Null);
        let profile = self.profile.clone().unwrap_or(Value::Null);
        let profile_id = profile.get("profile_id").and_then(Value::as_str).unwrap_or("").to_string();
        let tags = tags_matching(&profile, &segment_id, &["active", "pending"]);

        if let Some(tag) = tags.first() {
            let tag_status = tag.get("status").and_then(Value::as_str).unwrap_or("");
            if tag_status == "active" {
                eprintln!(
                    "[segmentation-worker] Active tag exists for segment with ID: {segment_id:?}. Deleting existing active tag"
                );
                self.delete_tag(tag, &profile_id).await;
                self.create_tag(segment, &profile_id, status).await;
                return (true, Some("new".to_string()));
            } else if tag_status == "pending" {
                eprintln!("[segmentation-worker] Tag already exists or segment with ID: {segment_id:?}, skipping...");
                return (true, Some("pending".to_string()));
            }
        } else {
            self.create_tag(segment, &profile_id, status).await;
            return (true, Some("new".to_string()));
        }
        (true, Some("new".to_string()))
    }

    async fn send_http_account_webhook_request(&self, payload: &Value) {
        post_json_best_effort(&self.webhook_url, payload).await;
    }

    /// `_send_webhook_request`.
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
        self.send_http_account_webhook_request(&payload).await;
    }

    /// `_segment_type_one_analysis`.
    async fn segment_type_one_analysis(&mut self, segment: &Value) -> Result<(), OctyError> {
        let sequence = segment.get("event_sequence").and_then(Value::as_array).cloned().unwrap_or_default();
        for event_sequence_event in &sequence {
            self.es_event_property_map.clear();
            let seq_type = event_sequence_event.get("event_type").and_then(Value::as_str).unwrap_or("");
            let ev_type = self.event.get("event_type").and_then(Value::as_str).unwrap_or("");
            if ev_type != seq_type {
                eprintln!("[segmentation-worker] Skipping...");
                return Ok(());
            }
            let invalid_event_sequence_event = self.event_sequence_event_property_analysis(event_sequence_event);
            if invalid_event_sequence_event {
                eprintln!("[segmentation-worker] Skipping... Invalid event sequence > event");
                return Ok(());
            }
            let invalid_event_properties = self.es_event_property_map_analysis();
            if !invalid_event_properties {
                let (res, _) = self.create_live_segment_tag(segment, "active").await;
                if !res {
                    return Err(OctyError::internal("Unexpected error occurred when attempting to create segment tag"));
                }
                self.send_webhook_request(segment).await;
            }
            eprintln!("[segmentation-worker] Skipping... Invalid event sequence > event > event properties");
        }
        Ok(())
    }

    /// `_segment_type_two_analysis`.
    async fn segment_type_two_analysis(&mut self, segment: &Value) -> Result<(), OctyError> {
        let sequence = segment.get("event_sequence").and_then(Value::as_array).cloned().unwrap_or_default();
        let segment_id = segment.get("segment_id").and_then(Value::as_str).unwrap_or("").to_string();
        for event_sequence_event in &sequence {
            self.es_event_property_map.clear();
            let seq_type = event_sequence_event.get("event_type").and_then(Value::as_str).unwrap_or("");
            let ev_type = self.event.get("event_type").and_then(Value::as_str).unwrap_or("");
            let action_inaction = event_sequence_event.get("action_inaction").and_then(Value::as_str).unwrap_or("");
            if ev_type == seq_type && action_inaction == "action" {
                let invalid_event_sequence_event = self.event_sequence_event_property_analysis(event_sequence_event);
                if invalid_event_sequence_event {
                    eprintln!("[segmentation-worker] Skipping... Invalid event sequence event");
                    return Ok(());
                }
                let invalid_event_properties = self.es_event_property_map_analysis();
                if !invalid_event_properties {
                    let (res, status) = self.create_live_segment_tag(segment, "pending").await;
                    if !res {
                        return Err(OctyError::internal("Unexpected error occurred when attempting to create segment tag"));
                    }
                    if status.as_deref() == Some("pending") {
                        eprintln!("[segmentation-worker] Octy job exists for this profile and segment.. continuing to next live segment definition");
                        return Ok(());
                    }
                    self.create_live_validation_octy_job(event_sequence_event, &segment_id).await;
                } else {
                    eprintln!("[segmentation-worker] Skipping... Invalid event sequence > event > event properties");
                }
            }
        }
        Ok(())
    }

    async fn exit_segmentation_process(&mut self, message: &str, status: &str) {
        eprintln!("[segmentation-worker] {message} -- {}", Utc::now());
        self.gsdo.process(self.ctx).await;
        if let Err(err) = send_job_callback(self.ctx, &self.account_id, &self.octy_job_id, message, status, false).await {
            eprintln!("[segmentation-worker] CRITICAL: error occurred when attempting to exit segmentation process. {err}");
        }
    }

    async fn try_run(&mut self) -> Result<(), OctyError> {
        self.get_profile().await?;
        let live_segments = repository::get_segment_definitions(self.ctx, &self.account_id, Some("live"), None).await?;
        if live_segments.is_empty() {
            return Err(OctyError::internal(format!("No LIVE segments associated with this account. Account ID : {}", self.account_id)));
        }
        for segment in &live_segments {
            eprintln!(
                "[segmentation-worker] ============================================== Analysing live segment : {:?}",
                segment.get("segment_id")
            );
            let sub_type = segment.get("segment_sub_type").and_then(Value::as_i64).unwrap_or(0);
            if sub_type == 1 {
                self.segment_type_one_analysis(segment).await?;
            } else if sub_type == 2 {
                let segment_id = segment.get("segment_id").cloned().unwrap_or(Value::Null);
                let is_pending_tag = self.profile.as_ref().and_then(|profile| {
                    profile
                        .get("segment_tags")
                        .and_then(Value::as_array)
                        .and_then(|tags| tags.iter().find(|st| json_eq(st.get("segment_id").unwrap_or(&Value::Null), &segment_id) && v_is(st.get("status").unwrap_or(&Value::Null), "pending")))
                });
                if is_pending_tag.is_none() {
                    self.segment_type_two_analysis(segment).await?;
                } else {
                    // Python: analysis of the pending tag is deferred to the
                    // relative pending-live job; the inline re-check here is
                    // commented out in the source (see module docs).
                    eprintln!("[segmentation-worker] Pending tag FOUND for segment: {segment_id:?} and profile");
                }
            }
        }
        Ok(())
    }

    pub async fn run(mut self) {
        match self.try_run().await {
            Ok(()) => self.exit_segmentation_process("Live segmentation job complete", "success").await,
            Err(err) => {
                let message = err_message(&err);
                self.exit_segmentation_process(&message, "failed").await;
            }
        }
    }
}
