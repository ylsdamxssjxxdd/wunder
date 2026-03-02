use crate::channels::adapter::ChannelAdapter;
use crate::channels::feishu::FeishuAdapter;
use crate::channels::qqbot::QqBotAdapter;
use crate::channels::wechat::WechatAdapter;
use crate::channels::wechat_mp::WechatMpAdapter;
use crate::channels::whatsapp_cloud::WhatsappCloudAdapter;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct ChannelAdapterRegistry {
    adapters: Arc<RwLock<HashMap<String, Arc<dyn ChannelAdapter>>>>,
}

impl ChannelAdapterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, adapter: Arc<dyn ChannelAdapter>) -> Result<()> {
        let key = adapter.channel().trim().to_ascii_lowercase();
        if key.is_empty() {
            return Err(anyhow!("channel adapter key is empty"));
        }
        let mut guard = self.adapters.write();
        if guard.contains_key(&key) {
            return Err(anyhow!("channel adapter already registered: {key}"));
        }
        guard.insert(key, adapter);
        Ok(())
    }

    pub fn register_replace(&self, adapter: Arc<dyn ChannelAdapter>) {
        let key = adapter.channel().trim().to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        let mut guard = self.adapters.write();
        guard.insert(key, adapter);
    }

    pub fn get(&self, channel: &str) -> Option<Arc<dyn ChannelAdapter>> {
        let key = channel.trim().to_ascii_lowercase();
        if key.is_empty() {
            return None;
        }
        let guard = self.adapters.read();
        guard.get(&key).cloned()
    }

    pub fn list(&self) -> Vec<String> {
        let guard = self.adapters.read();
        let mut items: Vec<String> = guard.keys().cloned().collect();
        items.sort_unstable();
        items
    }
}

pub fn build_default_channel_adapter_registry() -> ChannelAdapterRegistry {
    let registry = ChannelAdapterRegistry::new();
    registry.register_replace(Arc::new(WhatsappCloudAdapter));
    registry.register_replace(Arc::new(FeishuAdapter));
    registry.register_replace(Arc::new(QqBotAdapter));
    registry.register_replace(Arc::new(WechatAdapter));
    registry.register_replace(Arc::new(WechatMpAdapter));
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::adapter::{ChannelAdapter, OutboundContext};
    use anyhow::Result;
    use async_trait::async_trait;

    struct MockAdapter;

    #[async_trait]
    impl ChannelAdapter for MockAdapter {
        fn channel(&self) -> &'static str {
            "mock"
        }

        async fn send_outbound(&self, _context: OutboundContext<'_>) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn register_and_get_adapter() {
        let registry = ChannelAdapterRegistry::new();
        registry.register(Arc::new(MockAdapter)).unwrap();
        assert!(registry.get("mock").is_some());
        assert!(registry.get("MOCK").is_some());
    }

    #[test]
    fn duplicate_register_returns_error() {
        let registry = ChannelAdapterRegistry::new();
        registry.register(Arc::new(MockAdapter)).unwrap();
        let err = registry.register(Arc::new(MockAdapter)).unwrap_err();
        assert!(err.to_string().contains("already registered"));
    }

    #[test]
    fn list_is_sorted() {
        let registry = ChannelAdapterRegistry::new();
        registry.register_replace(Arc::new(MockAdapter));
        assert_eq!(registry.list(), vec!["mock".to_string()]);
    }
}
