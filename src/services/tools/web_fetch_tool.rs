use super::{tool_error::build_failed_tool_result, tool_error::ToolErrorMeta, ToolContext};
use crate::config::{Config, WebFetchToolConfig};
use crate::i18n;
use crate::services::browser::{browser_service, browser_tools_enabled, BrowserSessionScope};
use anyhow::{anyhow, Result};
use dashmap::DashMap;
use encoding_rs::{Encoding, GBK};
use hickory_resolver::TokioAsyncResolver;
use kuchiki::traits::TendrilSink;
use kuchiki::NodeRef;
use regex::Regex;
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, LOCATION, USER_AGENT};
use reqwest::redirect::Policy;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use url::Url;

pub const TOOL_WEB_FETCH: &str = "网页抓取";
pub const TOOL_WEB_FETCH_ALIAS: &str = "web_fetch";

const MIN_MAX_CHARS: usize = 100;
const MAX_MAX_REDIRECTS: usize = 10;
const MIN_MAX_RESPONSE_BYTES: usize = 32 * 1024;
const MAX_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_ERROR_DETAIL_CHARS: usize = 600;
const MIN_PRIMARY_CONTENT_CHARS: usize = 240;
const MIN_DYNAMIC_PAGE_CONTENT_CHARS: usize = 320;
const DEFAULT_ACCEPT_LANGUAGE_VALUE: &str = "zh-CN,zh;q=0.9,en;q=0.8";
const DEFAULT_ACCEPT_VALUE: &str =
    "text/html, text/plain, application/json, text/markdown;q=0.9, */*;q=0.1";

const PRIMARY_CONTENT_SELECTORS: &[(&str, f64)] = &[
    ("article", 240.0),
    ("main", 220.0),
    ("[role='main']", 220.0),
    ("#content", 210.0),
    ("#main", 210.0),
    (".content", 180.0),
    (".main-content", 180.0),
    (".article", 180.0),
    (".article-body", 180.0),
    (".post-content", 180.0),
    (".entry-content", 180.0),
    (".markdown-body", 180.0),
    (".prose", 160.0),
    (".doc-content", 160.0),
    (".docs-content", 160.0),
];

const ALWAYS_DROP_SELECTORS: &[&str] = &[
    "script", "style", "noscript", "template", "svg", "canvas", "iframe", "object", "embed",
    "meta", "link",
];

const STRONG_NOISE_KEYWORDS: &[&str] = &[
    "breadcrumb",
    "breadcrumbs",
    "share",
    "sharing",
    "social",
    "related",
    "recommend",
    "recommended",
    "pagination",
    "pager",
    "subscribe",
    "signup",
    "sign-up",
    "newsletter",
    "cookie",
    "consent",
    "comment",
    "comments",
    "sidebar",
    "toolbar",
    "footer",
    "advert",
    "ads",
    "promo",
    "login",
    "register",
    "搜索",
    "面包屑",
    "相关推荐",
    "推荐阅读",
    "相关文章",
    "评论",
    "登录",
    "注册",
    "订阅",
    "版权",
    "分享",
    "侧边栏",
];

const POSITIVE_HINT_KEYWORDS: &[&str] = &[
    "article", "content", "main", "post", "entry", "detail", "doc", "docs", "markdown", "prose",
    "正文", "内容", "文章", "详情", "文档",
];

const NOISE_BLOCK_PHRASES: &[&str] = &[
    "all rights reserved",
    "privacy policy",
    "terms of service",
    "cookie policy",
    "sign in",
    "sign up",
    "log in",
    "subscribe",
    "related articles",
    "recommended for you",
    "share this",
    "back to top",
    "上一篇",
    "下一篇",
    "相关推荐",
    "推荐阅读",
    "相关文章",
    "返回顶部",
    "版权所有",
    "登录",
    "注册",
    "订阅",
    "分享",
];

const DYNAMIC_PAGE_MARKERS: &[&str] = &[
    "__next",
    "__next_data__",
    "__nuxt",
    "data-reactroot",
    "reactdom",
    "hydrateroot",
    "createroot(",
    "id=\"app\"",
    "id='app'",
    "id=\"root\"",
    "id='root'",
    "vite",
    "webpack",
    "client-side rendering",
    "enable javascript",
];

const BOT_PROTECTION_PHRASES: &[&str] = &[
    "access denied",
    "attention required",
    "bot verification",
    "captcha",
    "checking your browser",
    "just a moment",
    "please enable cookies",
    "request blocked",
    "security check",
    "verify you are human",
];

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ExtractMode {
    Markdown,
    Text,
}

impl ExtractMode {
    fn from_raw(raw: Option<&str>) -> Self {
        match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
            "text" => Self::Text,
            _ => Self::Markdown,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Text => "text",
        }
    }
}

#[derive(Debug, Deserialize)]
struct WebFetchArgs {
    url: String,
    #[serde(default, alias = "extractMode")]
    extract_mode: Option<String>,
    #[serde(default, alias = "maxChars")]
    max_chars: Option<usize>,
}

#[derive(Debug)]
struct FetchedResponse {
    final_url: String,
    status: u16,
    content_type: Option<String>,
    body: Vec<u8>,
    body_truncated: bool,
}

#[derive(Debug, Clone)]
struct CachedPayload {
    final_url: String,
    status: u16,
    content_type: String,
    title: Option<String>,
    extractor: String,
    content_kind: String,
    fetch_strategy: String,
    content: String,
    warning: Option<String>,
    fetched_at: String,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    expires_at: Instant,
    payload: CachedPayload,
}

#[derive(Debug)]
struct HtmlExtraction {
    title: Option<String>,
    content: String,
    extractor: String,
}

#[derive(Debug, Clone)]
struct HtmlPageDiagnosis {
    kind: HtmlPageKind,
    reason: String,
}

#[derive(Debug, Clone)]
struct BrowserFallbackRequest<'a> {
    raw_url: &'a str,
    request_url: &'a Url,
    status: u16,
    content_type: &'a str,
    max_chars: usize,
    warning: Option<String>,
    diagnosis: &'a HtmlPageDiagnosis,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum HtmlPageKind {
    DynamicPage,
    BotProtection,
}

impl HtmlPageKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::DynamicPage => "dynamic_page",
            Self::BotProtection => "bot_protection",
        }
    }

    fn retry_with_browser(self) -> bool {
        matches!(self, Self::DynamicPage)
    }
}

#[derive(Debug, Default)]
struct CandidateStats {
    text_chars: usize,
    punctuation_count: usize,
    link_text_chars: usize,
    paragraph_count: usize,
    pre_count: usize,
    heading_count: usize,
    positive_hint: bool,
    negative_hint: bool,
}

#[derive(Debug)]
struct WebFetchFailure {
    message: String,
    data: Value,
    meta: ToolErrorMeta,
}

impl WebFetchFailure {
    fn into_value(self) -> Value {
        build_failed_tool_result(self.message, self.data, self.meta, false)
    }
}

#[allow(clippy::too_many_arguments)]
fn web_fetch_failure(
    raw_url: &str,
    url: Option<&Url>,
    phase: &str,
    code: &str,
    message: impl Into<String>,
    hint: Option<String>,
    retryable: bool,
    retry_after_ms: Option<u64>,
    extra: Value,
) -> WebFetchFailure {
    let message = message.into();
    let mut data = Map::new();
    data.insert("url".to_string(), Value::String(raw_url.to_string()));
    data.insert("phase".to_string(), Value::String(phase.to_string()));
    data.insert(
        "failure_summary".to_string(),
        Value::String(message.clone()),
    );
    data.insert(
        "error_detail_head".to_string(),
        Value::String(message.clone()),
    );
    if let Some(url) = url {
        data.insert("normalized_url".to_string(), Value::String(url.to_string()));
        if let Some(host) = url.host_str() {
            data.insert("host".to_string(), Value::String(host.to_string()));
        }
    }
    if let Some(text) = hint.clone().filter(|value| !value.trim().is_empty()) {
        data.insert("next_step_hint".to_string(), Value::String(text));
    }
    if let Value::Object(map) = extra {
        data.extend(map);
    }
    WebFetchFailure {
        message,
        data: Value::Object(data),
        meta: ToolErrorMeta::new(code, hint, retryable, retry_after_ms),
    }
}

pub fn is_web_fetch_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    if cleaned == TOOL_WEB_FETCH {
        return true;
    }
    cleaned.eq_ignore_ascii_case(TOOL_WEB_FETCH_ALIAS)
}

