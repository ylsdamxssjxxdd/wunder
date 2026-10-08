#[cfg(feature = "web-fetch")]
mod enabled {
    pub use super::super::web_fetch_provider_impl::*;
}

#[cfg(feature = "web-fetch")]
pub use enabled::*;

#[cfg(not(feature = "web-fetch"))]
mod disabled {
    use crate::config::WebFetchToolConfig;
    use anyhow::{anyhow, Result};

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub enum WebFetchProviderKind {
        Direct,
        Firecrawl,
        Auto,
    }

    impl WebFetchProviderKind {
        pub fn resolve(raw: &str) -> Self {
            match raw.trim().to_ascii_lowercase().as_str() {
                "firecrawl" => Self::Firecrawl,
                "auto" => Self::Auto,
                _ => Self::Direct,
            }
        }

        pub fn as_str(self) -> &'static str {
            match self {
                Self::Direct => "direct",
                Self::Firecrawl => "firecrawl",
                Self::Auto => "auto",
            }
        }
    }

    pub fn configured_provider(config: &WebFetchToolConfig) -> WebFetchProviderKind {
        WebFetchProviderKind::resolve(&config.provider())
    }

    pub fn firecrawl_configured(_config: &WebFetchToolConfig) -> bool {
        false
    }

    pub fn should_use_firecrawl(_config: &WebFetchToolConfig) -> bool {
        false
    }

    pub fn should_fallback_to_direct(_config: &WebFetchToolConfig) -> bool {
        false
    }

    pub async fn fetch_with_firecrawl(
        _raw_url: &str,
        _max_chars: usize,
        _extract_mode: &str,
        _config: &WebFetchToolConfig,
    ) -> Result<serde_json::Value> {
        Err(anyhow!(
            "web-fetch feature is disabled; rebuild with --features web-fetch"
        ))
    }
}

#[cfg(not(feature = "web-fetch"))]
pub use disabled::*;

#[cfg(all(test, not(feature = "web-fetch")))]
mod tests {
    use super::*;

    #[test]
    fn disabled_provider_still_normalizes_configured_provider() {
        assert_eq!(
            WebFetchProviderKind::resolve("firecrawl"),
            WebFetchProviderKind::Firecrawl
        );
        assert_eq!(
            WebFetchProviderKind::resolve("auto"),
            WebFetchProviderKind::Auto
        );
        assert_eq!(
            WebFetchProviderKind::resolve("unknown"),
            WebFetchProviderKind::Direct
        );
    }
}
