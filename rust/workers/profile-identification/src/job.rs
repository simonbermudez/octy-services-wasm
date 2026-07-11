//! Orchestration — port of
//! `services/profile_identification.py::ProfileIdentification`.
//!
//! Runs synchronously inside the `/internal/amqp/consume` request (there is
//! no background-thread pool here, unlike the Python's
//! `threading.Thread` + per-thread event loop in `amqp/consumer.py`).

use chrono::Utc;
use serde_json::{json, Map, Value};

use octy_spin::ctx::Ctx;

use crate::billing::BillingUnits;
use crate::http;
use crate::matching;
use crate::models::AccountData;
use crate::repos;

/// Port of the Python's `amqp_message_size_limit`/`webhook_payload_size_limit`
/// (100MB) safety net, splitting an oversized list into multiple messages
/// rather than sending one that could exceed the broker's or the receiving
/// webhook's message-size limit.
///
/// The Python measured `sys.getsizeof(list)`, which is the size of the list's
/// *pointer array* (roughly `56 + 8*len`), not its serialized contents — for
/// a 100MB threshold that requires ~13 million elements to ever trigger, so
/// in practice the Python check never chunks anything. That's a no-op, not
/// an intentional design choice worth preserving bug-for-bug (unlike the
/// deliberate ordering/business-rule bugs elsewhere in this port): the actual
/// risk this code exists to guard against — an oversized AMQP/webhook
/// payload — is real, so this measures the serialized JSON byte size
/// instead.
const MESSAGE_SIZE_LIMIT_BYTES: usize = 100 * 1024 * 1024;

fn chunk_by_byte_size(items: Vec<Value>) -> Vec<Vec<Value>> {
    if items.is_empty() {
        return vec![items];
    }
    let total_bytes = serde_json::to_vec(&items).map(|v| v.len()).unwrap_or(0);
    if total_bytes < MESSAGE_SIZE_LIMIT_BYTES {
        return vec![items];
    }
    let num_chunks = total_bytes.div_ceil(MESSAGE_SIZE_LIMIT_BYTES).max(1);
    let chunk_len = items.len().div_ceil(num_chunks).max(1);
    items.chunks(chunk_len).map(|c| c.to_vec()).collect()
}

pub struct ProfileIdentificationJob {
    account_id: String,
    webhook_url: String,
    authenticated_id_key: String,
    octy_job_id: String,
    billing: BillingUnits,
    profiles_batch: Vec<Value>,
}

impl ProfileIdentificationJob {
    pub fn new(account_data: AccountData, octy_job_id: String) -> Self {
        let billing = BillingUnits::new(
            &account_data.account_id,
            &account_data.account_type,
            &account_data.account_currency,
            "profile_identification",
        );
        Self {
            account_id: account_data.account_id,
            webhook_url: account_data.webhook_url,
            authenticated_id_key: account_data.authenticated_id_key,
            octy_job_id,
            billing,
            profiles_batch: Vec::new(),
        }
    }

    /// Port of `ProfileIdentification.run()`.
    ///
    /// Returns `Ok(())` on success (→ ack) or `Err(detail)` on any failure
    /// (→ reject, no requeue: `amqp/consumer.py`'s `handle_message` always
    /// called `ack_message(payload, False, False)` on an exception from
    /// `.run()` — retries are the Octy Job Scheduler's job, driven by the
    /// `/v1/internal/jobs/callback` `status: "failed"` call below, not
    /// RabbitMQ requeue).
    ///
    /// Bug-for-bug: on failure, `run()`'s `except` block always performs one
    /// extra `complete_compute_units()` billing capture *and* one full
    /// dispose round (job-failure callback + another billing capture). When
    /// the failure came from the `< 3 profiles` guard in `merge_profiles`
    /// (which disposes once *itself* before returning `Err`), this yields
    /// **two** job-failure callbacks and **three** billing captures for that
    /// one job — an accident of the Python's nested try/except, preserved
    /// here intentionally per the port brief.
    pub async fn run(mut self, ctx: &Ctx) -> Result<(), String> {
        self.billing.track_compute_units("hours");

        let result = async {
            self.merge_profiles(ctx).await?;
            self.complete_job(ctx).await
        }
        .await;

        match result {
            Ok(()) => {
                self.billing.complete_compute_units(ctx).await;
                Ok(())
            }
            Err(err) => {
                eprintln!("[profile-identification-worker] job failed: {err}");
                self.billing.complete_compute_units(ctx).await;
                self.dispose_job(ctx, &err).await;
                Err(err)
            }
        }
    }

