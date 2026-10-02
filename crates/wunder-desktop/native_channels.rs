use super::NativeDesktop;
use anyhow::{anyhow, Result};
use serde_json::Value;
use wunder_server::api::user_channel_logs::{
    list_user_channel_runtime_logs, reconnect_user_channel,
};
use wunder_server::api::user_channels::{
    build_weixin_qr_png_data_uri, delete_user_channel_account, delete_user_channel_binding,
    list_user_channel_accounts, list_user_channel_bindings, start_user_weixin_qr_login,
    upsert_user_channel_account, upsert_user_channel_binding, wait_user_weixin_qr_login,
    ChannelAccountUpsertRequest, ChannelBindingUpsertRequest, ChannelServiceError,
    FeishuAccountPayload, WechatAccountPayload, WechatMpAccountPayload, WeixinAccountPayload,
    WeixinQrStartRequest, WeixinQrWaitRequest,
};

/// Secret-free projection for the desktop channel page. Secrets never cross
/// this boundary: only `secret_set` flags and non-secret guided prefill values.
#[derive(Clone, Debug)]
pub struct ChannelAccountCard {
    pub channel: String,
    pub account_id: String,
    pub name: String,
    pub status: String,
    pub active: bool,
    pub configured: bool,
    pub peer_kind: String,
    pub agent_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub summary: String,
    pub app_id: String,
    pub corp_id: String,
    pub wechat_agent_id: String,
    pub original_id: String,
    pub weixin_bot_id: String,
    pub weixin_user_id: String,
    pub bot_type: String,
    pub domain: String,
    pub secret_set: bool,
}

#[derive(Clone, Debug)]
pub struct ChannelCatalogItem {
    pub channel: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, Default)]
pub struct ChannelAccountListing {
    pub items: Vec<ChannelAccountCard>,
    pub catalog: Vec<ChannelCatalogItem>,
}

/// Typed secret-free edit input. Empty secret fields keep the stored value;
/// the runtime service performs the same validation as the web API.
#[derive(Clone, Debug, Default)]
pub struct NativeChannelAccountEdit {
    /// Empty to create a new account.
    pub account_id: String,
    pub channel: String,
    pub account_name: String,
    pub agent_id: String,
    pub enabled: bool,
    /// Feishu only: receive group chats instead of direct messages.
    pub receive_group_chat: Option<bool>,
    pub app_id: String,
    pub app_secret: String,
    pub corp_id: String,
    pub wechat_agent_id: String,
    pub wechat_secret: String,
    pub wechat_token: String,
    pub wechat_aes_key: String,
    pub wechat_mp_token: String,
    pub wechat_mp_aes_key: String,
    pub wechat_mp_original_id: String,
    pub weixin_api_base: String,
    pub weixin_bot_token: String,
    pub weixin_bot_id: String,
    pub weixin_user_id: String,
    pub domain: String,
    /// Advanced JSON merged into the channel config; the only path for
    /// channels without a guided form (qqbot, xmpp, whatsapp, ...).
    pub config_json: String,
}

#[derive(Clone, Debug)]
pub struct ChannelBindingCard {
    pub binding_id: String,
    pub channel: String,
    pub account_id: String,
    pub peer_kind: String,
    pub peer_id: String,
    pub agent_id: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default)]
pub struct NativeChannelBindingEdit {
    pub channel: String,
    pub account_id: String,
    pub peer_kind: String,
    pub peer_id: String,
    pub agent_id: String,
    pub enabled: bool,
}

/// One weixin QR login session opened through the shared runtime service. The
/// QR is rendered locally from the raw qrcode text, never fetched remotely.
#[derive(Clone, Debug)]
pub struct WeixinQrLoginStart {
    pub session_key: String,
    pub qrcode: String,
    pub qrcode_open_url: String,
    pub png_data_uri: String,
}