pub fn web_fetch_enabled(config: &Config) -> bool {
    config.tools.web.fetch.enabled
}

pub async fn tool_web_fetch(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let request: WebFetchArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let raw_url = request.url.trim().to_string();
    let request_url = match normalize_request_url(&request.url) {
        Ok(value) => value,
        Err(failure) => return Ok(failure.into_value()),
    };
    let config = &context.config.tools.web.fetch;
    let extract_mode = ExtractMode::from_raw(request.extract_mode.as_deref());
    let max_chars = resolve_max_chars(request.max_chars, config);
    let cache_key = format!("{request_url}|{}", extract_mode.as_str());

    if let Some(entry) = read_cache_entry(&cache_key) {
        return Ok(build_tool_result(
            &raw_url,
            &entry,
            extract_mode,
            max_chars,
            true,
        ));
    }

    let fetched = match fetch_url(&raw_url, &request_url, config).await {
        Ok(value) => value,
        Err(failure) => return Ok(failure.into_value()),
    };
    let decoded = decode_body_text(&fetched.body, fetched.content_type.as_deref());
    let content_type = normalize_content_type(fetched.content_type.as_deref())
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let warning = fetched.body_truncated.then(|| {
        format!(
            "Response body truncated after {} bytes.",
            config.max_response_bytes
        )
    });

    let payload = if !status_is_success(fetched.status) {
        let detail = truncate_chars(
            &extract_error_detail(&decoded, &content_type),
            MAX_ERROR_DETAIL_CHARS,
        )
        .0;
        let message = if detail.is_empty() {
            let mut params = HashMap::new();
            params.insert("status".to_string(), fetched.status.to_string());
            i18n::t_with_params("tool.web_fetch.http_error", &params)
        } else {
            let mut params = HashMap::new();
            params.insert("status".to_string(), fetched.status.to_string());
            params.insert("detail".to_string(), detail);
            i18n::t_with_params("tool.web_fetch.http_error_with_detail", &params)
        };
        return Ok(web_fetch_failure(
            &raw_url,
            Some(&request_url),
            "response_status",
            "TOOL_WEB_FETCH_HTTP_ERROR",
            message,
            Some("Check the status/detail or try a different public source.".to_string()),
            (500..600).contains(&fetched.status),
            None,
            json!({
                "status": fetched.status,
                "content_type": content_type,
                "final_url": fetched.final_url,
            }),
        )
        .into_value());
    } else if is_html_content_type(&content_type) || looks_like_html(&decoded) {
        let html = match extract_html_content(&decoded, extract_mode) {
            Ok(value) => value,
            Err(err) => {
                return Ok(
                    web_fetch_failure(
                        &raw_url,
                        Some(&request_url),
                        "extract",
                        "TOOL_WEB_FETCH_NO_CONTENT",
                        err.to_string(),
                        Some(
                            "Try extract_mode=text, a simpler page, or the browser tool for dynamic pages."
                                .to_string(),
                        ),
                        false,
                        None,
                        json!({
                            "status": fetched.status,
                            "content_type": content_type,
                            "final_url": fetched.final_url,
                        }),
                    )
                    .into_value(),
                );
            }
        };
        if let Some(diagnosis) = diagnose_html_page(
            &decoded,
            html.title.as_deref(),
            &html.content,
            &html.extractor,
        ) {
            if diagnosis.kind.retry_with_browser() && browser_tools_enabled(context.config) {
                match fetch_with_browser_fallback(
                    context,
                    BrowserFallbackRequest {
                        raw_url: &raw_url,
                        request_url: &request_url,
                        status: fetched.status,
                        content_type: &content_type,
                        max_chars,
                        warning: warning.clone(),
                        diagnosis: &diagnosis,
                    },
                )
                .await
                {
                    Ok(payload) => payload,
                    Err(failure) => return Ok(failure.into_value()),
                }
            } else {
                let message_key = match diagnosis.kind {
                    HtmlPageKind::DynamicPage => "tool.web_fetch.dynamic_page",
                    HtmlPageKind::BotProtection => "tool.web_fetch.bot_protection",
                };
                let hint = match diagnosis.kind {
                    HtmlPageKind::DynamicPage => Some(
                        "This page appears to rely on client-side rendering. Use the browser tool or another source with stable public HTML."
                            .to_string(),
                    ),
                    HtmlPageKind::BotProtection => Some(
                        "This page appears to be protected by verification or anti-bot controls. Try another public source or open it in a browser."
                            .to_string(),
                    ),
                };
                return Ok(web_fetch_failure(
                    &raw_url,
                    Some(&request_url),
                    "extract",
                    match diagnosis.kind {
                        HtmlPageKind::DynamicPage => "TOOL_WEB_FETCH_DYNAMIC_PAGE",
                        HtmlPageKind::BotProtection => "TOOL_WEB_FETCH_BOT_PROTECTION",
                    },
                    i18n::t(message_key),
                    hint,
                    false,
                    None,
                    json!({
                        "status": fetched.status,
                        "content_type": content_type,
                        "final_url": fetched.final_url,
                        "page_kind": diagnosis.kind.as_str(),
                        "diagnosis": diagnosis.reason,
                        "browser_fallback_available": browser_tools_enabled(context.config),
                    }),
                )
                .into_value());
            }
        } else {
            CachedPayload {
                final_url: fetched.final_url,
                status: fetched.status,
                content_type,
                title: html.title,
                extractor: html.extractor,
                content_kind: "html".to_string(),
                fetch_strategy: "direct_http".to_string(),
                content: html.content,
                warning: warning.clone(),
                fetched_at: chrono::Utc::now().to_rfc3339(),
            }
        }
    } else if is_json_content_type(&content_type) {
        let content = match format_json_content(&decoded, extract_mode) {
            Ok(value) => value,
            Err(err) => {
                return Ok(
                    web_fetch_failure(
                        &raw_url,
                        Some(&request_url),
                        "parse",
                        "TOOL_WEB_FETCH_PARSE_FAILED",
                        err.to_string(),
                        Some(
                            "The response could not be decoded as valid JSON. Try extract_mode=text or another URL."
                                .to_string(),
                        ),
                        false,
                        None,
                        json!({
                            "status": fetched.status,
                            "content_type": content_type,
                            "final_url": fetched.final_url,
                        }),
                    )
                    .into_value(),
                );
            }
        };
        CachedPayload {
            final_url: fetched.final_url,
            status: fetched.status,
            content_type,
            title: None,
            extractor: "json".to_string(),
            content_kind: "json".to_string(),
            fetch_strategy: "direct_http".to_string(),
            content,
            warning: warning.clone(),
            fetched_at: chrono::Utc::now().to_rfc3339(),
        }
    } else if is_text_like_content_type(&content_type) || looks_like_text(&fetched.body) {
        CachedPayload {
            final_url: fetched.final_url,
            status: fetched.status,
            content_type,
            title: None,
            extractor: "text".to_string(),
            content_kind: "text".to_string(),
            fetch_strategy: "direct_http".to_string(),
            content: format_plain_text(&decoded),
            warning: warning.clone(),
            fetched_at: chrono::Utc::now().to_rfc3339(),
        }
    } else {
        let mut params = HashMap::new();
        params.insert("content_type".to_string(), content_type.clone());
        return Ok(
            web_fetch_failure(
                &raw_url,
                Some(&request_url),
                "content_type",
                "TOOL_WEB_FETCH_UNSUPPORTED_CONTENT_TYPE",
                i18n::t_with_params("tool.web_fetch.unsupported_content_type", &params),
                Some(
                    "Use web_fetch for HTML/text/JSON pages. Switch tools for binary downloads or interactive pages."
                        .to_string(),
                ),
                false,
                None,
                json!({
                    "status": fetched.status,
                    "content_type": content_type,
                    "final_url": fetched.final_url,
                }),
            )
            .into_value(),
        );
    };

    if payload.content.trim().is_empty() {
        return Ok(
            web_fetch_failure(
                &raw_url,
                Some(&request_url),
                "extract",
                "TOOL_WEB_FETCH_NO_CONTENT",
                i18n::t("tool.web_fetch.no_content"),
                Some(
                    "Try a different page, extract_mode=text, or the browser tool if content depends on client-side rendering."
                        .to_string(),
                ),
                false,
                None,
                json!({
                    "status": payload.status,
                    "content_type": payload.content_type,
                    "final_url": payload.final_url,
                }),
            )
            .into_value(),
        );
    }

    write_cache_entry(&cache_key, payload.clone(), config.cache_ttl_secs);
    Ok(build_tool_result(
        &raw_url,
        &payload,
        extract_mode,
        max_chars,
        false,
    ))
}

