#[cfg(feature = "xmpp")]
mod enabled {
    pub use super::super::xmpp_impl::*;
}

#[cfg(feature = "xmpp")]
pub use enabled::*;

#[cfg(not(feature = "xmpp"))]
mod disabled {
    use crate::channels::adapter::{ChannelAdapter, OutboundContext};
    use crate::channels::types::{ChannelOutboundMessage, XmppConfig};
    use anyhow::{anyhow, Result};
    use async_trait::async_trait;
    use reqwest::Client;
    use serde_json::{json, Value};
    use std::future::Future;

    pub const XMPP_CHANNEL: &str = "xmpp";

    #[derive(Debug, Default)]
    pub struct XmppAdapter;

    #[derive(Debug, Clone)]
    pub struct XmppRosterContact {
        pub jid: String,
        pub name: Option<String>,
        pub subscription: String,
        pub ask: Option<String>,
        pub groups: Vec<String>,
    }

    #[async_trait]
    impl ChannelAdapter for XmppAdapter {
        fn channel(&self) -> &'static str {
            XMPP_CHANNEL
        }

        async fn send_outbound(&self, _context: OutboundContext<'_>) -> Result<()> {
            Err(xmpp_disabled_error())
        }

        async fn health_check(
            &self,
            _http: &Client,
            account_config: &crate::channels::types::ChannelAccountConfig,
        ) -> Result<Value> {
            let configured = account_config
                .xmpp
                .as_ref()
                .is_some_and(has_long_connection_credentials);
            Ok(json!({
                "status": if configured { "feature_disabled" } else { "not_configured" },
                "enabled": false,
                "reason": "xmpp feature is disabled"
            }))
        }
    }

    pub fn long_connection_enabled(config: &XmppConfig) -> bool {
        config.long_connection_enabled.unwrap_or(true)
    }

    pub fn has_long_connection_credentials(config: &XmppConfig) -> bool {
        config
            .jid
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            && config
                .password
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }

    pub async fn send_outbound(
        _account_id: &str,
        _outbound: &ChannelOutboundMessage,
        _config: &XmppConfig,
    ) -> Result<()> {
        Err(xmpp_disabled_error())
    }

    pub async fn fetch_roster_contacts(
        _account_id: &str,
        _config: &XmppConfig,
        _force_refresh: bool,
    ) -> Result<Vec<XmppRosterContact>> {
        Err(xmpp_disabled_error())
    }

    pub async fn run_long_connection_session<F, Fut>(
        _account_id: &str,
        _config: &XmppConfig,
        _on_message: F,
    ) -> Result<()>
    where
        F: FnMut(crate::channels::types::ChannelMessage) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        Err(xmpp_disabled_error())
    }

    fn xmpp_disabled_error() -> anyhow::Error {
        anyhow!("xmpp feature is disabled; rebuild with --features xmpp")
    }
}

#[cfg(not(feature = "xmpp"))]
pub use disabled::*;

#[cfg(all(test, not(feature = "xmpp")))]
mod tests {
    use super::*;
    use crate::channels::types::{ChannelOutboundMessage, ChannelPeer, XmppConfig};

    #[test]
    fn disabled_stub_keeps_config_helpers_available() {
        let empty = XmppConfig::default();
        assert!(long_connection_enabled(&empty));
        assert!(!has_long_connection_credentials(&empty));

        let full = XmppConfig {
            jid: Some("user@example.test".to_string()),
            password: Some("secret".to_string()),
            ..Default::default()
        };
        assert!(has_long_connection_credentials(&full));
    }

    #[tokio::test]
    async fn disabled_stub_returns_clear_send_error() {
        let outbound = ChannelOutboundMessage {
            channel: XMPP_CHANNEL.to_string(),
            account_id: "account".to_string(),
            peer: ChannelPeer {
                kind: "user".to_string(),
                id: "target@example.test".to_string(),
                name: None,
            },
            thread: Default::default(),
            text: Some("hello".to_string()),
            attachments: Vec::new(),
            meta: None,
        };
        let err = send_outbound("account", &outbound, &XmppConfig::default())
            .await
            .expect_err("xmpp should be disabled in default feature set");
        assert!(err.to_string().contains("xmpp feature is disabled"));
    }
}