    /// Port of `_dispose_job`: report failure to the job service, then
    /// capture billing units. The Python's two exception-branch bodies
    /// (POST failed vs. POST succeeded) both boil down to "capture units",
    /// so they collapse to a single call here; we don't surface the
    /// sub-exception detail downstream either way.
    async fn dispose_job(&mut self, ctx: &Ctx, ex: &str) {
        let job_service_url = ctx.config.get_str("OCTY_JOB_SERVICE_CLUSTER_IP").unwrap_or("");
        let message = format!("Profile identification job failed. EX :: {ex}");
        if let Err(err) =
            http::post_job_callback(job_service_url, &self.account_id, &self.octy_job_id, &message, "failed").await
        {
            eprintln!("[profile-identification-worker] job-failure callback failed: {err}");
        }
        self.billing.complete_compute_units(ctx).await;
    }

    /// Port of `_merge_profiles`.
    async fn merge_profiles(&mut self, ctx: &Ctx) -> Result<(), String> {
        let profile_service_url = ctx
            .config
            .get_str("PROFILE_SERVICE_CLUSTER_IP")
            .map_err(|e| e.to_string())?
            .to_string();

        // _build_profiles_df: active + churned profiles, filtered to those
        // with `profile_data[authenticated_id_key]` truthy.
        let mut profiles = http::get_profiles(&profile_service_url, &self.account_id, "active")
            .await
            .map_err(|e| e.to_string())?;
        let churned = http::get_profiles(&profile_service_url, &self.account_id, "churned")
            .await
            .map_err(|e| e.to_string())?;
        profiles.extend(churned);

        let auth_key = self.authenticated_id_key.clone();
        profiles.retain(|p| {
            matching::is_truthy(p.get("profile_data").and_then(|pd| pd.get(&auth_key)))
        });

        if profiles.len() < 3 {
            let ex = format!(
                "Less than three profiles were found with the set authenticated_id_key : {} set in their profile_data attribute.",
                self.authenticated_id_key
            );
            // `_build_profiles_df` disposes itself before raising; `run()`'s
            // outer handler disposes *again* on top of the `Err` this
            // returns (see the doc comment on `run`).
            self.dispose_job(ctx, &ex).await;
            return Err(ex);
        }

        let now = Utc::now();
        let merge_groups = matching::build_merge_groups(&profiles, &self.authenticated_id_key, now);
        let key_types = repos::get_profile_key_types(ctx, &self.account_id).map_err(|e| e.to_string())?;

        let find_profile = |id: &str| profiles.iter().find(|p| p.get("profile_id").and_then(Value::as_str) == Some(id));

        let mut profiles_message: Vec<Value> = Vec::new();
        let mut profiles_delete_message: Vec<Value> = Vec::new();
        let mut rec_cache_delete_message: Vec<Value> = Vec::new();
        let mut past_segment_profiles_message: Vec<Value> = Vec::new();
        let mut event_instance_profiles_message: Vec<Value> = Vec::new();
        let mut profiles_batch: Vec<Value> = Vec::new();

        for group in &merge_groups {
            let parent = find_profile(&group.parent_profile_id)
                .ok_or_else(|| format!("parent profile {} missing after grouping", group.parent_profile_id))?
                .clone();
            let mut children: Vec<&Value> = Vec::with_capacity(group.child_profile_ids.len());
            for child_id in &group.child_profile_ids {
                let child = find_profile(child_id)
                    .ok_or_else(|| format!("child profile {child_id} missing after grouping"))?;
                children.push(child);
            }

            // _merge_segment_tags + _parent_profiles_df_to_formatted_json
            let merged_tags = matching::merge_segment_tags(&parent, &children);
            let formatted = matching::format_parent_profile(&parent, merged_tags, &key_types);
            profiles_message.push(formatted);

            for child in &children {
                let child_id = child.get("profile_id").and_then(Value::as_str).unwrap_or_default();
                profiles_delete_message.push(json!(child_id));
                rec_cache_delete_message.push(json!(child_id));
            }

            let group_dict = json!({
                "parent_profile": group.parent_profile_id,
                "child_profiles": group.child_profile_ids,
            });
            past_segment_profiles_message.push(group_dict.clone());
            event_instance_profiles_message.push(group_dict);

            // _generate_profiles_batch
            let parent_customer_id = parent.get("customer_id").cloned().unwrap_or(Value::Null);
            let authenticated_id_value = parent
                .get("profile_data")
                .and_then(|pd| pd.get(&self.authenticated_id_key))
                .cloned()
                .unwrap_or(Value::Null);
            let merged_profiles: Vec<Value> = children
                .iter()
                .map(|c| {
                    json!({
                        "profile_id": c.get("profile_id").cloned().unwrap_or(Value::Null),
                        "customer_id": c.get("customer_id").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect();
            profiles_batch.push(json!({
                "account_id": self.account_id,
                "authenticated_id_key": self.authenticated_id_key,
                "parent_customer_id": parent_customer_id,
                "authenticated_id_value": authenticated_id_value,
                "merged_profiles": merged_profiles,
                "parent_profile_id": group.parent_profile_id,
            }));
        }

        // _process_amqp_messages: publish all five message types
        // unconditionally, even when a type's message list is empty — the
        // Python's `for mes in self.amqp_messages` loop published every
        // entry regardless of contents; preserved bug-for-bug rather than
        // skipping empty publishes.
        //
        // Split any oversized list across multiple AMQP messages so a single
        // publish never exceeds MESSAGE_SIZE_LIMIT_BYTES — see
        // chunk_by_byte_size's doc comment for why this measures serialized
        // bytes rather than porting the Python's non-functional list-size
        // check literally.
        let publishes: [(&str, Vec<Value>); 5] = [
            ("events.cmd.update", event_instance_profiles_message),
            ("reccache.cmd.delete", rec_cache_delete_message),
            ("profiles.cmd.update", profiles_message),
            ("profiles.cmd.delete", profiles_delete_message),
            ("segment.profiles.cmd.update", past_segment_profiles_message),
        ];
        for (routing_key, messages) in publishes {
            for chunk in chunk_by_byte_size(messages) {
                let payload = json!({ "account_id": self.account_id, "profiles": chunk });
                ctx.gateway
                    .amqp_publish(routing_key, &payload)
                    .await
                    .map_err(|e| format!("failed to publish {routing_key}: {e}"))?;
            }
        }

        // create_merged_profiles_ref
        repos::create_merged_profiles_ref(ctx, &profiles_batch)
            .await
            .map_err(|e| e.to_string())?;
        self.profiles_batch = profiles_batch;

        Ok(())
    }

    /// Port of `_complete_job`. The webhook call is best-effort (the Python
    /// caught and logged every exception there); the job-service success
    /// callback is not — a failure there propagates to `run()`'s failure
    /// path exactly like the Python (whose `_send_http_request` call in
    /// `_complete_job` was unguarded), even though the merge itself
    /// succeeded.
    async fn complete_job(&mut self, ctx: &Ctx) -> Result<(), String> {
        let dropped_account_profiles: Vec<Value> = self
            .profiles_batch
            .iter()
            .map(|p| {
                let mut m: Map<String, Value> = p.as_object().cloned().unwrap_or_default();
                m.remove("account_id");
                Value::Object(m)
            })
            .collect();

        // Split an oversized profile batch across multiple webhook requests
        // — see chunk_by_byte_size's doc comment.
        for chunk in chunk_by_byte_size(dropped_account_profiles) {
            let payload = json!({
                "subject": "Profile identification service output",
                "body": { "profiles": chunk },
                "date_time": Utc::now().to_rfc3339(),
            });
            http::post_webhook_best_effort(&self.webhook_url, &payload).await;
        }

        let job_service_url = ctx.config.get_str("OCTY_JOB_SERVICE_CLUSTER_IP").unwrap_or("");
        http::post_job_callback(
            job_service_url,
            &self.account_id,
            &self.octy_job_id,
            "Profile identification job completed successfully",
            "success",
        )
        .await
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_list_is_not_chunked() {
        let items: Vec<Value> = (0..10).map(|i| json!({ "profile_id": i })).collect();
        let chunks = chunk_by_byte_size(items.clone());
        assert_eq!(chunks, vec![items]);
    }

    #[test]
    fn empty_list_is_not_chunked() {
        assert_eq!(chunk_by_byte_size(vec![]), vec![Vec::<Value>::new()]);
    }

    #[test]
    fn oversized_list_splits_into_multiple_chunks_preserving_all_items() {
        // Each element serializes to well over 100 bytes; use a tiny
        // effective limit by inflating element count far past what a real
        // 100MB payload would need, so the test runs fast while still
        // exercising the split path.
        let big_string = "x".repeat(1000);
        let items: Vec<Value> = (0..200_000)
            .map(|i| json!({ "profile_id": i, "padding": big_string }))
            .collect();
        let total_bytes = serde_json::to_vec(&items).unwrap().len();
        assert!(total_bytes > MESSAGE_SIZE_LIMIT_BYTES, "test setup should exceed the limit");

        let chunks = chunk_by_byte_size(items.clone());
        assert!(chunks.len() > 1, "expected the oversized list to be split");

        let rejoined: Vec<Value> = chunks.into_iter().flatten().collect();
        assert_eq!(rejoined, items, "chunking must not drop or reorder items");
    }
}
