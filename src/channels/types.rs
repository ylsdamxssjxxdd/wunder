use serde::{de, Deserialize, Deserializer, Serialize};
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
    #[serde(default, alias = "longConnectionEnabled")]
    pub long_connection_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QqBotConfig {
    #[serde(default, alias = "appId", alias = "appid")]
    pub app_id: Option<String>,
    #[serde(default, alias = "clientSecret", alias = "appSecret", alias = "secret")]
    pub client_secret: Option<String>,
    #[serde(default, alias = "botToken")]
    pub token: Option<String>,
    #[serde(default, alias = "markdownSupport")]
    pub markdown_support: Option<bool>,
    #[serde(default, alias = "longConnectionEnabled")]
    pub long_connection_enabled: Option<bool>,
    #[serde(default)]
    pub intents: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WechatConfig {
    #[serde(default, alias = "corpId")]
    pub corp_id: Option<String>,
    #[serde(default, alias = "agentId")]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default, alias = "encodingAesKey")]
    pub encoding_aes_key: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WechatMpConfig {
    #[serde(default, alias = "appId")]
    pub app_id: Option<String>,
    #[serde(default, alias = "appSecret")]
    pub app_secret: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default, alias = "encodingAesKey")]
    pub encoding_aes_key: Option<String>,
    #[serde(default, alias = "originalId", alias = "ghId")]
    pub original_id: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct XmppConfig {
    #[serde(default)]
    pub jid: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default, alias = "passwordEnv")]
    pub password_env: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default, alias = "server")]
    pub host: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_u16_from_any")]
    pub port: Option<u16>,
    #[serde(default, alias = "directTls")]
    pub direct_tls: Option<bool>,
    #[serde(
        default,
        alias = "trustSelfSigned",
        alias = "allowSelfSigned",
        alias = "insecureSkipTlsVerify"
    )]
    pub trust_self_signed: Option<bool>,
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default, alias = "mucNick")]
    pub muc_nick: Option<String>,
    #[serde(
        default,
        alias = "rooms",
        alias = "mucRooms",
        deserialize_with = "deserialize_string_vec_from_any"
    )]
    pub muc_rooms: Vec<String>,
    #[serde(default, alias = "longConnectionEnabled")]
    pub long_connection_enabled: Option<bool>,
    #[serde(default, alias = "sendInitialPresence")]
    pub send_initial_presence: Option<bool>,
    #[serde(default, alias = "statusText")]
    pub status_text: Option<String>,
    #[serde(default, alias = "heartbeatEnabled")]
    pub heartbeat_enabled: Option<bool>,
    #[serde(
        default,
        alias = "heartbeatIntervalS",
        deserialize_with = "deserialize_option_u64_from_any"
    )]
    pub heartbeat_interval_s: Option<u64>,
    #[serde(
        default,
        alias = "heartbeatTimeoutS",
        deserialize_with = "deserialize_option_u64_from_any"
    )]
    pub heartbeat_timeout_s: Option<u64>,
    #[serde(default, alias = "respondPing")]
    pub respond_ping: Option<bool>,
}

fn deserialize_option_u16_from_any<'de, D>(deserializer: D) -> Result<Option<u16>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|raw| u16::try_from(raw).ok())
            .ok_or_else(|| de::Error::custom("invalid u16 value"))
            .map(Some),
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed
                .parse::<u16>()
                .map(Some)
                .map_err(|_| de::Error::custom("invalid u16 value"))
        }
        other => Err(de::Error::custom(format!(
            "invalid u16 value type: {}",
            other
        ))),
    }
}

fn deserialize_option_u64_from_any<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| de::Error::custom("invalid u64 value"))
            .map(Some),
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed
                .parse::<u64>()
                .map(Some)
                .map_err(|_| de::Error::custom("invalid u64 value"))
        }
        other => Err(de::Error::custom(format!(
            "invalid u64 value type: {}",
            other
        ))),
    }
}

fn deserialize_string_vec_from_any<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    match value {
        Value::Array(items) => Ok(items
            .into_iter()
            .filter_map(|item| {
                item.as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .collect()),
        Value::String(raw) => Ok(raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect()),
        Value::Null => Ok(Vec::new()),
        other => Err(de::Error::custom(format!(
            "invalid string list type: {}",
            other
        ))),
    }
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
    #[serde(default)]
    pub wechat: Option<WechatConfig>,
    #[serde(default)]
    pub wechat_mp: Option<WechatMpConfig>,
    #[serde(default)]
    pub xmpp: Option<XmppConfig>,
}

impl ChannelAccountConfig {
    pub fn from_value(value: &Value) -> Self {
        serde_json::from_value::<ChannelAccountConfig>(value.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn xmpp_openfang_compatibility_aliases_are_supported() {
        let config = ChannelAccountConfig::from_value(&json!({
            "xmpp": {
                "jid": "bot@jabber.org",
                "password_env": "XMPP_PASSWORD",
                "server": "xmpp.jabber.org",
                "port": 5222,
                "rooms": [
                    "room-a@conference.jabber.org",
                    "room-b@conference.jabber.org"
                ]
            }
        }));
        let xmpp = config.xmpp.expect("xmpp config should exist");

        assert_eq!(xmpp.jid.as_deref(), Some("bot@jabber.org"));
        assert_eq!(xmpp.password_env.as_deref(), Some("XMPP_PASSWORD"));
        assert_eq!(xmpp.host.as_deref(), Some("xmpp.jabber.org"));
        assert_eq!(xmpp.port, Some(5222));
        assert_eq!(
            xmpp.muc_rooms,
            vec![
                "room-a@conference.jabber.org".to_string(),
                "room-b@conference.jabber.org".to_string()
            ]
        );
    }

    #[test]
    fn xmpp_rooms_alias_supports_csv_string() {
        let config = ChannelAccountConfig::from_value(&json!({
            "xmpp": {
                "rooms": "room1@conference.example.com, room2@conference.example.com"
            }
        }));
        let xmpp = config.xmpp.expect("xmpp config should exist");
        assert_eq!(
            xmpp.muc_rooms,
            vec![
                "room1@conference.example.com".to_string(),
                "room2@conference.example.com".to_string()
            ]
        );
    }

    #[test]
    fn xmpp_trust_self_signed_aliases_are_supported() {
        let config = ChannelAccountConfig::from_value(&json!({
            "xmpp": {
                "trustSelfSigned": true
            }
        }));
        let xmpp = config.xmpp.expect("xmpp config should exist");
        assert_eq!(xmpp.trust_self_signed, Some(true));

        let config = ChannelAccountConfig::from_value(&json!({
            "xmpp": {
                "insecureSkipTlsVerify": false
            }
        }));
        let xmpp = config.xmpp.expect("xmpp config should exist");
        assert_eq!(xmpp.trust_self_signed, Some(false));
    }

    #[test]
    fn qqbot_openfang_compatibility_aliases_are_supported() {
        let config = ChannelAccountConfig::from_value(&json!({
            "qqbot": {
                "appid": "123456",
                "secret": "qq-secret",
                "botToken": "123456:qq-secret",
                "longConnectionEnabled": false
            }
        }));
        let qqbot = config.qqbot.expect("qqbot config should exist");
        assert_eq!(qqbot.app_id.as_deref(), Some("123456"));
        assert_eq!(qqbot.client_secret.as_deref(), Some("qq-secret"));
        assert_eq!(qqbot.token.as_deref(), Some("123456:qq-secret"));
        assert_eq!(qqbot.long_connection_enabled, Some(false));
    }
}
