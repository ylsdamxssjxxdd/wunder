use crate::config::{WebFetchFirecrawlConfig, WebFetchToolConfig};
use anyhow::{anyhow, Result};
use dashmap::DashMap;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use url::Url;

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

#[derive(Debug, Clone)]
pub struct FirecrawlFetchPayload {
    pub final_url: String,
    pub status: Option<u16>,
    pub title: Option<String>,
    pub content: String,
    pub warning: Option<String>,
    pub cached: bool,
    pub fetched_at: String,
}

pub fn configured_provider(config: &WebFetchToolConfig) -> WebFetchProviderKind {
    WebFetchProviderKind::resolve(&config.provider())
}

pub fn firecrawl_configured(config: &WebFetchToolConfig) -> bool {
    resolve_firecrawl_api_key(&config.firecrawl).is_some()
        || firecrawl_base_url_is_custom(&config.firecrawl)
}

pub fn should_use_firecrawl(config: &WebFetchToolConfig) -> bool {
    match configured_provider(config) {
        WebFetchProviderKind::Firecrawl => true,
        WebFetchProviderKind::Auto => firecrawl_configured(config),
        WebFetchProviderKind::Direct => false,
    }
}

pub fn should_fallback_to_direct(config: &WebFetchToolConfig) -> bool {
    matches!(configured_provider(config), WebFetchProviderKind::Auto)
}

pub async fn fetch_with_firecrawl(
    raw_url: &str,
    max_chars: usize,
    extract_mode: &str,
    config: &WebFetchToolConfig,
) -> Result<FirecrawlFetchPayload> {
    let api_key = resolve_firecrawl_api_key(&config.firecrawl);
    if api_key.is_none() && firecrawl_requires_api_key(&config.firecrawl) {
        return Err(anyhow!(
            "Firecrawl Cloud requires tools.web.fetch.firecrawl.api_key or FIRECRAWL_API_KEY."
        ));
    }
    let endpoint = firecrawl_endpoint(&config.firecrawl)?;
    let timeout_secs = config.firecrawl.timeout_secs.clamp(1, 180);
    let cache_key = firecrawl_cache_key(raw_url, max_chars, extract_mode, &config.firecrawl);

    if let Some(cached) = read_firecrawl_cache(&cache_key) {
        return Ok(FirecrawlFetchPayload {
            cached: true,
            ..cached
        });
    }

    let body = json!({
        "url": raw_url,
        "formats": ["markdown"],
        "onlyMainContent": config.firecrawl.only_main_content,
        "timeout": timeout_secs * 1000,
        "maxAge": config.firecrawl.max_age_ms,
        "proxy": normalize_firecrawl_proxy(&config.firecrawl.proxy),
        "storeInCache": config.firecrawl.store_in_cache,
    });
    let client = firecrawl_client()?;
    let mut request = client
        .post(endpoint)
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/json")
        .json(&body);
    if let Some(api_key) = api_key {
        request = request.bearer_auth(api_key);
    }

    let response = timeout(Duration::from_secs(timeout_secs), request.send())
    .await
    .map_err(|_| anyhow!("Firecrawl request timed out after {timeout_secs}s."))?
    .map_err(|err| anyhow!("Firecrawl request failed: {err}"))?;

    let status = response.status().as_u16();
    let payload: Value = response
        .json()
        .await
        .map_err(|err| anyhow!("Firecrawl returned invalid JSON: {err}"))?;
    if !(200..300).contains(&status) || payload.get("success").and_then(Value::as_bool) == Some(false) {
        let detail = payload
            .get("error")
            .or_else(|| payload.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("request failed");
        return Err(anyhow!("Firecrawl fetch failed ({status}): {detail}"));
    }

    let result = parse_firecrawl_payload(raw_url, max_chars, extract_mode, payload)?;
    write_firecrawl_cache(&cache_key, result.clone(), config.cache_ttl_secs);
    Ok(result)
}

fn resolve_firecrawl_api_key(config: &WebFetchFirecrawlConfig) -> Option<String> {
    config
        .api_key()
        .map(|value| normalize_secret_input(&value))
        .filter(|value| !value.is_empty())
}

fn normalize_secret_input(value: &str) -> String {
    value
        .trim()
        .strip_prefix("Bearer ")
        .unwrap_or(value.trim())
        .trim()
        .to_string()
}

fn firecrawl_base_url_is_custom(config: &WebFetchFirecrawlConfig) -> bool {
    let base_url = config.base_url();
    let cleaned = base_url.trim();
    if cleaned.is_empty() {
        return false;
    }
    Url::parse(cleaned)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?.to_ascii_lowercase();
            Some(url.scheme() != "https" || host != "api.firecrawl.dev")
        })
        .unwrap_or(true)
}

