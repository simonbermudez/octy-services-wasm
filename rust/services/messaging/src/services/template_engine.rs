//! Port of `services/template_engine.py` — `TemplateEngine`, `ItemRec`,
//! `RewardCard`.
//!
//! The Python used Python's `str.format(**kwargs)` against
//! `{{tag}}` → `{tag}` collapsed placeholders (via `stuf`, an attribute-style
//! dict, when `item_rec.*` dotted access was needed). Rust has no equivalent
//! of `str.format`, so [`format_content`] implements the same substitution:
//! replace every `{name}` (after `{{`/`}}` collapsing) with `str(values[name])`,
//! and `{item_rec.attr}` with the nested `item_rec` map's `attr`.

use std::collections::{BTreeMap, HashMap};

use octy_shared::errors::OctyError;
use serde_json::{Map, Value};

use octy_spin::auth::AuthAccount;
use octy_spin::ctx::Ctx;

use crate::currency::item_price_format;
use crate::http_util::MsgError;
use crate::models::{extract_placeholder_tags, GenerateContent, GenerateContentChild};
use crate::repos::{content as content_repo, reward_cards as reward_cards_repo, templates as templates_repo};

#[derive(Debug, Clone)]
struct RequiredData {
    template_id: String,
    required_data: Vec<String>,
}

#[derive(Debug, Clone)]
struct ProfileItem {
    profile_id: String,
    item: Value,
}

#[derive(Debug, Clone)]
struct RybbonClaim {
    request_id: String,
    campaign_key: String,
    value: String,
    active: bool,
    exceeded: bool,
}

pub struct TemplateEngine<'a> {
    ctx: &'a Ctx,
    account: &'a AuthAccount,

    all_templates: Vec<Value>,
    working_templates: Vec<Value>,
    templates_required_data: Vec<RequiredData>,

    currency_rates: Option<Value>,
    item_recommendations: Vec<Value>,
    items: Vec<Value>,
    profile_item_map: Vec<ProfileItem>,

    rybbon_campaigns: Vec<Value>,
    customer_rybbon_campaign_map: Vec<Vec<RybbonClaim>>,
    rybbon_rewards: Vec<Value>,

    failed_template_ids: Vec<String>,

    pub created_messages: Vec<Value>,
    pub failed_messages: Vec<Value>,
    pub failed_templates: Vec<Value>,
}

impl<'a> TemplateEngine<'a> {
    pub fn new(ctx: &'a Ctx, account: &'a AuthAccount) -> Self {
        Self {
            ctx,
            account,
            all_templates: Vec::new(),
            working_templates: Vec::new(),
            templates_required_data: Vec::new(),
            currency_rates: None,
            item_recommendations: Vec::new(),
            items: Vec::new(),
            profile_item_map: Vec::new(),
            rybbon_campaigns: Vec::new(),
            customer_rybbon_campaign_map: Vec::new(),
            rybbon_rewards: Vec::new(),
            failed_template_ids: Vec::new(),
            created_messages: Vec::new(),
            failed_messages: Vec::new(),
            failed_templates: Vec::new(),
        }
    }

    fn handle_template_err(&mut self, template_id: &str, err_msg: &str) {
        self.failed_template_ids.push(template_id.to_string());
        self.failed_templates.push(serde_json::json!({
            "template_id": template_id,
            "error_message": err_msg,
        }));
    }

    async fn get_all_templates(&mut self) -> Result<(), MsgError> {
        let account_id = crate::services::messaging::account_id_str(self.account);
        self.all_templates = templates_repo::get_all_templates(self.ctx, &account_id)
            .await
            .map_err(MsgError::Octy)?;
        if self.all_templates.is_empty() {
            return Err(MsgError::Octy(OctyError::new(
                400,
                "Template resource not found",
                vec![octy_shared::errors::ErrorReason::new(
                    "No templates exist for this account",
                    self.ctx.config.opt_str("MESSAGING_EXTENDED_HELP").unwrap_or(""),
                )],
            )));
        }
        Ok(())
    }

    fn verify_template_exist(&mut self, template_id: &str) {
        let filtered: Vec<Value> = self
            .all_templates
            .iter()
            .filter(|t| t["_id"].as_str() == Some(template_id) || t.get("template_id").and_then(Value::as_str) == Some(template_id))
            .cloned()
            .collect();
        if filtered.is_empty() {
            self.handle_template_err(
                template_id,
                "No template found with this template_id. All messages using this template_id were not created.",
            );
            return;
        }
        self.working_templates.push(filtered[0].clone());
    }

