//! Port of `segmentation_engine.py::PastSegmentation`.

use super::*;

pub struct PastSegmentation<'a> {
    ctx: &'a Ctx,
    account_id: String,
    octy_job_id: String,
    segment_id: String,
    billing: BillingUnits,
    gsdo: Gsdo,
    matching_profile_ids: Vec<String>,
    segment: Option<Value>,
}

impl<'a> PastSegmentation<'a> {
    pub fn new(
        ctx: &'a Ctx,
        account_id: String,
        account_type: Option<String>,
        account_currency: Option<String>,
        octy_job_id: String,
        segment_id: String,
    ) -> Self {
        let gsdo = Gsdo::new(&account_id);
        let billing = BillingUnits::new(account_id.clone(), account_type, account_currency, "past_segmentation");
        Self {
            ctx,
            account_id,
            octy_job_id,
            segment_id,
            billing,
            gsdo,
            matching_profile_ids: Vec::new(),
            segment: None,
        }
    }

    async fn delete_tag(&mut self, tag: &Value, profile_id: &str) {
        self.gsdo.add(self.ctx, delete_tag_op(tag, profile_id)).await;
    }

    async fn create_tag(&mut self, tag: &Value, profile_id: &str, status: &str) {
        self.gsdo.add(self.ctx, create_tag_op(tag, profile_id, status)).await;
    }

    /// `_update_tag` — ported for parity; never invoked in the Python source
    /// either (its only call site is commented out).
    #[allow(dead_code)]
    async fn update_tag(&mut self, tag: &Value, profile_id: &str, status: &str) {
        self.gsdo.add(self.ctx, update_tag_op(tag, profile_id, status)).await;
    }

    /// `_delete_past_segment_tag`. Python's boolean return value is never
    /// inspected by callers, so this returns `()`.
    async fn delete_past_segment_tag(&mut self, profile: &Value) {
        let segment_id = self.segment.as_ref().and_then(|s| s.get("segment_id")).cloned().unwrap_or(Value::Null);
        let tags = tags_matching(profile, &segment_id, &["active", "pending"]);
        let profile_id = profile.get("profile_id").and_then(Value::as_str).unwrap_or("").to_string();
        if tags.is_empty() {
            eprintln!("[segmentation-worker] No tag found for segment and profile.. skipping");
            return;
        }
        for tag in &tags {
            let status = tag.get("status").and_then(Value::as_str).unwrap_or("");
            // Both `active` and `pending` tags are deleted outright rather than
            // moved to a "pending deletion" state — there should only ever be
            // one pending tag per segment/profile at a time, so an outright
            // delete avoids needing to track a second pending-deletion status.
            if status == "active" || status == "pending" {
                self.delete_tag(tag, &profile_id).await;
            }
        }
    }

    /// `_create_past_segment_tag`.
    async fn create_past_segment_tag(&mut self, profile: &Value, status: &str) {
        let segment_id = self.segment.as_ref().and_then(|s| s.get("segment_id")).cloned().unwrap_or(Value::Null);
        let profile_id = profile.get("profile_id").and_then(Value::as_str).unwrap_or("").to_string();
        let existing_active = profile
            .get("segment_tags")
            .and_then(Value::as_array)
            .and_then(|tags| {
                tags.iter()
                    .find(|st| json_eq(st.get("segment_id").unwrap_or(&Value::Null), &segment_id) && v_is(st.get("status").unwrap_or(&Value::Null), "active"))
            });
        if existing_active.is_none() {
            let segment = self.segment.clone().unwrap_or(Value::Null);
            self.create_tag(&segment, &profile_id, status).await;
        } else {
            eprintln!(
                "[segmentation-worker] Tag with Segment ID : {:?} already exists in profile with ID : {profile_id}",
                segment_id
            );
        }
    }

    /// `_property_evaluation`.
    async fn property_evaluation(&self, profile: &Value, profile_property_name: &Value, profile_property_value: &Value) -> bool {
        if profile_property_name.is_null() || profile_property_value.is_null() {
            return false;
        }
        let Some(name) = profile_property_name.as_str() else { return false };
        let Some(property_) = profile.get("profile_data").and_then(|d| d.get(name)) else {
            eprintln!("[segmentation-worker] Key error occurred, profile_property_name : {name} not in this profile_data");
            return false;
        };
        // Python: `strconv.convert` on the segment-configured comparison
        // value; see `pyval::strconv_convert` for the documented divergence
        // (the Python conversion could theoretically raise and crash the
        // whole run via an unbound local — not reproduced here).
        let inferred = strconv_convert(profile_property_value);
        if !json_eq(property_, &inferred) {
            eprintln!("[segmentation-worker] Profile did not match this segments required profile property value");
            return false;
        }
        eprintln!("[segmentation-worker] Profile matched this segments required profile property value");
        true
    }

    /// `_get_profiles`. Unions profiles that met this segment's criteria on
    /// the *previous* run (`segment.profile_ids`) with profiles behind this
    /// run's matched events, so profiles that no longer qualify are still
    /// re-evaluated (and have their tag revoked) rather than silently
    /// dropped from consideration.
    async fn get_profiles(&mut self, past_events: &[Value]) -> Result<Vec<Value>, OctyError> {
        let mut ids: Vec<String> = self
            .segment
            .as_ref()
            .and_then(|s| s.get("profile_ids"))
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        for ev in past_events {
            if let Some(pid) = ev.get("profile_id").and_then(Value::as_str) {
                ids.push(pid.to_string());
            }
        }
        ids.sort();
        ids.dedup();
        let profiles = repository::get_profiles_by_id(self.ctx, &self.account_id, &ids).await?;
        if profiles.is_empty() {
            return Err(OctyError::internal(format!(
                "No profiles associated with this account, or existing profiles have conducted no event instances. Account ID : {}",
                self.account_id
            )));
        }
        Ok(profiles)
    }