fn normalize_request_url(raw: &str) -> std::result::Result<Url, WebFetchFailure> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(web_fetch_failure(
            raw,
            None,
            "validation",
            "TOOL_WEB_FETCH_INVALID_URL",
            i18n::t("tool.web_fetch.invalid_url"),
            Some("Pass an absolute http:// or https:// URL.".to_string()),
            false,
            None,
            json!({}),
        ));
    }
    let url = Url::parse(trimmed).map_err(|_| {
        web_fetch_failure(
            raw,
            None,
            "validation",
            "TOOL_WEB_FETCH_INVALID_URL",
            i18n::t("tool.web_fetch.invalid_url"),
            Some("Pass an absolute http:// or https:// URL.".to_string()),
            false,
            None,
            json!({}),
        )
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(web_fetch_failure(
            raw,
            Some(&url),
            "validation",
            "TOOL_WEB_FETCH_UNSUPPORTED_SCHEME",
            i18n::t("tool.web_fetch.unsupported_scheme"),
            Some("Use http:// or https:// URLs only.".to_string()),
            false,
            None,
            json!({}),
        ));
    }
    if url.host_str().is_none() {
        return Err(web_fetch_failure(
            raw,
            Some(&url),
            "validation",
            "TOOL_WEB_FETCH_INVALID_URL",
            i18n::t("tool.web_fetch.invalid_url"),
            Some("Pass an absolute http:// or https:// URL with a host.".to_string()),
            false,
            None,
            json!({}),
        ));
    }
    Ok(url)
}

fn resolve_max_chars(requested: Option<usize>, config: &WebFetchToolConfig) -> usize {
    let cap = config.max_chars_cap.max(MIN_MAX_CHARS);
    let value = requested.unwrap_or(config.max_chars).max(MIN_MAX_CHARS);
    value.min(cap)
}

fn resolve_timeout_secs(config: &WebFetchToolConfig) -> u64 {
    config.timeout_secs.clamp(1, 120)
}

fn resolve_max_redirects(config: &WebFetchToolConfig) -> usize {
    config.max_redirects.min(MAX_MAX_REDIRECTS)
}

fn resolve_max_response_bytes(config: &WebFetchToolConfig) -> usize {
    config
        .max_response_bytes
        .clamp(MIN_MAX_RESPONSE_BYTES, MAX_MAX_RESPONSE_BYTES)
}

fn status_is_success(status: u16) -> bool {
    (200..300).contains(&status)
}

async fn fetch_url(
    raw_url: &str,
    url: &Url,
    config: &WebFetchToolConfig,
) -> std::result::Result<FetchedResponse, WebFetchFailure> {
    let client = web_fetch_client().map_err(|err| {
        web_fetch_failure(
            raw_url,
            Some(url),
            "request",
            "TOOL_WEB_FETCH_REQUEST_FAILED",
            format!("Failed to initialize HTTP client: {err}"),
            Some("Retry later or restart the service if the issue persists.".to_string()),
            true,
            Some(200),
            json!({}),
        )
    })?;
    let timeout_secs = resolve_timeout_secs(config);
    let max_redirects = resolve_max_redirects(config);
    let max_response_bytes = resolve_max_response_bytes(config);
    let user_agent = config.user_agent.trim();

    let mut current = url.clone();
    let mut visited = HashSet::new();

    for redirect_index in 0..=max_redirects {
        validate_remote_target(raw_url, &current, timeout_secs).await?;
        if !visited.insert(current.to_string()) {
            return Err(web_fetch_failure(
                raw_url,
                Some(&current),
                "redirect",
                "TOOL_WEB_FETCH_REDIRECT_LOOP",
                i18n::t("tool.web_fetch.redirect_loop"),
                Some("Use the final page URL directly if possible.".to_string()),
                false,
                None,
                json!({ "timeout_s": timeout_secs }),
            ));
        }

        let request = client
            .get(current.clone())
            .header(ACCEPT, DEFAULT_ACCEPT_VALUE)
            .header(ACCEPT_LANGUAGE, DEFAULT_ACCEPT_LANGUAGE_VALUE)
            .header(USER_AGENT, user_agent);
        let response = timeout(Duration::from_secs(timeout_secs), request.send())
            .await
            .map_err(|_| {
                web_fetch_failure(
                    raw_url,
                    Some(&current),
                    "request",
                    "TOOL_TIMEOUT",
                    i18n::t("tool.web_fetch.timeout"),
                    Some("Retry later or narrow to a faster public page.".to_string()),
                    true,
                    Some(timeout_secs.saturating_mul(1000)),
                    json!({ "timeout_s": timeout_secs }),
                )
            })?
            .map_err(|err| {
                web_fetch_failure(
                    raw_url,
                    Some(&current),
                    "request",
                    "TOOL_WEB_FETCH_REQUEST_FAILED",
                    format!("Request failed: {err}"),
                    Some("Check site reachability or retry with a simpler public URL.".to_string()),
                    err.is_timeout() || err.is_connect() || err.is_request(),
                    Some(timeout_secs.saturating_mul(1000)),
                    json!({ "timeout_s": timeout_secs }),
                )
            })?;

        if response.status().is_redirection() {
            if redirect_index >= max_redirects {
                return Err(web_fetch_failure(
                    raw_url,
                    Some(&current),
                    "redirect",
                    "TOOL_WEB_FETCH_TOO_MANY_REDIRECTS",
                    i18n::t("tool.web_fetch.too_many_redirects"),
                    Some(
                        "Use the final destination URL directly or reduce redirect hops."
                            .to_string(),
                    ),
                    false,
                    None,
                    json!({ "timeout_s": timeout_secs }),
                ));
            }
            let Some(location) = response.headers().get(LOCATION) else {
                return Err(web_fetch_failure(
                    raw_url,
                    Some(&current),
                    "redirect",
                    "TOOL_WEB_FETCH_REDIRECT_INVALID",
                    i18n::t("tool.web_fetch.redirect_missing_location"),
                    Some("Retry with the final URL directly.".to_string()),
                    false,
                    None,
                    json!({ "timeout_s": timeout_secs }),
                ));
            };
            let location = location.to_str().map_err(|_| {
                web_fetch_failure(
                    raw_url,
                    Some(&current),
                    "redirect",
                    "TOOL_WEB_FETCH_REDIRECT_INVALID",
                    i18n::t("tool.web_fetch.redirect_invalid_location"),
                    Some("Retry with the final URL directly.".to_string()),
                    false,
                    None,
                    json!({ "timeout_s": timeout_secs }),
                )
            })?;
            current = current.join(location).map_err(|_| {
                web_fetch_failure(
                    raw_url,
                    Some(&current),
                    "redirect",
                    "TOOL_WEB_FETCH_REDIRECT_INVALID",
                    i18n::t("tool.web_fetch.redirect_invalid_location"),
                    Some("Retry with the final URL directly.".to_string()),
                    false,
                    None,
                    json!({ "timeout_s": timeout_secs }),
                )
            })?;
            continue;
        }

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let status = response.status().as_u16();
        let (body, body_truncated) = timeout(
            Duration::from_secs(timeout_secs),
            read_response_bytes_limited(response, max_response_bytes),
        )
        .await
        .map_err(|_| {
            web_fetch_failure(
                raw_url,
                Some(&current),
                "response_body",
                "TOOL_TIMEOUT",
                i18n::t("tool.web_fetch.timeout"),
                Some("Retry later or fetch a smaller/faster page.".to_string()),
                true,
                Some(timeout_secs.saturating_mul(1000)),
                json!({ "timeout_s": timeout_secs }),
            )
        })?
        .map_err(|err| {
            web_fetch_failure(
                raw_url,
                Some(&current),
                "response_body",
                "TOOL_WEB_FETCH_REQUEST_FAILED",
                format!("Failed to read response body: {err}"),
                Some("Retry later or use another public source.".to_string()),
                true,
                Some(timeout_secs.saturating_mul(1000)),
                json!({ "timeout_s": timeout_secs }),
            )
        })?;
        return Ok(FetchedResponse {
            final_url: current.to_string(),
            status,
            content_type,
            body,
            body_truncated,
        });
    }

    Err(web_fetch_failure(
        raw_url,
        Some(&current),
        "redirect",
        "TOOL_WEB_FETCH_TOO_MANY_REDIRECTS",
        i18n::t("tool.web_fetch.too_many_redirects"),
        Some("Use the final destination URL directly or reduce redirect hops.".to_string()),
        false,
        None,
        json!({ "timeout_s": timeout_secs }),
    ))
}