    fn identify_required_data(&self, template_id: &str, content: &str) -> RequiredData {
        RequiredData {
            template_id: template_id.to_string(),
            required_data: extract_placeholder_tags(content),
        }
    }

    /// `_parse_group_profile_ids`
    fn parse_group_profile_ids(&self, messages: &[GenerateContentChild]) -> Vec<String> {
        let mut profile_ids: Vec<String> = Vec::new();
        for tr in &self.templates_required_data {
            for r in &tr.required_data {
                for m in messages {
                    for d in &m.data {
                        let Some(raw) = d.get(r) else { continue };
                        let parsed = if r.contains("item_rec") {
                            if r.contains("item_price") {
                                raw.as_str().and_then(|s| s.split("::").next()).map(String::from)
                            } else {
                                raw.as_str().map(String::from)
                            }
                        } else {
                            None
                        };
                        if let Some(p) = parsed {
                            if !profile_ids.contains(&p) {
                                profile_ids.push(p);
                            }
                        }
                    }
                }
            }
        }
        profile_ids
    }

    async fn get_currency_rates(&mut self) -> Result<(), OctyError> {
        if self
            .templates_required_data
            .iter()
            .any(|r| r.required_data.iter().any(|rd| rd.contains("item_price")))
        {
            self.currency_rates = Some(content_repo::get_currency_rates(self.ctx).await?);
        }
        Ok(())
    }

    async fn get_recommendations(&mut self, profile_ids: &[String]) -> Result<(), OctyError> {
        let account_id = crate::services::messaging::account_id_str(self.account);
        self.item_recommendations =
            content_repo::get_item_recommendations(self.ctx, &account_id, profile_ids).await?;
        Ok(())
    }

    async fn get_items(&mut self) -> Result<(), OctyError> {
        let account_id = crate::services::messaging::account_id_str(self.account);
        self.items = content_repo::get_items(self.ctx, &account_id).await?;
        Ok(())
    }

    fn build_profile_item_map(&mut self) {
        for rec in &self.item_recommendations {
            let recommendations = rec.get("recommendations").and_then(Value::as_array);
            let Some(recommendations) = recommendations else { continue };
            if recommendations.is_empty() {
                continue;
            }
            let Some(profile_id) = rec.get("profile_id").and_then(Value::as_str) else { continue };
            let Some(top_item) = recommendations[0].get("item_id").and_then(Value::as_str) else { continue };
            if let Some(item) = self
                .items
                .iter()
                .find(|i| i.get("item_id").and_then(Value::as_str) == Some(top_item))
            {
                self.profile_item_map.push(ProfileItem {
                    profile_id: profile_id.to_string(),
                    item: item.clone(),
                });
            }
        }
    }

    async fn get_rybbon_campaigns(&mut self) -> Result<String, OctyError> {
        let token = reward_cards_repo::auth(self.ctx).await?;
        let campaigns = reward_cards_repo::get_campaigns(self.ctx, &token).await?;
        self.rybbon_campaigns.extend(campaigns);
        Ok(token)
    }

    /// `_build_customer_rybbon_campaign_map` — parse `rybbon_reward_card`
    /// values, group by `campaignKey` (Python used `sorted` + `groupby`,
    /// which groups by *consecutive* runs of the same key after sorting —
    /// equivalent here to grouping by key directly since the sort makes runs
    /// contiguous).
    fn build_customer_rybbon_campaign_map(&mut self, messages: &[GenerateContentChild]) -> Result<(), MsgError> {
        let mut flat: Vec<RybbonClaim> = Vec::new();
        for tr in &self.templates_required_data {
            for r in &tr.required_data {
                if !r.contains("rybbon_reward_card") {
                    continue;
                }
                for m in messages {
                    for d in &m.data {
                        let Some(raw) = d.get(r) else { continue };
                        let value = raw.as_str().ok_or_else(|| {
                            MsgError::internal("AttributeError: 'NoneType' object has no attribute 'split'")
                        })?;
                        let params: Vec<&str> = value.split("::").collect();
                        flat.push(RybbonClaim {
                            request_id: params.first().copied().unwrap_or_default().to_string(),
                            campaign_key: params.get(1).copied().unwrap_or_default().to_string(),
                            value: params.get(2).copied().unwrap_or_default().to_string(),
                            active: false,
                            exceeded: false,
                        });
                    }
                }
            }
        }
        flat.sort_by(|a, b| a.campaign_key.cmp(&b.campaign_key));
        let mut groups: BTreeMap<String, Vec<RybbonClaim>> = BTreeMap::new();
        for claim in flat {
            groups.entry(claim.campaign_key.clone()).or_default().push(claim);
        }
        self.customer_rybbon_campaign_map = groups.into_values().collect();
        Ok(())
    }