    /// `_get_segment`.
    async fn get_segment(&mut self) -> Result<(), OctyError> {
        let segments = repository::get_segment_definitions(self.ctx, &self.account_id, None, Some(&self.segment_id)).await?;
        if segments.is_empty() {
            return Err(OctyError::internal(format!("No segment found with ID : {}", self.segment_id)));
        }
        self.segment = Some(segments[0].clone());
        Ok(())
    }

    async fn exit_segmentation_process(&mut self, message: &str, status: &str, segments_summary: Option<&Value>) {
        eprintln!("[segmentation-worker] {message} -- {}", Utc::now());
        if let Some(summary) = segments_summary {
            eprintln!("[segmentation-worker] {summary}");
        }
        self.gsdo.process(self.ctx).await;
        match send_job_callback(self.ctx, &self.account_id, &self.octy_job_id, message, status, true).await {
            Ok(()) => {
                self.billing.complete_compute_units(self.ctx, 0.0).await;
            }
            Err(err) => {
                self.billing.complete_compute_units(self.ctx, 0.0).await;
                eprintln!("[segmentation-worker] CRITICAL: error occurred when attempting to exit segmentation process. {err}");
            }
        }
    }

    async fn try_run(&mut self) -> Result<(String, Value), OctyError> {
        self.billing.track_compute_units(ComputeMetric::Hours);
        self.get_segment().await?;
        let mut segment_customer_count: i64 = 0;

        eprintln!("[segmentation-worker] Past segmentation -- {}", Utc::now());
        eprintln!("[segmentation-worker] Segment_id: {}", self.segment_id);

        let sequence = self
            .segment
            .as_ref()
            .and_then(|s| s.get("event_sequence"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let segment_timeframe_days = self.segment.as_ref().and_then(|s| s.get("segment_timeframe")).and_then(Value::as_i64).unwrap_or(0);
        let segment_timeframe_minutes = segment_timeframe_days * 24 * 60;

        let mut found_past_events: Vec<Value> = Vec::new();
        for ev in &sequence {
            let events = repository::get_events(self.ctx, &self.account_id, segment_timeframe_minutes, ev, None).await?;
            found_past_events.extend(events);
        }

        let profiles = self.get_profiles(&found_past_events).await?;
        let segment_id_value = self.segment.as_ref().and_then(|s| s.get("segment_id")).cloned().unwrap_or(Value::Null);

        for profile in &profiles {
            let profile_id = profile.get("profile_id").and_then(Value::as_str).unwrap_or("").to_string();
            let active_tag_exists = !tags_matching(profile, &segment_id_value, &["active"]).is_empty();

            eprintln!("[segmentation-worker] ======================================== Processing profile with ID: {profile_id}");

            let past_events = filter_profile_events(&found_past_events, &profile_id);
            let (events_map, invalid_event_sequence) = event_sequence_analysis(&sequence, &past_events, false);
            if invalid_event_sequence {
                if active_tag_exists {
                    self.delete_past_segment_tag(profile).await;
                }
                continue;
            }

            let meets_criteria = event_map_analysis(&events_map);
            if meets_criteria {
                let sub_type = self.segment.as_ref().and_then(|s| s.get("segment_sub_type")).and_then(Value::as_i64).unwrap_or(0);
                if sub_type == 3 || sub_type == 4 {
                    let name = self.segment.as_ref().and_then(|s| s.get("profile_property_name")).cloned().unwrap_or(Value::Null);
                    let value = self.segment.as_ref().and_then(|s| s.get("profile_property_value")).cloned().unwrap_or(Value::Null);
                    if !self.property_evaluation(profile, &name, &value).await {
                        self.delete_past_segment_tag(profile).await;
                        continue;
                    }
                }
                self.matching_profile_ids.push(profile_id);
                self.create_past_segment_tag(profile, "active").await;
                segment_customer_count += 1;
            } else {
                self.delete_past_segment_tag(profile).await;
                continue;
            }
        }

        repository::update_segment_profiles_ids(self.ctx, &self.account_id, &self.segment_id, &self.matching_profile_ids).await?;

        let msg = if segment_customer_count < 1 {
            "Segmentation proccess complete. No customer events currently meet this segments criteria.".to_string()
        } else {
            "Segmentation proccess complete.".to_string()
        };
        let segments_summary = json!([{
            "segment_id": self.segment.as_ref().and_then(|s| s.get("segment_id")).cloned().unwrap_or(Value::Null),
            "segment_name": self.segment.as_ref().and_then(|s| s.get("segment_name")).cloned().unwrap_or(Value::Null),
            "segment_type": self.segment.as_ref().and_then(|s| s.get("segment_type")).cloned().unwrap_or(Value::Null),
            "count": segment_customer_count,
        }]);
        Ok((msg, segments_summary))
    }

    pub async fn run(mut self) {
        match self.try_run().await {
            Ok((msg, summary)) => {
                self.exit_segmentation_process(&msg, "success", Some(&summary)).await;
            }
            Err(err) => {
                eprintln!("[segmentation-worker] CRITICAL: {err} -- {}", Utc::now());
                self.exit_segmentation_process("Past segmentation exitied early due to an error.", "failed", None).await;
            }
        }
    }
}
