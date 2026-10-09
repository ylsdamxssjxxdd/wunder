use super::{
    build_model_tool_success, tool_error::build_failed_tool_result, tool_error::ToolErrorMeta,
    ToolContext,
};
use crate::config::{Config, WebSearchFirecrawlConfig, WebSearchToolConfig};
use crate::i18n;
use anyhow::{anyhow, Result};
use dashmap::DashMap;
use futures::future::join_all;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use url::Url;

pub const TOOL_WEB_SEARCH: &str = "网页搜索";
pub const TOOL_WEB_SEARCH_ALIAS: &str = "web_search";

/// Upper bound on the number of queries accepted in a single call, mirroring
/// the dsh web seam's `maxQueries` default. Extra queries are dropped (not an
/// error) so a batch never fails just because it was batched too widely.
const MAX_QUERIES: usize = 4;
const MIN_COUNT: usize = 1;
const MAX_COUNT: usize = 10;
const MIN_MAX_RESULT_CHARS: usize = 120;
const MAX_MAX_RESULT_CHARS: usize = 4_000;

#[derive(Debug, Deserialize)]
struct WebSearchArgs {
    /// dsh-aligned batch input: one or more independent queries executed
    /// concurrently and merged. Preferred over the single-query `query` alias.
    #[serde(default)]
    queries: Option<Vec<String>>,
    /// Single-query alias kept for compatibility with earlier callers and the
    /// historical `query` parameter. Ignored when `queries` carries values.
    #[serde(default)]
    query: Option<String>,
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
    config.tools.web.search.enabled && config.tools.web.search.provider() == "firecrawl"
}