    fn assess_campaign_limit(&mut self) {
        for group in &mut self.customer_rybbon_campaign_map {
            let Some(group_key) = group.first().map(|c| c.campaign_key.clone()) else { continue };
            let campaign = self
                .rybbon_campaigns
                .iter()
                .find(|c| c.get("campaignKey").and_then(Value::as_str) == Some(group_key.as_str()));
            if let Some(campaign) = campaign {
                for claim in group.iter_mut() {
                    claim.active = true;
                }
                let available = campaign.get("availableRewards").and_then(Value::as_i64).unwrap_or(0);
                if group.len() as i64 > available {
                    // Python set `c['exceeded'] = True` on the campaign dict,
                    // never on individual claims — meaning `_filter_valid_claims`
                    // (which reads `x['exceeded']`, always False as set here)
                    // never actually filters exceeded claims. Preserved: no
                    // per-claim `exceeded` flag is set.
                }
            }
        }
    }

    async fn get_reward_claims(&mut self, auth_token: &str) -> Result<(), OctyError> {
        let claim_groups: Vec<Vec<Value>> = self
            .customer_rybbon_campaign_map
            .iter()
            .map(|group| {
                group
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "requestId": c.request_id,
                            "campaignKey": c.campaign_key,
                            "value": c.value,
                            "active": c.active,
                            "exceeded": c.exceeded,
                        })
                    })
                    .collect()
            })
            .collect();
        self.rybbon_rewards = reward_cards_repo::claim_rewards(self.ctx, auth_token, &claim_groups).await?;
        Ok(())
    }

    /// `_format_template_placeholder_tags`
    fn format_template_placeholder_tags(template: &mut Value) {
        if let Some(content) = template.get("content").and_then(Value::as_str) {
            let collapsed = content.replace("{{", "{").replace("}}", "}");
            template["content"] = Value::String(collapsed);
        }
    }

    async fn is_item_rec(&mut self, messages: &[GenerateContentChild]) -> Result<(), OctyError> {
        if self
            .templates_required_data
            .iter()
            .any(|r| r.required_data.iter().any(|rd| rd.contains("item_rec")))
        {
            let profile_ids = self.parse_group_profile_ids(messages);
            self.get_recommendations(&profile_ids).await?;
            self.get_items().await?;
            // Only worth the round trip to tbl_currency_rates if there are
            // items to price.
            if !self.items.is_empty() {
                self.get_currency_rates().await?;
            }
            self.build_profile_item_map();
        }
        Ok(())
    }

    async fn is_reward_card(&mut self, messages: &[GenerateContentChild]) -> Result<(), MsgError> {
        if self
            .templates_required_data
            .iter()
            .any(|r| r.required_data.iter().any(|rd| rd.contains("rybbon_reward_card")))
        {
            let token = self.get_rybbon_campaigns().await.map_err(MsgError::Octy)?;
            self.build_customer_rybbon_campaign_map(messages)?;
            self.assess_campaign_limit();
            self.get_reward_claims(&token).await.map_err(MsgError::Octy)?;
        }
        Ok(())
    }

    /// `_generate` — build one message per data object for `message`/`template`.
    /// Takes `&self` (not `&mut self`) and returns the created/failed
    /// entries so the caller can merge them into `self.*` without holding an
    /// immutable borrow (of `profile_item_map` / `rybbon_rewards` /
    /// `currency_rates`) across a mutable one.
    async fn generate_one(
        &self,
        message: &GenerateContentChild,
        template: Value,
    ) -> Result<(Vec<Value>, Vec<Value>), MsgError> {
        let required_data = self
            .templates_required_data
            .iter()
            .find(|k| k.template_id == message.template_id)
            .map(|k| k.required_data.clone())
            .unwrap_or_default();

        let mut created: Vec<Value> = Vec::new();
        let mut failed: Vec<Value> = Vec::new();

        for data in &message.data {
            let mut values_dict: HashMap<String, String> = HashMap::new();
            let mut item_rec_dict: HashMap<String, String> = HashMap::new();
            let mut message_failed = false;

            let item_rec = ItemRecCtx::new(data, &template, &required_data, &self.profile_item_map);
            let reward_card_tag = self
                .ctx
                .config
                .opt_str("REWARD_CARD_PLACEHOLDER_TAG")
                .unwrap_or("rybbon_reward_card");
            let reward_card =
                RewardCardCtx::new(data, &template, &required_data, &self.rybbon_rewards, reward_card_tag);

            for key in &required_data {
                let Some(value) = data.get(key) else {
                    failed.push(json_missing_data_error(&message.template_id, data, key));
                    message_failed = true;
                    break;
                };

                if key.contains("item_rec") {
                    // Placeholder keys are `item_rec.<item attribute>` (e.g.
                    // `item_rec.item_price`); `attr` is the part after the
                    // dot, used to look it up on the recommended item.
                    let attr = key.splitn(2, '.').nth(1).unwrap_or_default();
                    let populated = item_rec
                        .populate_value(key, attr, value, self.currency_rates.as_ref())
                        .await?;
                    item_rec_dict.insert(attr.to_string(), populated);
                    continue;
                }
                if key.contains("rybbon_reward_card") {
                    let populated = reward_card.populate_value(key)?;
                    values_dict.insert("rybbon_reward_card".to_string(), populated);
                    continue;
                }

                let is_empty = matches!(value, Value::Null) || value.as_str() == Some("");
                if is_empty {
                    let default = template["default_values"].get(key).cloned().unwrap_or(Value::Null);
                    values_dict.insert(key.clone(), crate::models::py_str(&default));
                } else {
                    values_dict.insert(key.clone(), crate::models::py_str(value));
                }
            }

            if message_failed {
                continue;
            }

            let mut template_fmt = template.clone();
            Self::format_template_placeholder_tags(&mut template_fmt);
            let content_template = template_fmt["content"].as_str().unwrap_or_default();
            let content = format_content(content_template, &values_dict, &item_rec_dict, item_rec.has_rec)?;

            created.push(serde_json::json!({
                "template_id": message.template_id,
                "friendly_name": template_fmt["friendly_name"],
                "title": template_fmt["title"],
                "content": content,
            }));
        }
        Ok((created, failed))
    }

    pub async fn generate(&mut self, messages: &GenerateContent) -> Result<(), MsgError> {
        self.get_all_templates().await?;
        for message in &messages.messages {
            self.verify_template_exist(&message.template_id);
        }

        for t in self.working_templates.clone() {
            let template_id = t.get("template_id").and_then(Value::as_str).unwrap_or_default();
            let content = t.get("content").and_then(Value::as_str).unwrap_or_default();
            let required = self.identify_required_data(template_id, content);
            self.templates_required_data.push(required);
        }

        self.is_item_rec(&messages.messages).await.map_err(MsgError::Octy)?;
        self.is_reward_card(&messages.messages).await?;

        let working_templates = self.working_templates.clone();
        for template in &working_templates {
            let template_id = template.get("template_id").and_then(Value::as_str).unwrap_or_default();
            for message in &messages.messages {
                if self.failed_template_ids.iter().any(|f| f == &message.template_id) {
                    continue;
                }
                if template_id == message.template_id {
                    let (created, failed) = self.generate_one(message, template.clone()).await?;
                    self.created_messages.extend(created);
                    self.failed_messages.extend(failed);
                }
            }
        }

        Ok(())
    }
}