fn firecrawl_requires_api_key(config: &WebFetchFirecrawlConfig) -> bool {
    Url::parse(config.base_url().trim())
        .ok()
        .and_then(|url| {
            let host = url.host_str()?.to_ascii_lowercase();
            Some(url.scheme() == "https" && host == "api.firecrawl.dev")
        })
        .unwrap_or(true)
}

fn firecrawl_endpoint(config: &WebFetchFirecrawlConfig) -> Result<String> {
    let base_url = config.base_url();
    let mut url = Url::parse(base_url.trim().trim_end_matches('/'))
        .map_err(|_| anyhow!("Firecrawl base_url must be a valid http or https URL."))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(anyhow!("Firecrawl base_url must use http or https."));
    }
    if url.host_str().unwrap_or_default().trim().is_empty() {
        return Err(anyhow!("Firecrawl base_url must include a host."));
    }
    url.set_username("")
        .map_err(|_| anyhow!("Firecrawl base_url cannot contain credentials."))?;
    url.set_password(None)
        .map_err(|_| anyhow!("Firecrawl base_url cannot contain credentials."))?;
    url.set_query(None);
    url.set_fragment(None);
    url.set_path("/v2/scrape");
    Ok(url.to_string())
}

fn normalize_firecrawl_proxy(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "basic" => "basic",
        "stealth" => "stealth",
        _ => "auto",
    }
}

fn parse_firecrawl_payload(
    raw_url: &str,
    max_chars: usize,
    extract_mode: &str,
    payload: Value,
) -> Result<FirecrawlFetchPayload> {
    let data = payload
        .get("data")
        .filter(|value| value.is_object())
        .unwrap_or(&payload);
    let metadata = data.get("metadata").filter(|value| value.is_object());
    let markdown = data
        .get("markdown")
        .or_else(|| data.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if markdown.is_empty() {
        return Err(anyhow!("Firecrawl scrape returned no content."));
    }

    let text = if extract_mode == "text" {
        firecrawl_markdown_to_text(markdown)
    } else {
        markdown.to_string()
    };
    let (content, truncated) = truncate_chars(&text, max_chars);
    let final_url = metadata
        .and_then(|value| value.get("sourceURL"))
        .or_else(|| data.get("url"))
        .and_then(Value::as_str)
        .unwrap_or(raw_url)
        .to_string();
    let status = metadata
        .and_then(|value| value.get("statusCode"))
        .or_else(|| data.get("statusCode"))
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok());
    let title = metadata
        .and_then(|value| value.get("title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let warning = payload
        .get("warning")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| truncated.then(|| format!("Content truncated after {max_chars} chars.")));

    Ok(FirecrawlFetchPayload {
        final_url,
        status,
        title,
        content,
        warning,
        cached: false,
        fetched_at: chrono::Utc::now().to_rfc3339(),
    })
}

fn firecrawl_markdown_to_text(markdown: &str) -> String {
    markdown
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches('#')
                .trim_start_matches(['-', '*', '+'])
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    let cutoff = text
        .char_indices()
        .nth(max_chars)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    (text[..cutoff].to_string(), true)
}

fn firecrawl_cache_key(
    raw_url: &str,
    max_chars: usize,
    extract_mode: &str,
    config: &WebFetchFirecrawlConfig,
) -> String {
    format!(
        "firecrawl|{}|{}|{}|{}|{}|{}|{}|{}",
        config.base_url(),
        raw_url,
        extract_mode,
        max_chars,
        config.only_main_content,
        config.max_age_ms,
        normalize_firecrawl_proxy(&config.proxy),
        config.store_in_cache
    )
}

#[derive(Debug, Clone)]
struct FirecrawlCacheEntry {
    expires_at: Instant,
    payload: FirecrawlFetchPayload,
}

fn read_firecrawl_cache(key: &str) -> Option<FirecrawlFetchPayload> {
    let cache = firecrawl_cache();
    if let Some(entry) = cache.get(key) {
        if Instant::now() <= entry.expires_at {
            return Some(entry.payload.clone());
        }
    }
    cache.remove(key);
    None
}

fn write_firecrawl_cache(key: &str, payload: FirecrawlFetchPayload, ttl_secs: u64) {
    if ttl_secs == 0 {
        return;
    }
    firecrawl_cache().insert(
        key.to_string(),
        FirecrawlCacheEntry {
            expires_at: Instant::now() + Duration::from_secs(ttl_secs),
            payload,
        },
    );
}

fn firecrawl_cache() -> &'static DashMap<String, FirecrawlCacheEntry> {
    static CACHE: OnceLock<DashMap<String, FirecrawlCacheEntry>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn firecrawl_client() -> Result<&'static reqwest::Client> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let client = reqwest::Client::builder()
        .build()
        .map_err(|err| anyhow!(err.to_string()))?;
    let _ = CLIENT.set(client);
    CLIENT
        .get()
        .ok_or_else(|| anyhow!("firecrawl client initialization failed"))
}

