use super::{
    ToolContext, build_model_tool_success, tool_error::ToolErrorMeta,
    tool_error::build_failed_tool_result,
};
use crate::config::{Config, WebSearchFirecrawlConfig, WebSearchToolConfig};
use crate::i18n;
use anyhow::{Result, anyhow};
use dashmap::DashMap;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::time::{Duration, Instant};
use tokio::time::timeout;
use url::Url;

pub const TOOL_WEB_SEARCH: &str = "网页搜索";
pub const TOOL_WEB_SEARCH_ALIAS: &str = "web_search";

const MIN_COUNT: usize = 1;
const MAX_COUNT: usize = 10;
const MIN_MAX_RESULT_CHARS: usize = 120;
const MAX_MAX_RESULT_CHARS: usize = 4_000;

#[derive(Debug, Deserialize)]
struct WebSearchArgs {
    query: String,
    #[serde(default)]
    count: Option<usize>,
    #[serde(default, alias = "siteUrl", alias = "domain")]
    site: Option<String>,
    #[serde(default, alias = "siteUrls", alias = "domains")]
    sites: Option<Vec<String>>,
    #[serde(default, alias = "scrapeResults")]
    scrape_results: Option<bool>,
    #[serde(default, alias = "maxResultChars")]
    max_result_chars: Option<usize>,
    #[serde(default)]
    sources: Option<Vec<String>>,
    #[serde(default)]
    categories: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchResultItem {
    title: String,
    url: String,
    description: Option<String>,
    content: Option<String>,
    published: Option<String>,
    site_name: Option<String>,
}

#[derive(Debug, Clone)]
struct SearchPayload {
    query: String,
    effective_query: String,
    provider: String,
    count: usize,
    cached: bool,
    took_ms: u128,
    scrape_results: bool,
    searched_at: String,
    site_filters: Vec<String>,
    results: Vec<SearchResultItem>,
}

#[derive(Debug)]
struct WebSearchFailure {
    message: String,
    data: Value,
    meta: ToolErrorMeta,
}

impl WebSearchFailure {
    fn into_value(self) -> Value {
        build_failed_tool_result(self.message, self.data, self.meta, false)
    }
}

pub fn is_web_search_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    if cleaned == TOOL_WEB_SEARCH {
        return true;
    }
    cleaned.eq_ignore_ascii_case(TOOL_WEB_SEARCH_ALIAS)
}

pub fn web_search_enabled(config: &Config) -> bool {
    config.tools.web.search.enabled
}

pub async fn tool_web_search(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let request: WebSearchArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let query = request.query.trim();
    let config = &context.config.tools.web.search;
    if query.is_empty() {
        return Ok(web_search_failure(
            query,
            "validation",
            "TOOL_WEB_SEARCH_EMPTY_QUERY",
            i18n::t("tool.web_search.empty_query"),
            Some("Pass a natural-language search query, not a URL.".to_string()),
            false,
            None,
            json!({}),
        )
        .into_value());
    }
    match config.provider().as_str() {
        "firecrawl" => match search_with_firecrawl(query, &request, config).await {
            Ok(payload) => Ok(build_search_result(payload)),
            Err(failure) => Ok(failure.into_value()),
        },
        _ => Ok(web_search_failure(
            query,
            "provider",
            "TOOL_WEB_SEARCH_PROVIDER_UNSUPPORTED",
            i18n::t("tool.web_search.provider_unsupported"),
            Some("Configure tools.web.search.provider to firecrawl.".to_string()),
            false,
            None,
            json!({
                "configured_provider": config.provider(),
            }),
        )
        .into_value()),
    }
}