fn json_missing_data_error(template_id: &str, provided_data: &Map<String, Value>, key: &str) -> Value {
    serde_json::json!({
        "template_id": template_id,
        "provided_data": provided_data,
        "error_message": format!("Missing required data parameter for this message: '{key}'"),
    })
}

/// `content.format(**values)` (or `**stuf(values)` when `item_rec` nested
/// access is needed) — replace `{name}` / `{item_rec.attr}` placeholders.
/// Unknown keys raise `KeyError` in Python → surfaced as a 500 here (should
/// not occur: `required_data` extraction guarantees every placeholder has a
/// matching value).
fn format_content(
    content: &str,
    values: &HashMap<String, String>,
    item_rec: &HashMap<String, String>,
    has_rec: bool,
) -> Result<String, MsgError> {
    let mut out = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                if let Some(end) = content[i..].find('}') {
                    let name = &content[i + 1..i + end];
                    let resolved = if let Some(attr) = name.strip_prefix("item_rec.") {
                        if !has_rec {
                            return Err(MsgError::internal(format!("KeyError: '{name}'")));
                        }
                        item_rec
                            .get(attr)
                            .cloned()
                            .ok_or_else(|| MsgError::internal(format!("KeyError: '{attr}'")))?
                    } else {
                        values
                            .get(name)
                            .cloned()
                            .ok_or_else(|| MsgError::internal(format!("KeyError: '{name}'")))?
                    };
                    out.push_str(&resolved);
                    i += end + 1;
                } else {
                    out.push('{');
                    i += 1;
                }
            }
            _ => {
                let ch = content[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    Ok(out)
}

/// Port of the `ItemRec` helper class.
struct ItemRecCtx<'a> {
    template: &'a Value,
    item: Option<&'a ProfileItem>,
    has_rec: bool,
}