async fn read_response_bytes_limited(
    mut response: reqwest::Response,
    max_bytes: usize,
) -> Result<(Vec<u8>, bool)> {
    let mut data = Vec::with_capacity(max_bytes.min(64 * 1024));
    let mut truncated = false;
    while let Some(chunk) = response.chunk().await? {
        let remaining = max_bytes.saturating_sub(data.len());
        if remaining == 0 {
            truncated = true;
            break;
        }
        if chunk.len() > remaining {
            data.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        data.extend_from_slice(&chunk);
    }
    Ok((data, truncated))
}

async fn validate_remote_target(
    raw_url: &str,
    url: &Url,
    timeout_secs: u64,
) -> std::result::Result<(), WebFetchFailure> {
    let host = url.host_str().ok_or_else(|| {
        web_fetch_failure(
            raw_url,
            Some(url),
            "validation",
            "TOOL_WEB_FETCH_INVALID_URL",
            i18n::t("tool.web_fetch.invalid_url"),
            Some("Pass an absolute http:// or https:// URL with a host.".to_string()),
            false,
            None,
            json!({}),
        )
    })?;
    let ascii_host = idna::domain_to_ascii(host).map_err(|_| {
        web_fetch_failure(
            raw_url,
            Some(url),
            "validation",
            "TOOL_WEB_FETCH_INVALID_URL",
            i18n::t("tool.web_fetch.invalid_url"),
            Some("Use a valid public host name.".to_string()),
            false,
            None,
            json!({}),
        )
    })?;

    if is_obviously_private_host(&ascii_host) {
        let mut params = HashMap::new();
        params.insert("host".to_string(), ascii_host);
        return Err(web_fetch_failure(
            raw_url,
            Some(url),
            "validation",
            "TOOL_WEB_FETCH_BLOCKED_HOST",
            i18n::t_with_params("tool.web_fetch.blocked_host", &params),
            Some("Use a public internet host. Private/internal targets are blocked.".to_string()),
            false,
            None,
            json!({}),
        ));
    }

    if let Ok(ip) = ascii_host.parse::<IpAddr>() {
        ensure_public_ip(raw_url, url, ip)?;
        return Ok(());
    }

    let resolver = TokioAsyncResolver::tokio_from_system_conf().map_err(|err| {
        web_fetch_failure(
            raw_url,
            Some(url),
            "dns_lookup",
            "TOOL_WEB_FETCH_DNS_FAILED",
            format!("{}: {err}", i18n::t("tool.web_fetch.dns_failed")),
            Some("Check DNS reachability or try another public host.".to_string()),
            true,
            Some(timeout_secs.saturating_mul(1000)),
            json!({ "timeout_s": timeout_secs }),
        )
    })?;
    let lookup = timeout(
        Duration::from_secs(timeout_secs),
        resolver.lookup_ip(ascii_host.clone()),
    )
    .await
    .map_err(|_| {
        web_fetch_failure(
            raw_url,
            Some(url),
            "dns_lookup",
            "TOOL_TIMEOUT",
            i18n::t("tool.web_fetch.timeout"),
            Some("DNS lookup timed out. Retry later or use another reachable host.".to_string()),
            true,
            Some(timeout_secs.saturating_mul(1000)),
            json!({ "timeout_s": timeout_secs }),
        )
    })?
    .map_err(|err| {
        web_fetch_failure(
            raw_url,
            Some(url),
            "dns_lookup",
            "TOOL_WEB_FETCH_DNS_FAILED",
            format!("{}: {err}", i18n::t("tool.web_fetch.dns_failed")),
            Some("Check DNS reachability or try another public host.".to_string()),
            true,
            Some(timeout_secs.saturating_mul(1000)),
            json!({ "timeout_s": timeout_secs }),
        )
    })?;
    let mut resolved_any = false;
    for ip in lookup.iter() {
        resolved_any = true;
        ensure_public_ip(raw_url, url, ip)?;
    }
    if !resolved_any {
        return Err(web_fetch_failure(
            raw_url,
            Some(url),
            "dns_lookup",
            "TOOL_WEB_FETCH_DNS_FAILED",
            i18n::t("tool.web_fetch.dns_failed"),
            Some("Check DNS reachability or try another public host.".to_string()),
            true,
            Some(timeout_secs.saturating_mul(1000)),
            json!({ "timeout_s": timeout_secs }),
        ));
    }
    Ok(())
}

fn ensure_public_ip(
    raw_url: &str,
    url: &Url,
    ip: IpAddr,
) -> std::result::Result<(), WebFetchFailure> {
    if ip_is_private_or_internal(ip) {
        let mut params = HashMap::new();
        params.insert("host".to_string(), ip.to_string());
        return Err(web_fetch_failure(
            raw_url,
            Some(url),
            "validation",
            "TOOL_WEB_FETCH_BLOCKED_HOST",
            i18n::t_with_params("tool.web_fetch.blocked_host", &params),
            Some("Use a public internet host. Private/internal targets are blocked.".to_string()),
            false,
            None,
            json!({ "resolved_ip": ip.to_string() }),
        ));
    }
    Ok(())
}

fn ip_is_private_or_internal(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || octets[0] == 0
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 198 && matches!(octets[1], 18 | 19))
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = ipv6_mapped_ipv4(v6) {
                return ip_is_private_or_internal(IpAddr::V4(mapped));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || is_ipv6_documentation(v6)
        }
    }
}

fn ipv6_mapped_ipv4(value: Ipv6Addr) -> Option<Ipv4Addr> {
    let segments = value.segments();
    if segments[..5] == [0, 0, 0, 0, 0] && matches!(segments[5], 0 | 0xffff) {
        let octets = value.octets();
        return Some(Ipv4Addr::new(
            octets[12], octets[13], octets[14], octets[15],
        ));
    }
    None
}

fn is_ipv6_documentation(value: Ipv6Addr) -> bool {
    let segments = value.segments();
    segments[0] == 0x2001 && segments[1] == 0x0db8
}

fn is_obviously_private_host(host: &str) -> bool {
    let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if normalized.is_empty() {
        return true;
    }
    if matches!(
        normalized.as_str(),
        "localhost" | "0.0.0.0" | "::1" | "[::1]" | "127.0.0.1"
    ) {
        return true;
    }
    normalized.ends_with(".local")
        || normalized.ends_with(".internal")
        || normalized.ends_with(".intranet")
        || !normalized.contains('.')
}

fn web_fetch_client() -> Result<&'static reqwest::Client> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }

    let client = reqwest::Client::builder()
        .redirect(Policy::none())
        .build()
        .map_err(|err| anyhow!(err.to_string()))?;
    let _ = CLIENT.set(client);
    CLIENT
        .get()
        .ok_or_else(|| anyhow!("web_fetch client initialization failed"))
}