async fn search_with_firecrawl(
    query: &str,
    request: &WebSearchArgs,
    config: &WebSearchToolConfig,
) -> std::result::Result<SearchPayload, WebSearchFailure> {
    let site_filters = normalize_site_filters(request.site.as_deref(), request.sites.as_deref());
    let effective_query = build_effective_query(query, &site_filters);
    let count = resolve_count(request.count, config.count);
    let max_result_chars = resolve_max_result_chars(request.max_result_chars, config);
    let scrape_results = request.scrape_results.unwrap_or(false);
    let sources = clean_string_list(request.sources.as_deref());
    let categories = clean_string_list(request.categories.as_deref());
    let cache_key = search_cache_key(
        &effective_query,
        count,
        max_result_chars,
        scrape_results,
        &sources,
        &categories,
        &config.firecrawl,
    );
    if let Some(mut cached) = read_search_cache(&cache_key) {
        cached.cached = true;
        return Ok(cached);
    }

    let endpoint = firecrawl_search_endpoint(&config.firecrawl).map_err(|err| {
        web_search_failure(
            query,
            "configuration",
            "TOOL_WEB_SEARCH_PROVIDER_CONFIG_INVALID",
            format!("{}: {err}", i18n::t("tool.web_search.provider_failed")),
            Some("Check tools.web.search.firecrawl.base_url.".to_string()),
            false,
            None,
            json!({
                "provider": "firecrawl",
            }),
        )
    })?;
    let api_key = resolve_firecrawl_api_key(&config.firecrawl);
    if api_key.is_none() && firecrawl_requires_api_key(&config.firecrawl) {
        return Err(web_search_failure(
            query,
            "configuration",
            "TOOL_WEB_SEARCH_PROVIDER_AUTH_REQUIRED",
            i18n::t("tool.web_search.firecrawl_api_key_required"),
            Some("Set FIRECRAWL_API_KEY or use a self-hosted FIRECRAWL_BASE_URL.".to_string()),
            false,
            None,
            json!({
                "provider": "firecrawl",
            }),
        ));
    }

    let timeout_secs = config.firecrawl.timeout_secs.clamp(1, 180);
    let body = build_firecrawl_search_body(
        &effective_query,
        count,
        scrape_results,
        &sources,
        &categories,
    );
    let client = firecrawl_client().map_err(|err| {
        web_search_failure(
            query,
            "request",
            "TOOL_WEB_SEARCH_REQUEST_FAILED",
            err.to_string(),
            None,
            true,
            Some(timeout_secs.saturating_mul(1000)),
            json!({
                "provider": "firecrawl",
            }),
        )
    })?;

    let start = Instant::now();
    let mut request_builder = client
        .post(endpoint)
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/json")
        .json(&body);
    if let Some(api_key) = api_key {
        request_builder = request_builder.bearer_auth(api_key);
    }
    let response = timeout(Duration::from_secs(timeout_secs), request_builder.send())
        .await
        .map_err(|_| {
            web_search_failure(
                query,
                "request",
                "TOOL_WEB_SEARCH_TIMEOUT",
                i18n::t("tool.web_search.timeout"),
                Some("Retry later or reduce count/scrape_results.".to_string()),
                true,
                Some(timeout_secs.saturating_mul(1000)),
                json!({
                    "provider": "firecrawl",
                    "timeout_secs": timeout_secs,
                }),
            )
        })?
        .map_err(|err| {
            web_search_failure(
                query,
                "request",
                "TOOL_WEB_SEARCH_REQUEST_FAILED",
                format!("{}: {err}", i18n::t("tool.web_search.provider_failed")),
                None,
                true,
                Some(timeout_secs.saturating_mul(1000)),
                json!({
                    "provider": "firecrawl",
                }),
            )
        })?;
    let status = response.status().as_u16();
    let payload: Value = response.json().await.map_err(|err| {
        web_search_failure(
            query,
            "provider",
            "TOOL_WEB_SEARCH_PROVIDER_INVALID_JSON",
            format!("{}: {err}", i18n::t("tool.web_search.invalid_json")),
            None,
            true,
            Some(timeout_secs.saturating_mul(1000)),
            json!({
                "provider": "firecrawl",
                "status": status,
            }),
        )
    })?;
    if !(200..300).contains(&status)
        || payload.get("success").and_then(Value::as_bool) == Some(false)
    {
        let detail = payload
            .get("error")
            .or_else(|| payload.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("request failed");
        return Err(web_search_failure(
            query,
            "provider",
            "TOOL_WEB_SEARCH_PROVIDER_FAILED",
            format!(
                "{}: Firecrawl Search failed ({status}): {detail}",
                i18n::t("tool.web_search.provider_failed")
            ),
            Some("Check Firecrawl logs or retry with a narrower query.".to_string()),
            true,
            Some(timeout_secs.saturating_mul(1000)),
            json!({
                "provider": "firecrawl",
                "status": status,
            }),
        ));
    }

    let results = parse_firecrawl_search_items(&payload, max_result_chars);
    let payload = SearchPayload {
        query: query.to_string(),
        effective_query,
        provider: "firecrawl".to_string(),
        count: results.len(),
        cached: false,
        took_ms: start.elapsed().as_millis(),
        scrape_results,
        searched_at: chrono::Utc::now().to_rfc3339(),
        site_filters,
        results,
    };
    write_search_cache(&cache_key, payload.clone(), config.cache_ttl_secs);
    Ok(payload)
}

fn build_firecrawl_search_body(
    query: &str,
    count: usize,
    scrape_results: bool,
    sources: &[String],
    categories: &[String],
) -> Value {
    let mut body = Map::new();
    body.insert("query".to_string(), Value::String(query.to_string()));
    body.insert("limit".to_string(), Value::from(count as u64));
    if !sources.is_empty() {
        body.insert(
            "sources".to_string(),
            Value::Array(sources.iter().cloned().map(Value::String).collect()),
        );
    }
    if !categories.is_empty() {
        body.insert(
            "categories".to_string(),
            Value::Array(categories.iter().cloned().map(Value::String).collect()),
        );
    }
    if scrape_results {
        body.insert(
            "scrapeOptions".to_string(),
            json!({
                "formats": ["markdown"],
            }),
        );
    }
    Value::Object(body)
}

fn build_search_result(payload: SearchPayload) -> Value {
    let count = payload.count;
    let next_step_hint = if count == 0 {
        "No search results were returned. Do not guess URLs or fabricate sources; retry with a narrower query, change provider settings, or report that web search returned no evidence."
    } else {
        "Pick concrete result URLs and call web_fetch for source pages that need verification."
    };
    let data = json!({
        "query": payload.query,
        "effective_query": payload.effective_query,
        "provider": payload.provider,
        "count": count,
        "cached": payload.cached,
        "took_ms": payload.took_ms,
        "scrape_results": payload.scrape_results,
        "searched_at": payload.searched_at,
        "site_filters": payload.site_filters,
        "results": payload.results.into_iter().map(|item| {
            json!({
                "title": item.title,
                "url": item.url,
                "description": item.description,
                "content": item.content,
                "published": item.published,
                "site_name": item.site_name,
            })
        }).collect::<Vec<_>>(),
        "next_step_hint": next_step_hint,
    });
    build_model_tool_success(
        "web_search",
        "completed",
        format!("Found {count} web results."),
        data,
    )
}

fn web_search_failure(
    query: &str,
    phase: &str,
    code: &str,
    message: impl Into<String>,
    hint: Option<String>,
    retryable: bool,
    retry_after_ms: Option<u64>,
    extra: Value,
) -> WebSearchFailure {
    let message = message.into();
    let mut data = Map::new();
    data.insert("query".to_string(), Value::String(query.to_string()));
    data.insert("phase".to_string(), Value::String(phase.to_string()));
    data.insert(
        "failure_summary".to_string(),
        Value::String(message.clone()),
    );
    data.insert(
        "error_detail_head".to_string(),
        Value::String(message.clone()),
    );
    if let Some(text) = hint.clone().filter(|value| !value.trim().is_empty()) {
        data.insert("next_step_hint".to_string(), Value::String(text));
    }
    if let Value::Object(map) = extra {
        data.extend(map);
    }
    WebSearchFailure {
        message,
        data: Value::Object(data),
        meta: ToolErrorMeta::new(code, hint, retryable, retry_after_ms),
    }
}

fn parse_firecrawl_search_items(payload: &Value, max_result_chars: usize) -> Vec<SearchResultItem> {
    firecrawl_result_candidates(payload)
        .into_iter()
        .flat_map(|items| items.iter())
        .filter_map(|entry| parse_firecrawl_search_item(entry, max_result_chars))
        .collect()
}

fn firecrawl_result_candidates(payload: &Value) -> Vec<&Vec<Value>> {
    let mut candidates = Vec::new();
    if let Some(items) = payload.get("data").and_then(Value::as_array) {
        candidates.push(items);
    }
    if let Some(items) = payload.get("results").and_then(Value::as_array) {
        candidates.push(items);
    }
    if let Some(data) = payload.get("data") {
        if let Some(items) = data.get("results").and_then(Value::as_array) {
            candidates.push(items);
        }
        if let Some(items) = data.get("data").and_then(Value::as_array) {
            candidates.push(items);
        }
        if let Some(items) = data.get("web").and_then(Value::as_array) {
            candidates.push(items);
        }
    }
    if let Some(items) = payload
        .get("web")
        .and_then(|value| value.get("results"))
        .and_then(Value::as_array)
    {
        candidates.push(items);
    }
    candidates
}

fn parse_firecrawl_search_item(entry: &Value, max_result_chars: usize) -> Option<SearchResultItem> {
    let record = entry.as_object()?;
    let metadata = record.get("metadata").and_then(Value::as_object);
    let url = string_field(record, "url")
        .or_else(|| string_field(record, "sourceURL"))
        .or_else(|| string_field(record, "sourceUrl"))
        .or_else(|| metadata.and_then(|value| string_field(value, "sourceURL")))?;
    if Url::parse(&url).is_err() {
        return None;
    }
    let title = string_field(record, "title")
        .or_else(|| metadata.and_then(|value| string_field(value, "title")))
        .unwrap_or_else(|| url.clone());
    let description = string_field(record, "description")
        .or_else(|| string_field(record, "snippet"))
        .or_else(|| string_field(record, "summary"))
        .map(|value| truncate_string(&value, max_result_chars));
    let content = string_field(record, "markdown")
        .or_else(|| string_field(record, "content"))
        .or_else(|| string_field(record, "text"))
        .map(|value| truncate_string(&value, max_result_chars));
    let published = string_field(record, "publishedDate")
        .or_else(|| string_field(record, "published"))
        .or_else(|| metadata.and_then(|value| string_field(value, "publishedTime")))
        .or_else(|| metadata.and_then(|value| string_field(value, "publishedDate")));
    Some(SearchResultItem {
        title: truncate_string(&title, max_result_chars),
        url: url.clone(),
        description,
        content,
        published,
        site_name: site_name(&url),
    })
}

fn string_field(record: &Map<String, Value>, key: &str) -> Option<String> {
    record
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn site_name(raw_url: &str) -> Option<String> {
    Url::parse(raw_url)
        .ok()
        .and_then(|url| url.host_str().map(ToString::to_string))
        .map(|host| host.trim_start_matches("www.").to_string())
        .filter(|host| !host.is_empty())
}

fn resolve_count(request_count: Option<usize>, default_count: usize) -> usize {
    request_count
        .unwrap_or(default_count)
        .clamp(MIN_COUNT, MAX_COUNT)
}

fn resolve_max_result_chars(request_max: Option<usize>, config: &WebSearchToolConfig) -> usize {
    request_max
        .unwrap_or(config.max_result_chars)
        .clamp(MIN_MAX_RESULT_CHARS, MAX_MAX_RESULT_CHARS)
}

fn clean_string_list(values: Option<&[String]>) -> Vec<String> {
    values
        .unwrap_or_default()
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn normalize_site_filters(site: Option<&str>, sites: Option<&[String]>) -> Vec<String> {
    let mut values = Vec::new();
    if let Some(site) = site {
        values.push(site.to_string());
    }
    values.extend(sites.unwrap_or_default().iter().cloned());
    values
        .into_iter()
        .filter_map(|value| normalize_site_filter(&value))
        .fold(Vec::new(), |mut acc, value| {
            if !acc.iter().any(|item| item == &value) {
                acc.push(value);
            }
            acc
        })
}

fn normalize_site_filter(raw: &str) -> Option<String> {
    let trimmed = raw
        .trim()
        .trim_start_matches("site:")
        .trim_start_matches("SITE:")
        .trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let host = Url::parse(&candidate)
        .ok()
        .and_then(|url| url.host_str().map(ToString::to_string))
        .or_else(|| trimmed.split('/').next().map(ToString::to_string))?;
    let host = host
        .trim()
        .trim_start_matches("www.")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty()
        || host.contains(char::is_whitespace)
        || !host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'))
    {
        return None;
    }
    Some(host)
}

fn build_effective_query(query: &str, site_filters: &[String]) -> String {
    if site_filters.is_empty() {
        return query.to_string();
    }
    let site_expr = if site_filters.len() == 1 {
        format!("site:{}", site_filters[0])
    } else {
        site_filters
            .iter()
            .map(|site| format!("site:{site}"))
            .collect::<Vec<_>>()
            .join(" OR ")
    };
    format!("{site_expr} {query}")
}

fn truncate_string(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cutoff = text
        .char_indices()
        .nth(max_chars)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    text[..cutoff].to_string()
}

fn firecrawl_search_endpoint(config: &WebSearchFirecrawlConfig) -> Result<String> {
    let mut url = Url::parse(config.base_url().trim().trim_end_matches('/'))
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
    url.set_path("/v2/search");
    Ok(url.to_string())
}

fn resolve_firecrawl_api_key(config: &WebSearchFirecrawlConfig) -> Option<String> {
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

fn firecrawl_requires_api_key(config: &WebSearchFirecrawlConfig) -> bool {
    Url::parse(config.base_url().trim())
        .ok()
        .and_then(|url| {
            let host = url.host_str()?.to_ascii_lowercase();
            Some(url.scheme() == "https" && host == "api.firecrawl.dev")
        })
        .unwrap_or(true)
}

fn search_cache_key(
    query: &str,
    count: usize,
    max_result_chars: usize,
    scrape_results: bool,
    sources: &[String],
    categories: &[String],
    config: &WebSearchFirecrawlConfig,
) -> String {
    format!(
        "firecrawl-search|{}|{}|{}|{}|{}|{}|{}",
        config.base_url(),
        query,
        count,
        max_result_chars,
        scrape_results,
        sources.join(","),
        categories.join(",")
    )
}

fn read_search_cache(key: &str) -> Option<SearchPayload> {
    let cache = search_cache();
    if let Some(entry) = cache.get(key) {
        if Instant::now() <= entry.expires_at {
            return Some(entry.payload.clone());
        }
    }
    cache.remove(key);
    None
}

fn write_search_cache(key: &str, payload: SearchPayload, ttl_secs: u64) {
    if ttl_secs == 0 {
        return;
    }
    search_cache().insert(
        key.to_string(),
        SearchCacheEntry {
            expires_at: Instant::now() + Duration::from_secs(ttl_secs),
            payload,
        },
    );
}

#[derive(Debug, Clone)]
struct SearchCacheEntry {
    expires_at: Instant,
    payload: SearchPayload,
}

fn search_cache() -> &'static DashMap<String, SearchCacheEntry> {
    static CACHE: std::sync::OnceLock<DashMap<String, SearchCacheEntry>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn firecrawl_client() -> Result<&'static reqwest::Client> {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let client = reqwest::Client::builder()
        .build()
        .map_err(|err| anyhow!(err.to_string()))?;
    let _ = CLIENT.set(client);
    CLIENT
        .get()
        .ok_or_else(|| anyhow!("firecrawl search client initialization failed"))
}

#[cfg(test)]
mod tests {
    use super::{
        build_effective_query, build_firecrawl_search_body, firecrawl_requires_api_key,
        firecrawl_search_endpoint, normalize_site_filters, parse_firecrawl_search_items,
        resolve_count, site_name, truncate_string,
    };
    use crate::config::WebSearchToolConfig;
    use serde_json::json;

    #[test]
    fn count_is_clamped() {
        assert_eq!(resolve_count(None, 5), 5);
        assert_eq!(resolve_count(Some(0), 5), 1);
        assert_eq!(resolve_count(Some(30), 5), 10);
    }

    #[test]
    fn endpoint_uses_search_path() {
        let mut config = WebSearchToolConfig::default().firecrawl;
        assert_eq!(
            firecrawl_search_endpoint(&config).expect("endpoint"),
            "https://api.firecrawl.dev/v2/search"
        );
        assert!(firecrawl_requires_api_key(&config));

        config.base_url = "http://wunder-firecrawl:3002".to_string();
        assert_eq!(
            firecrawl_search_endpoint(&config).expect("endpoint"),
            "http://wunder-firecrawl:3002/v2/search"
        );
        assert!(!firecrawl_requires_api_key(&config));
    }

    #[test]
    fn request_body_supports_optional_scraping() {
        let body = build_firecrawl_search_body(
            "test query",
            3,
            true,
            &["web".to_string()],
            &["github".to_string()],
        );
        assert_eq!(body["query"], json!("test query"));
        assert_eq!(body["limit"], json!(3));
        assert_eq!(body["sources"], json!(["web"]));
        assert_eq!(body["categories"], json!(["github"]));
        assert_eq!(body["scrapeOptions"]["formats"], json!(["markdown"]));
    }

    #[test]
    fn site_filters_are_normalized_into_query() {
        let sites = normalize_site_filters(
            Some("https://www.example.com/docs?q=1"),
            Some(&["site:github.com".to_string(), "EXAMPLE.com".to_string()]),
        );
        assert_eq!(sites, vec!["example.com", "github.com"]);
        assert_eq!(
            build_effective_query("release notes", &sites),
            "site:example.com OR site:github.com release notes"
        );
    }

    #[test]
    fn parses_common_firecrawl_result_shapes() {
        let payload = json!({
            "success": true,
            "data": {
                "results": [{
                    "title": "Example",
                    "url": "https://www.example.com/docs",
                    "description": "Snippet",
                    "markdown": "Long content",
                    "metadata": {"publishedDate": "2026-05-15"}
                }]
            }
        });
        let items = parse_firecrawl_search_items(&payload, 200);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Example");
        assert_eq!(items[0].url, "https://www.example.com/docs");
        assert_eq!(items[0].site_name.as_deref(), Some("example.com"));
        assert_eq!(items[0].content.as_deref(), Some("Long content"));
    }

    #[test]
    fn helper_truncates_on_char_boundary() {
        assert_eq!(truncate_string("你好abc", 3), "你好a");
        assert_eq!(
            site_name("https://www.example.com/a").as_deref(),
            Some("example.com")
        );
    }
}
