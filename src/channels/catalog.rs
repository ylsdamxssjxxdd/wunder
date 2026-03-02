#[derive(Debug, Clone, Copy)]
pub struct ChannelCatalogItem {
    pub channel: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub user_supported: bool,
    pub webhook_mode: &'static str,
    pub docs_hint: &'static str,
}

pub const CHANNEL_CATALOG: &[ChannelCatalogItem] = &[
    ChannelCatalogItem {
        channel: "feishu",
        display_name: "Feishu",
        description: "Feishu webhook and long-connection channel",
        user_supported: true,
        webhook_mode: "specialized+generic",
        docs_hint: "/wunder/channel/feishu/webhook",
    },
    ChannelCatalogItem {
        channel: "qqbot",
        display_name: "QQ Bot",
        description: "QQ bot webhook and outbound adapter",
        user_supported: true,
        webhook_mode: "specialized+generic",
        docs_hint: "/wunder/channel/qqbot/webhook",
    },
    ChannelCatalogItem {
        channel: "whatsapp",
        display_name: "WhatsApp Cloud",
        description: "WhatsApp Cloud API webhook and outbound adapter",
        user_supported: true,
        webhook_mode: "specialized+generic",
        docs_hint: "/wunder/channel/whatsapp/webhook",
    },
    ChannelCatalogItem {
        channel: "wechat",
        display_name: "WeCom",
        description: "Enterprise WeChat callback and outbound adapter",
        user_supported: true,
        webhook_mode: "specialized+generic",
        docs_hint: "/wunder/channel/wechat/webhook",
    },
    ChannelCatalogItem {
        channel: "wechat_mp",
        display_name: "WeChat MP",
        description: "WeChat Official Account callback and outbound adapter",
        user_supported: true,
        webhook_mode: "specialized+generic",
        docs_hint: "/wunder/channel/wechat_mp/webhook",
    },
    ChannelCatalogItem {
        channel: "telegram",
        display_name: "Telegram",
        description: "User-managed Telegram webhook/callback channel",
        user_supported: true,
        webhook_mode: "generic",
        docs_hint: "/wunder/channel/telegram/webhook",
    },
];

pub fn find_channel(channel: &str) -> Option<&'static ChannelCatalogItem> {
    let normalized = channel.trim();
    if normalized.is_empty() {
        return None;
    }
    CHANNEL_CATALOG
        .iter()
        .find(|item| item.channel.eq_ignore_ascii_case(normalized))
}

pub fn user_supported_channels() -> Vec<&'static ChannelCatalogItem> {
    CHANNEL_CATALOG
        .iter()
        .filter(|item| item.user_supported)
        .collect()
}

pub fn user_supported_channel_names() -> Vec<&'static str> {
    user_supported_channels()
        .into_iter()
        .map(|item| item.channel)
        .collect()
}