fn normalize_content_type(raw: Option<&str>) -> Option<String> {
    raw.and_then(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

fn decode_body_text(bytes: &[u8], content_type: Option<&str>) -> String {
    if let Some(encoding) = detect_encoding(bytes, content_type) {
        let (decoded, _, _) = encoding.decode(bytes);
        return decoded.into_owned();
    }
    if let Ok(text) = String::from_utf8(bytes.to_vec()) {
        return text;
    }
    let (decoded, _, had_errors) = GBK.decode(bytes);
    if !had_errors {
        return decoded.into_owned();
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn detect_encoding(bytes: &[u8], content_type: Option<&str>) -> Option<&'static Encoding> {
    if let Some(content_type) = content_type {
        if let Some(label) = extract_charset_from_header(content_type) {
            if let Some(encoding) = Encoding::for_label(label.as_bytes()) {
                return Some(encoding);
            }
        }
    }
    extract_charset_from_html(bytes).and_then(|label| Encoding::for_label(label.as_bytes()))
}

fn extract_charset_from_header(content_type: &str) -> Option<String> {
    let lower = content_type.to_ascii_lowercase();
    let charset_index = lower.find("charset=")?;
    let value = &content_type[charset_index + "charset=".len()..];
    let cleaned = value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .trim_matches('\'');
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_string())
    }
}

fn extract_charset_from_html(bytes: &[u8]) -> Option<String> {
    let sample = &bytes[..bytes.len().min(4096)];
    let sample = String::from_utf8_lossy(sample);
    let captures = meta_charset_regex().captures(&sample)?;
    captures
        .get(1)
        .map(|value| value.as_str().trim().to_string())
        .filter(|value| !value.is_empty())
}

fn is_html_content_type(content_type: &str) -> bool {
    content_type.contains("text/html") || content_type.contains("application/xhtml+xml")
}

fn is_json_content_type(content_type: &str) -> bool {
    content_type.contains("application/json") || content_type.ends_with("+json")
}

fn is_text_like_content_type(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || content_type.contains("application/xml")
        || content_type.contains("text/xml")
}

fn looks_like_html(value: &str) -> bool {
    let trimmed = value.trim_start();
    let head = trimmed
        .chars()
        .take(256)
        .collect::<String>()
        .to_ascii_lowercase();
    head.starts_with("<!doctype html") || head.starts_with("<html") || head.contains("<body")
}

fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let sample = bytes.len().min(4096);
    let control = bytes
        .iter()
        .take(sample)
        .filter(|byte| **byte < 0x20 && !matches!(**byte, b'\n' | b'\r' | b'\t'))
        .count();
    (control as f64) / (sample as f64) < 0.12
}

fn format_json_content(text: &str, mode: ExtractMode) -> Result<String> {
    let parsed: Value =
        serde_json::from_str(text).map_err(|_| anyhow!(i18n::t("tool.web_fetch.invalid_json")))?;
    let pretty = serde_json::to_string_pretty(&parsed).map_err(|err| anyhow!(err.to_string()))?;
    Ok(match mode {
        ExtractMode::Markdown => format!("```json\n{pretty}\n```"),
        ExtractMode::Text => pretty,
    })
}

fn format_plain_text(text: &str) -> String {
    normalize_text_block(text)
}

fn extract_error_detail(decoded: &str, content_type: &str) -> String {
    if is_html_content_type(content_type) || looks_like_html(decoded) {
        extract_html_content(decoded, ExtractMode::Text)
            .map(|value| value.content)
            .unwrap_or_else(|_| normalize_text_block(decoded))
    } else {
        normalize_text_block(decoded)
    }
}

fn diagnose_html_page(
    html: &str,
    title: Option<&str>,
    content: &str,
    extractor: &str,
) -> Option<HtmlPageDiagnosis> {
    let title_text = title.unwrap_or("").trim();
    if let Some(reason) = detect_bot_protection_reason(title_text, content) {
        return Some(HtmlPageDiagnosis {
            kind: HtmlPageKind::BotProtection,
            reason,
        });
    }

    let html_lower = html.to_ascii_lowercase();
    let content_chars = content.chars().count();
    let script_tag_count = html_lower.matches("<script").count();
    let dynamic_marker_count = DYNAMIC_PAGE_MARKERS
        .iter()
        .filter(|marker| html_lower.contains(**marker))
        .count();
    let shell_score = usize::from(content_chars < MIN_DYNAMIC_PAGE_CONTENT_CHARS)
        + usize::from(script_tag_count >= 4)
        + dynamic_marker_count.min(3)
        + usize::from(extractor == "sanitized-html" || extractor == "raw-html")
        + usize::from(looks_like_script_payload(content));
    if shell_score >= 4 {
        return Some(HtmlPageDiagnosis {
            kind: HtmlPageKind::DynamicPage,
            reason: format!(
                "dynamic shell markers={dynamic_marker_count}, scripts={script_tag_count}, extractor={extractor}, content_chars={content_chars}"
            ),
        });
    }

    None
}

fn detect_bot_protection_reason(title: &str, content: &str) -> Option<String> {
    let combined = format!("{title}\n{content}").to_ascii_lowercase();
    BOT_PROTECTION_PHRASES
        .iter()
        .find(|phrase| combined.contains(**phrase))
        .map(|phrase| format!("matched bot-protection marker '{phrase}'"))
}

fn looks_like_script_payload(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("var ") || lower.starts_with("const ") || lower.starts_with("let ") {
        return true;
    }

    let mut non_empty_lines = 0usize;
    let mut script_lines = 0usize;
    let mut prose_lines = 0usize;
    for line in trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(40)
    {
        non_empty_lines += 1;
        let line_lower = line.to_ascii_lowercase();
        if line_lower.starts_with("var ")
            || line_lower.starts_with("const ")
            || line_lower.starts_with("let ")
            || line_lower.starts_with("function ")
            || line_lower.contains("=>")
            || line_lower.contains("window.")
            || line_lower.contains("document.")
            || line_lower.contains("reactdom")
            || line_lower.contains("__next")
            || line_lower.contains("__nuxt")
            || line_lower.contains("webpack")
            || (line.ends_with(';') && line.contains('='))
        {
            script_lines += 1;
        }
        if line.contains(' ')
            && line.chars().filter(|ch| ch.is_ascii_alphabetic()).count() >= 24
            && matches!(line.chars().last(), Some('.' | '!' | '?' | ';'))
        {
            prose_lines += 1;
        }
    }

    non_empty_lines > 0
        && script_lines > 0
        && (script_lines * 2 >= non_empty_lines || (script_lines >= 3 && prose_lines == 0))
}

async fn fetch_with_browser_fallback(
    context: &ToolContext<'_>,
    request: BrowserFallbackRequest<'_>,
) -> std::result::Result<CachedPayload, WebFetchFailure> {
    let BrowserFallbackRequest {
        raw_url,
        request_url,
        status,
        content_type,
        max_chars,
        warning,
        diagnosis,
    } = request;
    let scope = browser_fallback_scope(context, request_url);
    let browser = browser_service(context.config);
    let navigate_result = browser
        .execute(
            &scope,
            "navigate",
            &json!({ "url": request_url.to_string() }),
        )
        .await;
    let navigate = match navigate_result {
        Ok(value) => value,
        Err(err) => {
            let _ = browser.execute(&scope, "stop", &json!({})).await;
            return Err(browser_fallback_failure(
                raw_url,
                request_url,
                status,
                content_type,
                diagnosis,
                err.to_string(),
            ));
        }
    };

    let target_id = navigate
        .get("target_id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let read_result = browser
        .execute(
            &scope,
            "read_page",
            &json!({
                "target_id": target_id,
                "max_chars": max_chars,
            }),
        )
        .await;
    let _ = browser.execute(&scope, "stop", &json!({})).await;

    let read_value = read_result.map_err(|err| {
        browser_fallback_failure(
            raw_url,
            request_url,
            status,
            content_type,
            diagnosis,
            err.to_string(),
        )
    })?;
    let content = read_value
        .get("content")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            navigate
                .get("content")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
        })
        .map(ToString::to_string)
        .ok_or_else(|| {
            browser_fallback_failure(
                raw_url,
                request_url,
                status,
                content_type,
                diagnosis,
                i18n::t("tool.web_fetch.no_content"),
            )
        })?;
    if let Some(reason) = detect_bot_protection_reason(
        read_value
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        &content,
    ) {
        return Err(web_fetch_failure(
            raw_url,
            Some(request_url),
            "browser_fallback",
            "TOOL_WEB_FETCH_BOT_PROTECTION",
            i18n::t("tool.web_fetch.bot_protection"),
            Some(
                "The page still appears to be protected by verification after browser fallback. Try another source."
                    .to_string(),
            ),
            false,
            None,
            json!({
                "status": status,
                "content_type": content_type,
                "final_url": read_value.get("url").and_then(Value::as_str).unwrap_or(request_url.as_str()),
                "page_kind": HtmlPageKind::BotProtection.as_str(),
                "diagnosis": reason,
                "fetch_strategy": "browser_fallback",
            }),
        ));
    }

    Ok(CachedPayload {
        final_url: read_value
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or(request_url.as_str())
            .to_string(),
        status,
        content_type: content_type.to_string(),
        title: read_value
            .get("title")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        extractor: "browser-read-page".to_string(),
        content_kind: "html".to_string(),
        fetch_strategy: "browser_fallback".to_string(),
        content,
        warning: merge_warnings(
            warning.as_deref(),
            Some(
                "Direct HTTP fetch looked like a frontend shell, so the page was re-read with the browser runtime."
                    .to_string(),
            ),
        ),
        fetched_at: chrono::Utc::now().to_rfc3339(),
    })
}