#[cfg(test)]
mod tests {
    use super::{
        configured_provider, firecrawl_configured, firecrawl_base_url_is_custom,
        firecrawl_endpoint, firecrawl_requires_api_key, normalize_firecrawl_proxy,
        parse_firecrawl_payload, should_fallback_to_direct, should_use_firecrawl,
        WebFetchProviderKind,
    };
    use crate::config::WebFetchToolConfig;
    use serde_json::json;

    #[test]
    fn provider_resolution_defaults_to_direct() {
        assert_eq!(WebFetchProviderKind::resolve(""), WebFetchProviderKind::Direct);
        assert_eq!(
            WebFetchProviderKind::resolve("firecrawl"),
            WebFetchProviderKind::Firecrawl
        );
        assert_eq!(WebFetchProviderKind::resolve("auto"), WebFetchProviderKind::Auto);
    }

    #[test]
    fn auto_provider_uses_firecrawl_only_when_configured() {
        let mut config = WebFetchToolConfig {
            provider: "auto".to_string(),
            ..WebFetchToolConfig::default()
        };
        assert_eq!(configured_provider(&config), WebFetchProviderKind::Auto);
        assert!(!firecrawl_configured(&config));
        assert!(!should_use_firecrawl(&config));
        assert!(should_fallback_to_direct(&config));

        config.firecrawl.api_key = Some("fc-test".to_string());
        assert!(firecrawl_configured(&config));
        assert!(should_use_firecrawl(&config));

        config.firecrawl.api_key = None;
        config.firecrawl.base_url = "http://wunder-firecrawl:3002".to_string();
        assert!(firecrawl_configured(&config));
        assert!(should_use_firecrawl(&config));
    }

    #[test]
    fn firecrawl_endpoint_accepts_cloud_and_self_hosted_urls() {
        let mut config = WebFetchToolConfig::default().firecrawl;
        assert_eq!(
            firecrawl_endpoint(&config).expect("endpoint"),
            "https://api.firecrawl.dev/v2/scrape"
        );
        assert!(firecrawl_requires_api_key(&config));
        assert!(!firecrawl_base_url_is_custom(&config));

        config.base_url = "http://wunder-firecrawl:3002".to_string();
        assert_eq!(
            firecrawl_endpoint(&config).expect("endpoint"),
            "http://wunder-firecrawl:3002/v2/scrape"
        );
        assert!(!firecrawl_requires_api_key(&config));
        assert!(firecrawl_base_url_is_custom(&config));
    }

    #[test]
    fn firecrawl_payload_maps_markdown_result() {
        let payload = json!({
            "success": true,
            "data": {
                "markdown": "# Title\n\nBody text",
                "metadata": {
                    "title": "Title",
                    "sourceURL": "https://example.com/final",
                    "statusCode": 200
                }
            }
        });
        let parsed = parse_firecrawl_payload("https://example.com", 100, "markdown", payload)
            .expect("payload");
        assert_eq!(parsed.title.as_deref(), Some("Title"));
        assert_eq!(parsed.final_url, "https://example.com/final");
        assert_eq!(parsed.status, Some(200));
        assert!(parsed.content.contains("Body text"));
    }

    #[test]
    fn firecrawl_proxy_normalizes_to_safe_values() {
        assert_eq!(normalize_firecrawl_proxy("basic"), "basic");
        assert_eq!(normalize_firecrawl_proxy("stealth"), "stealth");
        assert_eq!(normalize_firecrawl_proxy("bad"), "auto");
    }
}