impl<'a> ItemRecCtx<'a> {
    fn new(
        data: &Map<String, Value>,
        template: &'a Value,
        required_data: &[String],
        profile_item_map: &'a [ProfileItem],
    ) -> Self {
        let has_rec = required_data.iter().any(|rd| rd.contains("item_rec"));
        let item = if has_rec {
            required_data
                .iter()
                .find(|t| t.contains("item_rec"))
                .and_then(|first_param| data.get(first_param))
                .and_then(Value::as_str)
                .and_then(|profile_id| profile_item_map.iter().find(|p| p.profile_id == profile_id))
        } else {
            None
        };
        Self { template, item, has_rec }
    }

    async fn populate_value(
        &self,
        key: &str,
        attr: &str,
        raw_value: &Value,
        currency_rates: Option<&Value>,
    ) -> Result<String, MsgError> {
        if let Some(item) = self.item {
            let item_value = item
                .item
                .get(attr)
                .cloned()
                .ok_or_else(|| MsgError::internal(format!("KeyError: '{attr}'")))?;
            if key.contains("item_price") {
                let params = raw_value
                    .as_str()
                    .ok_or_else(|| MsgError::internal("AttributeError: item_price value must be a string"))?;
                let amount = item_value
                    .as_f64()
                    .or_else(|| item_value.as_i64().map(|n| n as f64))
                    .ok_or_else(|| MsgError::internal("item_price attribute is not numeric"))?;
                return item_price_format(params, amount, currency_rates);
            }
            Ok(crate::models::py_str(&item_value))
        } else {
            let default = self.template["default_values"].get(key).cloned().unwrap_or(Value::Null);
            Ok(crate::models::py_str(&default))
        }
    }
}

/// Port of the `RewardCard` helper class.
struct RewardCardCtx<'a> {
    template: &'a Value,
    reward: Option<&'a Value>,
}

impl<'a> RewardCardCtx<'a> {
    fn new(
        data: &Map<String, Value>,
        template: &'a Value,
        required_data: &[String],
        rybbon_rewards: &'a [Value],
        reward_card_tag: &str,
    ) -> Self {
        let has_rc = required_data.iter().any(|rd| rd.contains(reward_card_tag));
        let reward = if has_rc {
            data.get("rybbon_reward_card")
                .and_then(Value::as_str)
                .and_then(|v| v.split("::").next())
                .and_then(|customer_id| {
                    rybbon_rewards
                        .iter()
                        .find(|r| r.get("requestId").and_then(Value::as_str) == Some(customer_id))
                })
        } else {
            None
        };
        Self { template, reward }
    }

    fn populate_value(&self, key: &str) -> Result<String, MsgError> {
        if let Some(reward) = self.reward {
            let snippet = reward
                .get("htmlSnippet")
                .cloned()
                .ok_or_else(|| MsgError::internal("KeyError: 'htmlSnippet'"))?;
            Ok(crate::models::py_str(&snippet))
        } else {
            let default = self.template["default_values"].get(key).cloned().unwrap_or(Value::Null);
            Ok(crate::models::py_str(&default))
        }
    }
}