fn browser_fallback_scope(context: &ToolContext<'_>, request_url: &Url) -> BrowserSessionScope {
    let mut hasher = DefaultHasher::new();
    request_url.as_str().hash(&mut hasher);
    let url_hash = hasher.finish();
    BrowserSessionScope {
        user_id: context.user_id.to_string(),
        session_id: context.session_id.to_string(),
        agent_id: context.agent_id.map(ToString::to_string),
        profile: None,
        browser_session_id: Some(format!(
            "web-fetch:{}:{}:{url_hash:x}",
            chrono::Utc::now().timestamp_millis(),
            context.session_id
        )),
    }
}

fn browser_fallback_failure(
    raw_url: &str,
    request_url: &Url,
    status: u16,
    content_type: &str,
    diagnosis: &HtmlPageDiagnosis,
    detail: String,
) -> WebFetchFailure {
    let mut params = HashMap::new();
    params.insert("detail".to_string(), detail.clone());
    web_fetch_failure(
        raw_url,
        Some(request_url),
        "browser_fallback",
        "TOOL_WEB_FETCH_BROWSER_FALLBACK_FAILED",
        i18n::t_with_params("tool.web_fetch.browser_fallback_failed", &params),
        Some(
            "Direct HTTP fetch did not return meaningful content, and browser fallback also failed. Try another public source."
                .to_string(),
        ),
        true,
        Some(500),
        json!({
            "status": status,
            "content_type": content_type,
            "final_url": request_url.as_str(),
            "page_kind": diagnosis.kind.as_str(),
            "diagnosis": diagnosis.reason,
            "fetch_strategy": "browser_fallback",
            "browser_error": detail,
        }),
    )
}

fn read_cache_entry(key: &str) -> Option<CachedPayload> {
    let cache = web_fetch_cache();
    if let Some(entry) = cache.get(key) {
        if Instant::now() <= entry.expires_at {
            return Some(entry.payload.clone());
        }
    }
    cache.remove(key);
    None
}

fn write_cache_entry(key: &str, payload: CachedPayload, ttl_secs: u64) {
    if ttl_secs == 0 {
        return;
    }
    let expires_at = Instant::now() + Duration::from_secs(ttl_secs);
    web_fetch_cache().insert(
        key.to_string(),
        CacheEntry {
            expires_at,
            payload,
        },
    );
}

fn web_fetch_cache() -> &'static DashMap<String, CacheEntry> {
    static CACHE: OnceLock<DashMap<String, CacheEntry>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn build_tool_result(
    raw_url: &str,
    payload: &CachedPayload,
    extract_mode: ExtractMode,
    max_chars: usize,
    cached: bool,
) -> Value {
    let (content, truncated) = truncate_chars(&payload.content, max_chars);
    let warning = merge_warnings(
        payload.warning.as_deref(),
        truncated.then(|| format!("Content truncated after {max_chars} chars.")),
    );
    json!({
        "url": raw_url,
        "final_url": payload.final_url,
        "status": payload.status,
        "title": payload.title,
        "content_type": payload.content_type,
        "content_kind": payload.content_kind,
        "fetch_strategy": payload.fetch_strategy,
        "format": extract_mode.as_str(),
        "extractor": payload.extractor,
        "truncated": truncated,
        "warning": warning,
        "cached": cached,
        "fetched_at": payload.fetched_at,
        "content": content,
    })
}