/// Resolve the outgoing query batch from `queries` (preferred) or the
/// single-query `query` alias. Blank entries are dropped, duplicates removed,
/// and the batch is capped at [`MAX_QUERIES`] so it always matches the dsh
/// "1..maxQueries" contract without hard-failing on over-wide batches.
fn resolve_queries(request: &WebSearchArgs) -> Vec<String> {
    let mut raw: Vec<String> = Vec::new();
    if let Some(queries) = request.queries.as_ref() {
        raw.extend(queries.iter().cloned());
    }
    if raw.iter().all(|value| value.trim().is_empty()) {
        if let Some(query) = request.query.as_ref() {
            raw.push(query.clone());
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut resolved: Vec<String> = Vec::new();
    for value in raw {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            resolved.push(trimmed.to_string());
        }
        if resolved.len() >= MAX_QUERIES {
            break;
        }
    }
    resolved
}

pub async fn tool_web_search(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let request: WebSearchArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let config = &context.config.tools.web.search;
    let queries = resolve_queries(&request);
    if queries.is_empty() {
        return Ok(web_search_failure(
            "",
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
        "firecrawl" => {
            // Run every query concurrently (dsh runs the batch with
            // Promise.allSettled); the first failure wins and the batch
            // short-circuits, matching the seam's first-error semantics.
            let futures = queries
                .iter()
                .map(|query| search_with_firecrawl(query, &request, config));
            let outcomes = join_all(futures).await;
            let mut payloads = Vec::with_capacity(outcomes.len());
            for outcome in outcomes {
                match outcome {
                    Ok(payload) => payloads.push(payload),
                    Err(failure) => return Ok(failure.into_value()),
                }
            }
            let cap = resolve_count(request.count, config.count);
            Ok(build_search_result(payloads, &queries, cap))
        }
        _ => Ok(web_search_failure(
            &queries.join(" | "),
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

/// Round-robin merge across query payloads: take rank 0 from every query, then
/// rank 1, and so on, de-duplicating by normalized URL and truncating to `cap`.
/// Returns the merged items plus whether truncation dropped any result, so the
/// caller can mark `truncated` exactly like the dsh web seam does.
fn merge_search_items(payloads: &[SearchPayload], cap: usize) -> (Vec<SearchResultItem>, bool) {
    let max_len = payloads.iter().map(|payload| payload.results.len()).max().unwrap_or(0);
    let mut items: Vec<SearchResultItem> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for rank in 0..max_len {
        for payload in payloads {
            if let Some(item) = payload.results.get(rank) {
                let key = item.url.trim().to_ascii_lowercase();
                if seen.insert(key) {
                    items.push(item.clone());
                }
            }
        }
    }
    let truncated = items.len() > cap;
    if truncated {
        items.truncate(cap);
    }
    (items, truncated)
}

/// Render per-query sections (`### <query>` then markdown links with snippets),
/// available only for multi-query batches so single-query callers keep the
/// leaner structured payload.
fn render_query_sections(queries: &[String], payloads: &[SearchPayload]) -> String {
    let mut sections: Vec<String> = Vec::new();
    for (index, query) in queries.iter().enumerate() {
        let mut section = format!("### {query}");
        if let Some(payload) = payloads.get(index) {
            for item in &payload.results {
                let label = if item.title.trim().is_empty() {
                    item.url.clone()
                } else {
                    item.title.clone()
                };
                match item.description.as_ref().map(|value| value.trim()) {
                    Some(snippet) if !snippet.is_empty() => {
                        section.push_str(&format!("\n- [{}]({}): {}", label, item.url, snippet));
                    }
                    _ => {
                        section.push_str(&format!("\n- [{}]({})", label, item.url));
                    }
                }
            }
        }
        sections.push(section);
    }
    sections.join("\n\n")
}

fn build_search_result(payloads: Vec<SearchPayload>, queries: &[String], cap: usize) -> Value {
    let (items, truncated) = merge_search_items(&payloads, cap);
    let count = items.len();
    let next_step_hint = if count == 0 {
        "No search results were returned. Do not guess URLs or fabricate sources; retry with a narrower query, change provider settings, or report that web search returned no evidence."
    } else if truncated {
        "Showing the first results only. Refine the query for more, then cite the relevant URLs above as markdown links in your answer."
    } else {
        "Pick concrete result URLs and call web_fetch for source pages that need verification. Cite the relevant URLs above as markdown links in your answer."
    };
    let first = payloads.first();
    let provider = first
        .map(|payload| payload.provider.clone())
        .unwrap_or_else(|| "firecrawl".to_string());
    let cached = !payloads.is_empty() && payloads.iter().all(|payload| payload.cached);
    let took_ms = payloads.iter().map(|payload| payload.took_ms).max().unwrap_or(0);
    let searched_at = payloads
        .iter()
        .map(|payload| payload.searched_at.clone())
        .max()
        .unwrap_or_default();
    let scrape_results = payloads.iter().any(|payload| payload.scrape_results);
    let effective_query = payloads
        .iter()
        .map(|payload| payload.effective_query.clone())
        .collect::<Vec<_>>()
        .join(" | ");
    let mut site_filters: Vec<String> = Vec::new();
    for payload in &payloads {
        for filter in &payload.site_filters {
            if !site_filters.iter().any(|existing| existing == filter) {
                site_filters.push(filter.clone());
            }
        }
    }

    let results_json = items
        .iter()
        .map(|item| {
            json!({
                "title": item.title,
                "url": item.url,
                "description": item.description,
                "content": item.content,
                "published": item.published,
                "site_name": item.site_name,
            })
        })
        .collect::<Vec<_>>();
    let sources_json = items
        .iter()
        .map(|item| {
            json!({
                "url": item.url,
                "title": item.title,
                "snippet": item.description,
                "publishedAt": item.published,
            })
        })
        .collect::<Vec<_>>();

    let mut data = Map::new();
    data.insert("query".to_string(), Value::String(queries.join(" | ")));
    data.insert("queries".to_string(), json!(queries));
    data.insert(
        "effective_query".to_string(),
        Value::String(effective_query),
    );
    data.insert("provider".to_string(), Value::String(provider));
    data.insert("count".to_string(), json!(count));
    data.insert("truncated".to_string(), json!(truncated));
    data.insert("cached".to_string(), json!(cached));
    data.insert("took_ms".to_string(), json!(took_ms));
    data.insert("scrape_results".to_string(), json!(scrape_results));
    data.insert("searched_at".to_string(), Value::String(searched_at));
    data.insert("site_filters".to_string(), json!(site_filters));
    data.insert("results".to_string(), Value::Array(results_json));
    data.insert("sources".to_string(), Value::Array(sources_json));
    if queries.len() > 1 {
        data.insert(
            "content".to_string(),
            Value::String(render_query_sections(queries, &payloads)),
        );
    }
    data.insert(
        "next_step_hint".to_string(),
        Value::String(next_step_hint.to_string()),
    );

    build_model_tool_success(
        "web_search",
        "completed",
        format!("Found {count} web results."),
        Value::Object(data),
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
        build_effective_query, build_firecrawl_search_body, build_search_result,
        firecrawl_requires_api_key, firecrawl_search_endpoint, merge_search_items,
        normalize_site_filters, parse_firecrawl_search_items, resolve_count, resolve_queries,
        site_name, truncate_string, SearchPayload, SearchResultItem, WebSearchArgs,
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

    fn search_item(title: &str, url: &str, description: Option<&str>) -> SearchResultItem {
        SearchResultItem {
            title: title.to_string(),
            url: url.to_string(),
            description: description.map(str::to_string),
            content: None,
            published: None,
            site_name: None,
        }
    }

    fn search_payload(query: &str, results: Vec<SearchResultItem>) -> SearchPayload {
        SearchPayload {
            query: query.to_string(),
            effective_query: query.to_string(),
            provider: "firecrawl".to_string(),
            count: results.len(),
            cached: false,
            took_ms: 1,
            scrape_results: false,
            searched_at: "2026-05-15T00:00:00Z".to_string(),
            site_filters: Vec::new(),
            results,
        }
    }

    // dsh `parseSearchArgs`: a batch is preferred over the single alias, blanks
    // are dropped, duplicates collapse, and the batch never exceeds maxQueries.
    #[test]
    fn queries_prefer_batch_and_dedupe_over_single_alias() {
        let args: WebSearchArgs = serde_json::from_value(json!({
            "queries": ["rust", " rust ", "Rust", "", "async"],
            "query": "ignored-when-batch-present"
        }))
        .expect("args");
        assert_eq!(resolve_queries(&args), vec!["rust", "Rust", "async"]);
    }

    #[test]
    fn queries_cap_batch_and_fall_back_to_single_alias() {
        // Over-wide batches are truncated to maxQueries, not rejected.
        let wide: WebSearchArgs = serde_json::from_value(json!({
            "queries": ["q1", "q2", "q3", "q4", "q5"]
        }))
        .expect("args");
        assert_eq!(resolve_queries(&wide), vec!["q1", "q2", "q3", "q4"]);

        // An all-blank batch falls back to the historical `query` alias.
        let legacy: WebSearchArgs = serde_json::from_value(json!({
            "queries": ["", "   "],
            "query": "legacy query"
        }))
        .expect("args");
        assert_eq!(resolve_queries(&legacy), vec!["legacy query"]);

        // No usable query at all resolves to an empty batch.
        let empty: WebSearchArgs =
            serde_json::from_value(json!({ "queries": ["", ""] })).expect("args");
        assert!(resolve_queries(&empty).is_empty());
    }

    // dsh `runSearchQueries`: results are merged round-robin by rank and
    // de-duplicated by URL (case-insensitive) across queries.
    #[test]
    fn merge_round_robins_and_dedupes_by_url() {
        let payloads = vec![
            search_payload(
                "a",
                vec![
                    search_item("A0", "https://a.test/0", Some("s0")),
                    search_item("A1", "https://shared.test/x", None),
                ],
            ),
            search_payload(
                "b",
                vec![
                    search_item("B0", "https://b.test/0", None),
                    search_item("B1", "https://SHARED.test/x", Some("dup")),
                ],
            ),
        ];
        let (items, truncated) = merge_search_items(&payloads, 10);
        assert!(!truncated);
        let urls: Vec<&str> = items.iter().map(|item| item.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://a.test/0",
                "https://b.test/0",
                "https://shared.test/x"
            ]
        );
    }

    #[test]
    fn merge_truncates_and_flags_when_over_cap() {
        let payloads = vec![
            search_payload(
                "a",
                vec![
                    search_item("A0", "https://a.test/0", None),
                    search_item("A1", "https://a.test/1", None),
                ],
            ),
            search_payload("b", vec![search_item("B0", "https://b.test/0", None)]),
        ];
        let (items, truncated) = merge_search_items(&payloads, 2);
        let urls: Vec<&str> = items.iter().map(|item| item.url.as_str()).collect();
        assert_eq!(urls, vec!["https://a.test/0", "https://b.test/0"]);
        assert!(truncated);
    }

    // dsh seam result shape: `{content?, sources:[{url,title?,snippet?,publishedAt?}], truncated}`.
    // Multi-query runs keep per-query markdown sections and always nudge the
    // model to cite sources as markdown links.
    #[test]
    fn build_search_result_exposes_sources_and_query_sections() {
        let payloads = vec![
            search_payload(
                "alpha",
                vec![search_item("Alpha", "https://a.test", Some("snippet"))],
            ),
            search_payload("beta", vec![search_item("Beta", "https://b.test", None)]),
        ];
        let queries = vec!["alpha".to_string(), "beta".to_string()];
        let value = build_search_result(payloads, &queries, 8);
        let data = &value["data"];
        assert_eq!(data["queries"], json!(["alpha", "beta"]));
        assert_eq!(data["count"], json!(2));
        assert_eq!(data["truncated"], json!(false));
        assert_eq!(data["sources"][0]["url"], json!("https://a.test"));
        assert_eq!(data["sources"][0]["snippet"], json!("snippet"));
        let content = data["content"].as_str().expect("multi-query content");
        assert!(content.contains("### alpha"));
        assert!(content.contains("### beta"));
        assert!(content.contains("[Alpha](https://a.test): snippet"));
        assert!(content.contains("[Beta](https://b.test)"));
        assert!(data["next_step_hint"]
            .as_str()
            .expect("hint")
            .contains("markdown links"));
    }

    #[test]
    fn single_query_result_omits_query_sections() {
        let payloads = vec![search_payload(
            "solo",
            vec![search_item("S", "https://s.test", None)],
        )];
        let value = build_search_result(payloads, &["solo".to_string()], 8);
        assert!(value["data"].get("content").is_none());
        assert_eq!(value["data"]["queries"], json!(["solo"]));
    }

    #[test]
    fn empty_result_hint_discourages_fabrication() {
        let value = build_search_result(Vec::new(), &["none".to_string()], 8);
        assert_eq!(value["data"]["count"], json!(0));
        let hint = value["data"]["next_step_hint"].as_str().expect("hint");
        assert!(hint.contains("No search results"));
    }

    #[test]
    fn truncated_result_hint_asks_to_refine() {
        let payloads = vec![
            search_payload(
                "a",
                vec![
                    search_item("A0", "https://a.test/0", None),
                    search_item("A1", "https://a.test/1", None),
                ],
            ),
            search_payload("b", vec![search_item("B0", "https://b.test/0", None)]),
        ];
        let value = build_search_result(payloads, &["a".to_string(), "b".to_string()], 2);
        assert_eq!(value["data"]["truncated"], json!(true));
        let hint = value["data"]["next_step_hint"].as_str().expect("hint");
        assert!(hint.contains("Refine the query"));
    }
}
