use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPeer {
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelThread {
    pub id: String,
    #[serde(default)]
    pub topic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSender {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAttachment {
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub mime: Option<String>,
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelLocation {
    pub lat: f64,
    pub lng: f64,
    #[serde(default)]
    pub address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WhatsappCloudConfig {
    #[serde(default)]
    pub phone_number_id: Option<String>,
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub verify_token: Option<String>,
    #[serde(default)]
    pub app_secret: Option<String>,
    #[serde(default)]
    pub api_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeishuConfig {
    #[serde(default, alias = "appId")]
    pub app_id: Option<String>,
    #[serde(default, alias = "appSecret")]
    pub app_secret: Option<String>,
    #[serde(default, alias = "verifyToken", alias = "verificationToken")]
    pub verification_token: Option<String>,
    #[serde(default, alias = "encryptKey")]
    pub encrypt_key: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default, alias = "receiveIdType")]
    pub receive_id_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QqBotConfig {
    #[serde(default, alias = "appId")]
    pub app_id: Option<String>,
    #[serde(default, alias = "clientSecret")]
    pub client_secret: Option<String>,
    #[serde(default, alias = "markdownSupport")]
    pub markdown_support: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessage {
    pub channel: String,
    pub account_id: String,
    pub peer: ChannelPeer,
    #[serde(default)]
    pub thread: Option<ChannelThread>,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub sender: Option<ChannelSender>,
    #[serde(rename = "type")]
    pub message_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub attachments: Vec<ChannelAttachment>,
    #[serde(default)]
    pub location: Option<ChannelLocation>,
    #[serde(default)]
    pub ts: Option<f64>,
    #[serde(default)]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelOutboundMessage {
    pub channel: String,
    pub account_id: String,
    pub peer: ChannelPeer,
    #[serde(default)]
    pub thread: Option<ChannelThread>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub attachments: Vec<ChannelAttachment>,
    #[serde(default)]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelAccountConfig {
    #[serde(default)]
    pub inbound_token: Option<String>,
    #[serde(default)]
    pub outbound_url: Option<String>,
    #[serde(default)]
    pub outbound_token: Option<String>,
    #[serde(default)]
    pub outbound_headers: Option<Value>,
    #[serde(default)]
    pub timeout_s: Option<u64>,
    #[serde(default)]
    pub allow_peers: Vec<String>,
    #[serde(default)]
    pub deny_peers: Vec<String>,
    #[serde(default)]
    pub allow_senders: Vec<String>,
    #[serde(default)]
    pub deny_senders: Vec<String>,
    #[serde(default)]
    pub tts_enabled: Option<bool>,
    #[serde(default)]
    pub tts_voice: Option<String>,
    #[serde(default)]
    pub tool_overrides: Vec<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub whatsapp_cloud: Option<WhatsappCloudConfig>,
    #[serde(default)]
    pub feishu: Option<FeishuConfig>,
    #[serde(default)]
    pub qqbot: Option<QqBotConfig>,
}

impl ChannelAccountConfig {
    pub fn from_value(value: &Value) -> Self {
        serde_json::from_value::<ChannelAccountConfig>(value.clone()).unwrap_or_default()
    }
}