fn merge_warnings(primary: Option<&str>, secondary: Option<String>) -> Option<String> {
    match (
        primary.map(str::trim).filter(|value| !value.is_empty()),
        secondary,
    ) {
        (Some(left), Some(right)) => Some(format!("{left} {right}")),
        (Some(left), None) => Some(left.to_string()),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
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

fn extract_html_content(html: &str, mode: ExtractMode) -> Result<HtmlExtraction> {
    let cleaned_source = preclean_html(html);
    let document = kuchiki::parse_html().one(cleaned_source);
    let title = extract_title(&document);
    sanitize_document(&document);

    let body_node = document
        .select_first("body")
        .ok()
        .map(|node| node.as_node().clone());
    let selected_node = select_main_container(&document).or_else(|| body_node.clone());
    let mut extractor = "main-content".to_string();
    let mut markdown = selected_node
        .as_ref()
        .map(render_markdown_from_node)
        .unwrap_or_default();

    if markdown.chars().count() < MIN_PRIMARY_CONTENT_CHARS {
        if let Some(body) = body_node.as_ref() {
            let fallback = render_markdown_from_node(body);
            if fallback.chars().count() > markdown.chars().count() {
                markdown = fallback;
                extractor = "body-html".to_string();
            }
        }
    }

    if markdown.trim().is_empty() {
        markdown = body_node
            .as_ref()
            .map(render_markdown_from_node)
            .unwrap_or_else(|| clean_markdown(&html2md::parse_html(&document.to_string())));
        extractor = "sanitized-html".to_string();
    }

    let content = match mode {
        ExtractMode::Markdown => markdown,
        ExtractMode::Text => markdown_to_text(&markdown),
    };
    let content = if content.trim().is_empty() {
        body_node
            .as_ref()
            .map(|node| normalize_text_block(&node.text_contents()))
            .unwrap_or_default()
    } else {
        content
    };

    if content.trim().is_empty() {
        return Err(anyhow!(i18n::t("tool.web_fetch.no_content")));
    }

    Ok(HtmlExtraction {
        title,
        content,
        extractor,
    })
}

fn preclean_html(html: &str) -> String {
    let without_comments = html_comment_regex().replace_all(html, "");
    strip_invisible_unicode(&without_comments)
}

fn sanitize_document(document: &NodeRef) {
    for selector in ALWAYS_DROP_SELECTORS {
        detach_selector_matches(document, selector);
    }
    let mut nodes = document
        .select("*")
        .ok()
        .into_iter()
        .flat_map(|selection| selection.map(|node| node.as_node().clone()))
        .collect::<Vec<_>>();
    nodes.reverse();
    for node in nodes {
        if should_detach_node(&node) {
            node.detach();
        }
    }
}

fn detach_selector_matches(document: &NodeRef, selector: &str) {
    let matches = document
        .select(selector)
        .ok()
        .into_iter()
        .flat_map(|selection| selection.map(|node| node.as_node().clone()))
        .collect::<Vec<_>>();
    for node in matches {
        node.detach();
    }
}

fn should_detach_node(node: &NodeRef) -> bool {
    let Some(element) = node.as_element() else {
        return false;
    };
    let tag = element.name.local.as_ref();
    if matches!(tag, "html" | "body") {
        return false;
    }
    if matches!(tag, "nav" | "aside" | "footer" | "form") {
        return true;
    }

    let attrs = element.attributes.borrow();
    if attrs.get("hidden").is_some() {
        return true;
    }
    if attrs
        .get("aria-hidden")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return true;
    }
    if tag == "input"
        && attrs
            .get("type")
            .map(|value| value.eq_ignore_ascii_case("hidden"))
            .unwrap_or(false)
    {
        return true;
    }

    let role = attrs
        .get("role")
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if matches!(
        role.as_str(),
        "navigation" | "complementary" | "contentinfo" | "search" | "dialog"
    ) {
        return true;
    }

    let class_name = attrs.get("class").unwrap_or("");
    if has_hidden_class(class_name) {
        return true;
    }

    let style = attrs.get("style").unwrap_or("");
    if is_hidden_style(style) {
        return true;
    }

    let hint_text =
        format!("{} {}", class_name, attrs.get("id").unwrap_or("")).to_ascii_lowercase();
    has_strong_noise_keyword(&hint_text) && !has_positive_hint_keyword(&hint_text)
}

fn has_hidden_class(class_name: &str) -> bool {
    class_name.split_whitespace().any(|name| {
        matches!(
            name.to_ascii_lowercase().as_str(),
            "hidden"
                | "sr-only"
                | "visually-hidden"
                | "screen-reader-only"
                | "offscreen"
                | "invisible"
                | "d-none"
        )
    })
}

fn is_hidden_style(style: &str) -> bool {
    let normalized = style.to_ascii_lowercase().replace(' ', "");
    normalized.contains("display:none")
        || normalized.contains("visibility:hidden")
        || normalized.contains("opacity:0")
        || normalized.contains("font-size:0")
        || normalized.contains("clip-path:inset(")
        || normalized.contains("transform:scale(0)")
        || (normalized.contains("width:0")
            && normalized.contains("height:0")
            && normalized.contains("overflow:hidden"))
}

fn has_strong_noise_keyword(text: &str) -> bool {
    STRONG_NOISE_KEYWORDS
        .iter()
        .any(|keyword| text.contains(keyword))
}

fn has_positive_hint_keyword(text: &str) -> bool {
    POSITIVE_HINT_KEYWORDS
        .iter()
        .any(|keyword| text.contains(keyword))
}

fn extract_title(document: &NodeRef) -> Option<String> {
    extract_attr(document, "meta[property='og:title']", "content")
        .or_else(|| extract_text(document, "title"))
        .or_else(|| extract_text(document, "h1"))
        .map(|value| normalize_text_block(&value))
        .filter(|value| !value.is_empty())
}

fn extract_attr(document: &NodeRef, selector: &str, attr: &str) -> Option<String> {
    let node = document.select_first(selector).ok()?;
    let attrs = node.attributes.borrow();
    attrs.get(attr).map(str::to_string)
}

fn extract_text(document: &NodeRef, selector: &str) -> Option<String> {
    let node = document.select_first(selector).ok()?;
    let text = normalize_text_block(&node.text_contents());
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn select_main_container(document: &NodeRef) -> Option<NodeRef> {
    let mut best_score = f64::MIN;
    let mut best_chars = 0usize;
    let mut best_node = None;

    for (selector, base_bonus) in PRIMARY_CONTENT_SELECTORS {
        let Ok(selection) = document.select(selector) else {
            continue;
        };
        for candidate in selection {
            let node = candidate.as_node().clone();
            let stats = summarize_candidate(&node);
            if stats.text_chars == 0 {
                continue;
            }
            let score = score_candidate(&stats, *base_bonus, &node);
            if score > best_score || (score == best_score && stats.text_chars > best_chars) {
                best_score = score;
                best_chars = stats.text_chars;
                best_node = Some(node);
            }
        }
    }

    if best_chars >= MIN_PRIMARY_CONTENT_CHARS {
        return best_node;
    }
    None
}

fn summarize_candidate(node: &NodeRef) -> CandidateStats {
    let plain_text = normalize_text_block(&node.text_contents());
    let text_chars = plain_text.chars().count();
    let punctuation_count = count_punctuation(&plain_text);
    let link_text_chars = node
        .select("a")
        .ok()
        .into_iter()
        .flat_map(|selection| selection.map(|link| normalize_text_block(&link.text_contents())))
        .map(|text| text.chars().count())
        .sum();
    let paragraph_count = node
        .select("p, li, blockquote")
        .ok()
        .into_iter()
        .flat_map(|selection| selection.map(|item| normalize_text_block(&item.text_contents())))
        .filter(|text| text.chars().count() >= 40)
        .count();
    let pre_count = node
        .select("pre, code")
        .ok()
        .into_iter()
        .flat_map(|selection| selection.map(|item| normalize_text_block(&item.text_contents())))
        .filter(|text| text.chars().count() >= 8)
        .count();
    let heading_count = node
        .select("h1, h2, h3")
        .ok()
        .map(|value| value.count())
        .unwrap_or(0);
    let hint_text = node
        .as_element()
        .map(|element| {
            let attrs = element.attributes.borrow();
            format!(
                "{} {} {}",
                element.name.local,
                attrs.get("class").unwrap_or(""),
                attrs.get("id").unwrap_or("")
            )
            .to_ascii_lowercase()
        })
        .unwrap_or_default();
    CandidateStats {
        text_chars,
        punctuation_count,
        link_text_chars,
        paragraph_count,
        pre_count,
        heading_count,
        positive_hint: has_positive_hint_keyword(&hint_text),
        negative_hint: has_strong_noise_keyword(&hint_text),
    }
}

fn score_candidate(stats: &CandidateStats, base_bonus: f64, node: &NodeRef) -> f64 {
    let text_chars = stats.text_chars as f64;
    let link_density = (stats.link_text_chars as f64) / text_chars.max(1.0);
    let tag_bonus = node
        .as_element()
        .map(|element| match element.name.local.as_ref() {
            "article" => 140.0,
            "main" => 120.0,
            _ => 0.0,
        })
        .unwrap_or(0.0);
    let mut score = text_chars;
    score += stats.paragraph_count as f64 * 90.0;
    score += stats.pre_count as f64 * 50.0;
    score += stats.heading_count as f64 * 24.0;
    score += (stats.punctuation_count.min(160)) as f64 * 4.0;
    score += base_bonus + tag_bonus;
    if stats.positive_hint {
        score += 180.0;
    }
    if stats.negative_hint {
        score -= 420.0;
    }
    if stats.text_chars < MIN_PRIMARY_CONTENT_CHARS {
        score -= 180.0;
    }
    score - link_density * 900.0
}

fn render_markdown_from_node(node: &NodeRef) -> String {
    clean_markdown(&html2md::parse_html(&node.to_string()))
}

fn clean_markdown(markdown: &str) -> String {
    let normalized = strip_invisible_unicode(markdown)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\u{00a0}', " ");
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    let mut in_code = false;

    for raw_line in normalized.lines() {
        let trimmed_end = raw_line.trim_end();
        let marker = trimmed_end.trim_start();
        if marker.starts_with("```") {
            in_code = !in_code;
            current.push(trimmed_end.to_string());
            continue;
        }
        if in_code {
            current.push(trimmed_end.to_string());
            continue;
        }

        let line = collapse_inline_whitespace(trimmed_end);
        if line.is_empty() {
            push_clean_block(&mut blocks, &mut current);
            continue;
        }
        current.push(line);
    }
    push_clean_block(&mut blocks, &mut current);

    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for block in blocks {
        if is_noise_block(&block) {
            continue;
        }
        let dedupe_key = canonicalize_block_for_dedupe(&block);
        if dedupe_key.chars().count() > 24 && !seen.insert(dedupe_key) {
            continue;
        }
        output.push(block);
    }
    output.join("\n\n").trim().to_string()
}

fn push_clean_block(blocks: &mut Vec<String>, current: &mut Vec<String>) {
    if current.is_empty() {
        return;
    }
    let block = current.join("\n").trim().to_string();
    current.clear();
    if !block.is_empty() {
        blocks.push(block);
    }
}

fn collapse_inline_whitespace(line: &str) -> String {
    let mut output = String::new();
    let mut last_was_space = false;
    for ch in line.chars() {
        let mapped = match ch {
            '\t' | '\u{2002}' | '\u{2003}' | '\u{2009}' => ' ',
            other => other,
        };
        if mapped.is_whitespace() {
            if !last_was_space {
                output.push(' ');
                last_was_space = true;
            }
            continue;
        }
        output.push(mapped);
        last_was_space = false;
    }
    output.trim().to_string()
}

fn is_noise_block(block: &str) -> bool {
    let plain = markdown_to_text(block);
    if plain.is_empty() {
        return true;
    }
    if plain.chars().count() <= 2 && plain.chars().all(|ch| !ch.is_alphanumeric()) {
        return true;
    }

    let lower = plain.to_lowercase();
    if plain.chars().count() <= 180
        && NOISE_BLOCK_PHRASES
            .iter()
            .any(|phrase| lower.contains(phrase))
    {
        return true;
    }

    let separator_count = plain
        .chars()
        .filter(|ch| matches!(ch, '|' | '>' | '/' | '·' | '•'))
        .count();
    if plain.chars().count() <= 120
        && separator_count >= 2
        && count_punctuation(&plain) <= 1
        && !plain.contains('。')
    {
        return true;
    }

    markdown_link_regex().captures_iter(block).count() >= 3 && plain.chars().count() <= 120
}

fn canonicalize_block_for_dedupe(block: &str) -> String {
    normalize_text_block(&markdown_to_text(block)).to_lowercase()
}

fn markdown_to_text(markdown: &str) -> String {
    let mut stripped = markdown.to_string();
    stripped = markdown_image_regex()
        .replace_all(&stripped, "")
        .into_owned();
    stripped = markdown_link_regex()
        .replace_all(&stripped, "$1")
        .into_owned();
    stripped = markdown_inline_code_regex()
        .replace_all(&stripped, "$1")
        .into_owned();
    stripped = markdown_heading_regex()
        .replace_all(&stripped, "")
        .into_owned();
    stripped = markdown_unordered_list_regex()
        .replace_all(&stripped, "")
        .into_owned();
    stripped = markdown_ordered_list_regex()
        .replace_all(&stripped, "")
        .into_owned();

    let mut lines = Vec::new();
    let mut in_code = false;
    for line in stripped.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        lines.push(trimmed.to_string());
    }
    normalize_text_block(&lines.join("\n"))
}

fn normalize_text_block(text: &str) -> String {
    let normalized = strip_invisible_unicode(text)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\u{00a0}', " ");
    let mut lines = Vec::new();
    let mut blank_streak = 0usize;
    for raw_line in normalized.lines() {
        let line = collapse_inline_whitespace(raw_line);
        if line.is_empty() {
            blank_streak += 1;
            if blank_streak <= 1 {
                lines.push(String::new());
            }
            continue;
        }
        blank_streak = 0;
        lines.push(line);
    }
    lines.join("\n").trim().to_string()
}

fn count_punctuation(text: &str) -> usize {
    text.chars()
        .filter(|ch| {
            matches!(
                ch,
                '.' | ',' | ';' | ':' | '!' | '?' | '，' | '。' | '！' | '？' | '；' | '：'
            )
        })
        .count()
}

fn strip_invisible_unicode(text: &str) -> String {
    text.chars()
        .filter(|ch| !is_invisible_unicode(*ch))
        .collect()
}

fn is_invisible_unicode(ch: char) -> bool {
    matches!(
        ch,
        '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{206a}'..='\u{206f}'
            | '\u{feff}'
    )
}

fn html_comment_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"<!--[\s\S]*?-->").expect("comment regex"))
}