/// Result of one short wait window; the façade polls it until connected,
/// expired or the user cancels.
#[derive(Clone, Debug, Default)]
pub struct WeixinQrLoginStatus {
    pub connected: bool,
    pub status: String,
    pub message: String,
    pub bot_token: String,
    pub ilink_bot_id: String,
    pub ilink_user_id: String,
    pub api_base: String,
}

#[derive(Clone, Debug)]
pub struct ChannelLogEntry {
    pub time: String,
    pub level: String,
    pub channel: String,
    pub account_id: String,
    pub event: String,
    pub message: String,
}

fn channel_error(error: ChannelServiceError) -> anyhow::Error {
    anyhow!("{}", error.message)
}

fn trim_opt(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn text_at(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

impl NativeDesktop {
    pub(crate) fn channel_user(&self) -> Result<wunder_server::storage::UserAccountRecord> {
        self.state()
            .user_store
            .get_user_by_id(self.user_id())?
            .ok_or_else(|| anyhow!("本地用户不存在"))
    }

    pub fn list_channel_accounts(&self) -> Result<ChannelAccountListing> {
        let data = self
            .runtime
            .block_on(list_user_channel_accounts(
                self.state(),
                self.user_id(),
                None,
            ))
            .map_err(channel_error)?;
        let mut listing = ChannelAccountListing::default();
        if let Some(supported) = data.get("supported_channels").and_then(Value::as_array) {
            for item in supported {
                listing.catalog.push(ChannelCatalogItem {
                    channel: text_at(item, "channel"),
                    name: text_at(item, "name"),
                    description: text_at(item, "description"),
                });
            }
        }
        if let Some(items) = data.get("items").and_then(Value::as_array) {
            for item in items {
                listing.items.push(account_card_from_item(item));
            }
        }
        Ok(listing)
    }

    pub fn save_channel_account(&self, edit: &NativeChannelAccountEdit) -> Result<ChannelAccountCard> {
        let channel = edit.channel.trim().to_ascii_lowercase();
        if channel.is_empty() {
            return Err(anyhow!("请选择渠道类型"));
        }
        let user = self.channel_user()?;
        let config = config_json_opt(&edit.config_json)?;
        let request = ChannelAccountUpsertRequest {
            channel: channel.clone(),
            account_id: trim_opt(&edit.account_id),
            create_new: None,
            agent_id: trim_opt(&edit.agent_id),
            account_name: trim_opt(&edit.account_name),
            app_id: None,
            app_secret: None,
            receive_group_chat: edit.receive_group_chat,
            enabled: Some(edit.enabled),
            domain: None,
            peer_kind: None,
            config,
            feishu: (channel == "feishu").then(|| FeishuAccountPayload {
                app_id: trim_opt(&edit.app_id),
                app_secret: trim_opt(&edit.app_secret),
                domain: trim_opt(&edit.domain),
            }),
            wechat: (channel == "wechat").then(|| WechatAccountPayload {
                corp_id: trim_opt(&edit.corp_id),
                agent_id: trim_opt(&edit.wechat_agent_id),
                secret: trim_opt(&edit.wechat_secret),
                token: trim_opt(&edit.wechat_token),
                encoding_aes_key: trim_opt(&edit.wechat_aes_key),
                domain: trim_opt(&edit.domain),
            }),
            wechat_mp: (channel == "wechat_mp").then(|| WechatMpAccountPayload {
                app_id: trim_opt(&edit.app_id),
                app_secret: trim_opt(&edit.app_secret),
                token: trim_opt(&edit.wechat_mp_token),
                encoding_aes_key: trim_opt(&edit.wechat_mp_aes_key),
                original_id: trim_opt(&edit.wechat_mp_original_id),
                domain: trim_opt(&edit.domain),
            }),
            weixin: (channel == "weixin").then(|| WeixinAccountPayload {
                api_base: trim_opt(&edit.weixin_api_base),
                cdn_base: None,
                bot_token: trim_opt(&edit.weixin_bot_token),
                ilink_bot_id: trim_opt(&edit.weixin_bot_id),
                ilink_user_id: trim_opt(&edit.weixin_user_id),
                bot_type: None,
                long_connection_enabled: None,
                poll_timeout_ms: None,
                api_timeout_ms: None,
                max_consecutive_failures: None,
                backoff_ms: None,
                route_tag: None,
                allow_from: None,
            }),
        };
        let item = self
            .runtime
            .block_on(upsert_user_channel_account(
                self.state(),
                &user,
                self.user_id(),
                request,
            ))
            .map_err(channel_error)?;
        Ok(account_card_from_item(&item))
    }

    /// Enabled toggle reuses the account upsert: empty fields keep every stored
    /// value and only the enabled flag changes.
    pub fn toggle_channel_account(&self, channel: &str, account_id: &str, enabled: bool) -> Result<()> {
        let user = self.channel_user()?;
        let request = ChannelAccountUpsertRequest {
            channel: channel.trim().to_ascii_lowercase(),
            account_id: trim_opt(account_id),
            create_new: None,
            agent_id: None,
            account_name: None,
            app_id: None,
            app_secret: None,
            receive_group_chat: None,
            enabled: Some(enabled),
            domain: None,
            peer_kind: None,
            config: None,
            feishu: None,
            wechat: None,
            wechat_mp: None,
            weixin: None,
        };
        self.runtime
            .block_on(upsert_user_channel_account(
                self.state(),
                &user,
                self.user_id(),
                request,
            ))
            .map_err(channel_error)?;
        Ok(())
    }

    pub fn delete_channel_account(&self, channel: &str, account_id: &str) -> Result<()> {
        self.runtime
            .block_on(delete_user_channel_account(
                self.state(),
                self.user_id(),
                channel,
                account_id,
            ))
            .map_err(channel_error)?;
        Ok(())
    }

    pub fn list_channel_bindings(
        &self,
        channel: Option<&str>,
        account_id: Option<&str>,
    ) -> Result<Vec<ChannelBindingCard>> {
        let data = self
            .runtime
            .block_on(list_user_channel_bindings(
                self.state(),
                self.user_id(),
                channel,
                account_id,
                None,
                None,
            ))
            .map_err(channel_error)?;
        let mut bindings = Vec::new();
        if let Some(items) = data.get("items").and_then(Value::as_array) {
            for item in items {
                bindings.push(ChannelBindingCard {
                    binding_id: text_at(item, "binding_id"),
                    channel: text_at(item, "channel"),
                    account_id: text_at(item, "account_id"),
                    peer_kind: text_at(item, "peer_kind"),
                    peer_id: text_at(item, "peer_id"),
                    agent_id: text_at(item, "agent_id"),
                    enabled: item
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
            }
        }
        Ok(bindings)
    }

    pub fn save_channel_binding(&self, edit: &NativeChannelBindingEdit) -> Result<()> {
        let user = self.channel_user()?;
        let request = ChannelBindingUpsertRequest {
            channel: edit.channel.trim().to_string(),
            account_id: edit.account_id.trim().to_string(),
            peer_kind: edit.peer_kind.trim().to_string(),
            peer_id: edit.peer_id.trim().to_string(),
            agent_id: trim_opt(&edit.agent_id),
            tool_overrides: None,
            enabled: Some(edit.enabled),
            priority: None,
        };
        self.runtime
            .block_on(upsert_user_channel_binding(
                self.state(),
                &user,
                self.user_id(),
                request,
            ))
            .map_err(channel_error)?;
        Ok(())
    }

    pub fn delete_channel_binding(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<()> {
        self.runtime
            .block_on(delete_user_channel_binding(
                self.state(),
                self.user_id(),
                channel,
                account_id,
                peer_kind,
                peer_id,
            ))
            .map_err(channel_error)?;
        Ok(())
    }

    /// Opens a weixin QR login session. `account_id` rebinds an existing
    /// account; empty creates a fresh login for a new account.
    pub fn start_weixin_qr_login(&self, account_id: Option<&str>, force: bool) -> Result<WeixinQrLoginStart> {
        let payload = WeixinQrStartRequest {
            account_id: account_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            api_base: None,
            bot_type: None,
            force: Some(force),
        };
        let data = self
            .runtime
            .block_on(start_user_weixin_qr_login(
                self.state(),
                self.user_id(),
                payload,
            ))
            .map_err(channel_error)?;
        let qrcode = text_at(&data, "qrcode");
        Ok(WeixinQrLoginStart {
            png_data_uri: build_weixin_qr_png_data_uri(&qrcode).unwrap_or_default(),
            session_key: text_at(&data, "session_key"),
            qrcode_open_url: text_at(&data, "qrcode_open_url"),
            qrcode,
        })
    }

    /// One short wait window (the façade loops with ~3s timeouts so the UI can
    /// cancel between polls). Error texts distinguish missing/expired sessions.
    pub fn wait_weixin_qr_login(&self, session_key: &str, timeout_ms: u64) -> Result<WeixinQrLoginStatus> {
        let payload = WeixinQrWaitRequest {
            session_key: session_key.trim().to_string(),
            api_base: None,
            timeout_ms: Some(timeout_ms.clamp(1_000, 30_000)),
        };
        let data = self
            .runtime
            .block_on(wait_user_weixin_qr_login(
                self.state(),
                self.user_id(),
                payload,
            ))
            .map_err(channel_error)?;
        Ok(WeixinQrLoginStatus {
            connected: data.get("connected").and_then(Value::as_bool).unwrap_or(false),
            status: text_at(&data, "status"),
            message: text_at(&data, "message"),
            bot_token: text_at(&data, "bot_token"),
            ilink_bot_id: text_at(&data, "ilink_bot_id"),
            ilink_user_id: text_at(&data, "ilink_user_id"),
            api_base: text_at(&data, "api_base"),
        })
    }

    /// Renders qrcode text into a PNG data URI locally; kept on the façade so
    /// contract tests can pin the offline renderer the QR dialog relies on.
    pub fn weixin_qr_png_data_uri(&self, qrcode_text: &str) -> Option<String> {
        build_weixin_qr_png_data_uri(qrcode_text)
    }

    /// Requests a reconnect; currently only XMPP accounts support it because
    /// other channels are long-poll based.
    pub fn reconnect_channel_account(&self, channel: &str, account_id: &str) -> Result<()> {
        self.runtime
            .block_on(reconnect_user_channel(
                self.state(),
                self.user_id(),
                channel,
                account_id,
            ))
            .map_err(channel_error)?;
        Ok(())
    }

    /// Recent runtime log entries for owned accounts, newest first.
    pub fn list_channel_runtime_logs(
        &self,
        channel: Option<&str>,
        account_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ChannelLogEntry>> {
        let data = self
            .runtime
            .block_on(list_user_channel_runtime_logs(
                self.state(),
                self.user_id(),
                channel,
                account_id,
                None,
                Some(limit),
            ))
            .map_err(channel_error)?;
        let mut entries = Vec::new();
        if let Some(items) = data.get("items").and_then(Value::as_array) {
            for item in items {
                let ts = item.get("ts").and_then(Value::as_f64).unwrap_or_default();
                entries.push(ChannelLogEntry {
                    time: format_ts(ts),
                    level: text_at(item, "level"),
                    channel: text_at(item, "channel"),
                    account_id: text_at(item, "account_id"),
                    event: text_at(item, "event"),
                    message: text_at(item, "message"),
                });
            }
        }
        Ok(entries)
    }
}

fn config_json_opt(raw: &str) -> Result<Option<Value>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(trimmed)
        .map_err(|err| anyhow!("高级配置不是合法 JSON：{err}"))?;
    if !value.is_object() {
        return Err(anyhow!("高级配置必须是 JSON 对象"));
    }
    Ok(Some(value))
}

/// Builds the secret-free card. `raw_config` from the service is only read for
/// the binding agent id; every secret stays behind a `_set` flag.
fn account_card_from_item(item: &Value) -> ChannelAccountCard {
    let meta = item.get("meta").cloned().unwrap_or_else(|| Value::Null);
    let preview = item.get("config").cloned().unwrap_or_else(|| Value::Null);
    let channel = text_at(item, "channel");
    let mut summary_parts: Vec<String> = Vec::new();
    let mut secret_set = false;

    let mut app_id = String::new();
    let mut corp_id = String::new();
    let mut wechat_agent_id = String::new();
    let mut original_id = String::new();
    let mut weixin_bot_id = String::new();
    let mut weixin_user_id = String::new();
    let mut bot_type = String::new();
    let mut domain = String::new();

    // The preview nests one object per channel; flatten it so the guided
    // prefill and the summary work the same for every channel shape.
    let mut flat = serde_json::Map::new();
    if let Some(section) = preview.as_object() {
        for (key, value) in section {
            if let Some(inner) = value.as_object() {
                for (inner_key, inner_value) in inner {
                    flat.insert(inner_key.clone(), inner_value.clone());
                }
            } else {
                flat.insert(key.clone(), value.clone());
            }
        }
    }
    for (key, value) in &flat {
        match key.as_str() {
            "app_id" => app_id = text_at(&Value::Object(flat.clone()), "app_id"),
            "corp_id" => corp_id = text_at(&Value::Object(flat.clone()), "corp_id"),
            "agent_id" => wechat_agent_id = text_at(&Value::Object(flat.clone()), "agent_id"),
            "original_id" => original_id = text_at(&Value::Object(flat.clone()), "original_id"),
            "ilink_bot_id" => weixin_bot_id = text_at(&Value::Object(flat.clone()), "ilink_bot_id"),
            "ilink_user_id" => weixin_user_id = text_at(&Value::Object(flat.clone()), "ilink_user_id"),
            "bot_type" => bot_type = text_at(&Value::Object(flat.clone()), "bot_type"),
            "domain" => domain = text_at(&Value::Object(flat.clone()), "domain"),
            _ => {}
        }
        if key.ends_with("_set") {
            if value.as_bool().unwrap_or(false) {
                secret_set = true;
            }
            continue;
        }
        if let Some(text) = value.as_str() {
            if !text.is_empty() {
                summary_parts.push(format!("{key}={text}"));
            }
        } else if let Some(number) = value.as_i64() {
            summary_parts.push(format!("{key}={number}"));
        }
    }

    let agent_id = item
        .get("raw_config")
        .and_then(|raw| raw.get("agent_id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    ChannelAccountCard {
        summary: summary_parts.join("  ·  "),
        secret_set,
        channel,
        account_id: text_at(item, "account_id"),
        name: text_at(item, "name"),
        status: text_at(item, "status"),
        active: item.get("active").and_then(Value::as_bool).unwrap_or(false),
        configured: meta
            .get("configured")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        peer_kind: text_at(&meta, "peer_kind"),
        agent_id,
        created_at: format_ts(item.get("created_at").and_then(Value::as_f64).unwrap_or_default()),
        updated_at: format_ts(item.get("updated_at").and_then(Value::as_f64).unwrap_or_default()),
        app_id,
        corp_id,
        wechat_agent_id,
        original_id,
        weixin_bot_id,
        weixin_user_id,
        bot_type,
        domain,
    }
}

pub(crate) fn format_ts(value: f64) -> String {
    if value <= 0.0 {
        return "未知".into();
    }
    chrono::DateTime::from_timestamp(value as i64, 0)
        .map(|v| v.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "未知".into())
}