fn meta_charset_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"(?is)<meta[^>]+charset\s*=\s*["']?\s*([a-zA-Z0-9._-]+)"#)
            .expect("charset regex")
    })
}

fn markdown_image_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"!\[[^\]]*]\([^)]+\)").expect("markdown image regex"))
}

fn markdown_link_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\[([^\]]+)]\([^)]+\)").expect("markdown link regex"))
}

fn markdown_inline_code_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"`([^`]+)`").expect("markdown inline code regex"))
}

fn markdown_heading_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"(?m)^\s*#{1,6}\s+").expect("markdown heading regex"))
}

fn markdown_unordered_list_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"(?m)^\s*[-*+]\s+").expect("markdown unordered list regex"))
}

fn markdown_ordered_list_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"(?m)^\s*\d+\.\s+").expect("markdown ordered list regex"))
}

#[cfg(test)]
mod tests {
    use super::{
        canonicalize_block_for_dedupe, diagnose_html_page, extract_html_content,
        ip_is_private_or_internal, is_noise_block, normalize_text_block, strip_invisible_unicode,
        truncate_chars, web_fetch_failure, ExtractMode, HtmlPageKind,
    };
    use serde_json::json;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use url::Url;

    #[test]
    fn extract_html_content_prefers_main_and_drops_noise() {
        let html = r#"
            <html>
              <head><title>Test Article</title></head>
              <body>
                <nav>Home > Docs > Tools</nav>
                <article class="post-content">
                  <h1>Test Article</h1>
                  <p>This is the first paragraph with enough detail to be useful.</p>
                  <p>This is the second paragraph. It contains the actual body content.</p>
                </article>
                <footer>All rights reserved</footer>
              </body>
            </html>
        "#;

        let result = extract_html_content(html, ExtractMode::Text).expect("html should extract");
        assert_eq!(result.title.as_deref(), Some("Test Article"));
        assert!(result.content.contains("first paragraph"));
        assert!(result.content.contains("second paragraph"));
        assert!(!result.content.contains("All rights reserved"));
        assert!(!result.content.contains("Home > Docs"));
    }

    #[test]
    fn extract_html_content_removes_hidden_nodes() {
        let html = r#"
            <html>
              <body>
                <main>
                  <p>Visible text.</p>
                  <div class="hidden">Hidden text.</div>
                  <div aria-hidden="true">Also hidden.</div>
                </main>
              </body>
            </html>
        "#;

        let result = extract_html_content(html, ExtractMode::Text).expect("html should extract");
        assert!(result.content.contains("Visible text"));
        assert!(!result.content.contains("Hidden text"));
        assert!(!result.content.contains("Also hidden"));
    }

    #[test]
    fn extract_html_content_rejects_script_only_shells() {
        let html = r#"
            <html>
              <head>
                <title>Shell Page</title>
                <script>var buildId = "abc123"; window.__NEXT_DATA__ = {};</script>
              </head>
              <body>
                <div id="__next"></div>
              </body>
            </html>
        "#;

        assert!(extract_html_content(html, ExtractMode::Text).is_err());
    }

    #[test]
    fn diagnose_html_page_flags_dynamic_shells() {
        let html = r#"
            <html>
              <head>
                <script>window.__NEXT_DATA__ = {};</script>
                <script>console.log("boot")</script>
                <script>ReactDOM.hydrateRoot(document.getElementById('root'));</script>
              </head>
              <body><div id="root"></div></body>
            </html>
        "#;

        let diagnosis = diagnose_html_page(
            html,
            Some("Shell"),
            "var buildId = \"abc123\";",
            "sanitized-html",
        )
        .expect("dynamic shell should be detected");
        assert_eq!(diagnosis.kind, HtmlPageKind::DynamicPage);
    }

    #[test]
    fn diagnose_html_page_flags_bot_protection() {
        let diagnosis = diagnose_html_page(
            "<html><body><h1>Access Denied</h1></body></html>",
            Some("Access Denied"),
            "Verify you are human before continuing.",
            "main-content",
        )
        .expect("bot protection should be detected");
        assert_eq!(diagnosis.kind, HtmlPageKind::BotProtection);
    }

    #[test]
    fn noise_block_filters_footer_and_breadcrumbs() {
        assert!(is_noise_block("Home > Docs > Tools"));
        assert!(is_noise_block("All rights reserved"));
        assert!(!is_noise_block(
            "This is an actual paragraph with enough content to keep."
        ));
    }

    #[test]
    fn strip_invisible_unicode_removes_zero_width_markers() {
        assert_eq!(strip_invisible_unicode("ab\u{200b}cd\u{feff}"), "abcd");
    }

    #[test]
    fn normalize_text_block_collapses_whitespace() {
        assert_eq!(
            normalize_text_block("  hello   world \n\n\t next line  "),
            "hello world\n\nnext line"
        );
    }

    #[test]
    fn truncate_chars_respects_char_boundaries() {
        let (text, truncated) = truncate_chars("你好abc", 3);
        assert_eq!(text, "你好a");
        assert!(truncated);
    }

    #[test]
    fn private_ip_detection_blocks_internal_ranges() {
        assert!(ip_is_private_or_internal(IpAddr::V4(Ipv4Addr::new(
            127, 0, 0, 1
        ))));
        assert!(ip_is_private_or_internal(IpAddr::V4(Ipv4Addr::new(
            10, 1, 2, 3
        ))));
        assert!(ip_is_private_or_internal(IpAddr::V4(Ipv4Addr::new(
            100, 64, 0, 1
        ))));
        assert!(ip_is_private_or_internal(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(!ip_is_private_or_internal(IpAddr::V4(Ipv4Addr::new(
            93, 184, 216, 34
        ))));
    }

    #[test]
    fn canonicalize_block_for_dedupe_uses_plain_text() {
        let left = canonicalize_block_for_dedupe("[Read more](https://example.com)");
        let right = canonicalize_block_for_dedupe("Read more");
        assert_eq!(left, right);
    }

    #[test]
    fn structured_failure_contains_phase_and_error_meta() {
        let url = Url::parse("https://example.com/docs").expect("url");
        let payload = web_fetch_failure(
            "https://example.com/docs",
            Some(&url),
            "request",
            "TOOL_TIMEOUT",
            "request timed out",
            Some("retry later".to_string()),
            true,
            Some(1000),
            json!({ "timeout_s": 1 }),
        )
        .into_value();
        assert_eq!(payload["data"]["phase"], json!("request"));
        assert_eq!(payload["data"]["host"], json!("example.com"));
        assert_eq!(payload["data"]["error_meta"]["code"], json!("TOOL_TIMEOUT"));
    }
}
