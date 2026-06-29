use crate::config::LlmModelConfig;
use crate::services::virtual_llm::VIRTUAL_REPLAY_PROVIDER;
use reqwest::header::HeaderMap;
use url::Url;

use super::{
    build_headers, OpenAiApiMode, DEFAULT_ANTHROPIC_BASE_URL, DEFAULT_DEEPSEEK_BASE_URL,
    DEFAULT_GROQ_BASE_URL, DEFAULT_LMSTUDIO_BASE_URL, DEFAULT_MISTRAL_BASE_URL,
    DEFAULT_MOONSHOT_BASE_URL, DEFAULT_OLLAMA_BASE_URL, DEFAULT_OPENAI_BASE_URL,
    DEFAULT_OPENROUTER_BASE_URL, DEFAULT_QWEN_BASE_URL, DEFAULT_SILICONFLOW_BASE_URL,
    DEFAULT_TOGETHER_BASE_URL, DEFAULT_VLLM_OMNI_BASE_URL, DEFAULT_WHISPER_CPP_BASE_URL,
    MESSAGES_RESOURCE, OPENAI_COMPAT_RESOURCE_SUFFIXES,
};

pub fn normalize_provider(provider: Option<&str>) -> String {
    let raw = provider.unwrap_or("openai_compatible").trim();
    if raw.is_empty() {
        return "openai_compatible".to_string();
    }
    let normalized = raw.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    match normalized.as_str() {
        "openai_compat" => "openai_compatible".to_string(),
        "openai_native" => "openai".to_string(),
        "openai" => "openai".to_string(),
        "openai_compatible" => "openai_compatible".to_string(),
        "anthropic" => "anthropic".to_string(),
        "claude" => "anthropic".to_string(),
        "anthropic_api" => "anthropic".to_string(),
        "openrouter" => "openrouter".to_string(),
        "silicon_flow" => "siliconflow".to_string(),
        "siliconflow" => "siliconflow".to_string(),
        "deepseek" => "deepseek".to_string(),
        "moonshot" => "moonshot".to_string(),
        "kimi" => "moonshot".to_string(),
        "dashscope" => "qwen".to_string(),
        "qwen" => "qwen".to_string(),
        "groq" => "groq".to_string(),
        "mistral" => "mistral".to_string(),
        "together" => "together".to_string(),
        "vllm_omni" => "vllm_omni".to_string(),
        "vllmomni" => "vllm_omni".to_string(),
        "vllm_omni_api" => "vllm_omni".to_string(),
        "whisper_cpp" => "whisper_cpp".to_string(),
        "whispercpp" => "whisper_cpp".to_string(),
        "ollama" => "ollama".to_string(),
        "lm_studio" => "lmstudio".to_string(),
        "lmstudio" => "lmstudio".to_string(),
        "virtual"
        | "virtual_llm"
        | "virtual_model"
        | "replay"
        | "jsonl_replay"
        | "mock_replay"
        | VIRTUAL_REPLAY_PROVIDER => VIRTUAL_REPLAY_PROVIDER.to_string(),
        other => other.to_string(),
    }
}

pub(super) fn should_strip_openai_tool_schema(provider: Option<&str>) -> bool {
    normalize_provider(provider) == "openai"
}

pub fn provider_default_base_url(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some(DEFAULT_OPENAI_BASE_URL),
        "anthropic" => Some(DEFAULT_ANTHROPIC_BASE_URL),
        "openrouter" => Some(DEFAULT_OPENROUTER_BASE_URL),
        "siliconflow" => Some(DEFAULT_SILICONFLOW_BASE_URL),
        "deepseek" => Some(DEFAULT_DEEPSEEK_BASE_URL),
        "moonshot" => Some(DEFAULT_MOONSHOT_BASE_URL),
        "qwen" => Some(DEFAULT_QWEN_BASE_URL),
        "groq" => Some(DEFAULT_GROQ_BASE_URL),
        "mistral" => Some(DEFAULT_MISTRAL_BASE_URL),
        "together" => Some(DEFAULT_TOGETHER_BASE_URL),
        "vllm_omni" => Some(DEFAULT_VLLM_OMNI_BASE_URL),
        "whisper_cpp" => Some(DEFAULT_WHISPER_CPP_BASE_URL),
        "ollama" => Some(DEFAULT_OLLAMA_BASE_URL),
        "lmstudio" => Some(DEFAULT_LMSTUDIO_BASE_URL),
        _ => None,
    }
}

pub fn is_openai_compatible_provider(provider: &str) -> bool {
    let normalized = normalize_provider(Some(provider));
    if normalized == "anthropic" {
        return false;
    }
    if normalized == "openai_compatible" {
        return true;
    }
    provider_default_base_url(&normalized).is_some()
}

pub fn should_disable_streaming_for_native_tools(
    _config: &LlmModelConfig,
    _has_native_tools: bool,
) -> bool {
    false
}

pub(super) fn resolve_base_url(config: &LlmModelConfig) -> Option<String> {
    let inline = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(value) = inline {
        return Some(value.to_string());
    }
    let provider = normalize_provider(config.provider.as_deref());
    provider_default_base_url(&provider).map(ToString::to_string)
}

pub fn resolve_model_base_url(config: &LlmModelConfig) -> Option<String> {
    resolve_base_url(config)
}

pub fn build_openai_model_resource_endpoint(base_url: &str, resource: &str) -> Option<String> {
    build_openai_resource_endpoint(base_url, resource)
}

pub fn build_model_auth_headers(api_key: &str) -> HeaderMap {
    build_headers(api_key)
}

pub fn resolve_openai_api_mode(config: &LlmModelConfig) -> OpenAiApiMode {
    if let Some(value) = config.api_mode.as_deref() {
        return normalize_openai_api_mode(Some(value));
    }
    if let Some(base_url) = config.base_url.as_deref() {
        if base_url
            .to_ascii_lowercase()
            .trim_end_matches('/')
            .contains("/responses")
        {
            return OpenAiApiMode::Responses;
        }
    }
    if should_default_to_responses_api(config) {
        return OpenAiApiMode::Responses;
    }
    OpenAiApiMode::ChatCompletions
}

fn should_default_to_responses_api(config: &LlmModelConfig) -> bool {
    let provider = normalize_provider(config.provider.as_deref());
    if provider != "openai" {
        return false;
    }
    let Some(model) = config.model.as_deref().map(str::trim) else {
        return false;
    };
    if model.is_empty() {
        return false;
    }
    let lowered = model.to_ascii_lowercase();
    lowered.starts_with("gpt-5")
        || lowered.starts_with("o1")
        || lowered.starts_with("o3")
        || lowered.starts_with("o4")
}

pub(super) fn build_openai_resource_endpoint(base_url: &str, resource: &str) -> Option<String> {
    let normalized_base = normalize_base_url(base_url)?;
    let trimmed = resource.trim_matches('/');
    if trimmed.is_empty() {
        return Some(normalized_base);
    }
    Some(format!("{normalized_base}/{trimmed}"))
}

pub(super) fn build_anthropic_messages_endpoint(base_url: &str) -> Option<String> {
    let normalized_base = normalize_anthropic_base_url(base_url)?;
    Some(format!("{normalized_base}/{MESSAGES_RESOURCE}"))
}

pub(super) fn parse_url_without_query_fragment(value: &str) -> Option<Url> {
    let mut parsed = Url::parse(value).ok()?;
    parsed.set_query(None);
    parsed.set_fragment(None);
    Some(parsed)
}

pub(super) fn parse_or_clean_base_url(base_url: &str) -> Option<(Option<Url>, String)> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(parsed) = parse_url_without_query_fragment(trimmed) {
        return Some((Some(parsed), String::new()));
    }
    let cleaned = trimmed
        .split(['?', '#'])
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches('/')
        .to_string();
    if cleaned.is_empty() {
        None
    } else {
        Some((None, cleaned))
    }
}

pub(super) fn collect_path_segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub(super) fn strip_openai_resource_suffix(segments: &mut Vec<String>) {
    for suffix in OPENAI_COMPAT_RESOURCE_SUFFIXES {
        if segments.len() < suffix.len() {
            continue;
        }
        let start = segments.len() - suffix.len();
        if segments[start..]
            .iter()
            .map(String::as_str)
            .eq(suffix.iter().copied())
        {
            segments.truncate(start);
            break;
        }
    }
}

pub(super) fn is_version_segment(segment: &str) -> bool {
    let Some(rest) = segment
        .strip_prefix('v')
        .or_else(|| segment.strip_prefix('V'))
    else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit())
}

pub(super) fn normalize_base_url(base_url: &str) -> Option<String> {
    let (parsed, cleaned_fallback) = parse_or_clean_base_url(base_url)?;
    if let Some(mut parsed) = parsed {
        let mut segments = collect_path_segments(parsed.path());
        strip_openai_resource_suffix(&mut segments);
        if !segments
            .last()
            .is_some_and(|segment| is_version_segment(segment))
        {
            segments.push("v1".to_string());
        }
        parsed.set_path(&format!("/{}", segments.join("/")));
        return Some(parsed.to_string().trim_end_matches('/').to_string());
    }

    let mut base = cleaned_fallback;
    for suffix in ["/chat/completions", "/responses", "/embeddings", "/models"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.trim_end_matches('/').to_string();
            break;
        }
    }
    if base.is_empty() {
        return None;
    }
    if base.rsplit('/').next().is_some_and(is_version_segment) {
        Some(base)
    } else {
        Some(format!("{base}/v1"))
    }
}

pub fn normalize_openai_api_mode(value: Option<&str>) -> OpenAiApiMode {
    let raw = value.unwrap_or("").trim();
    if raw.is_empty() {
        return OpenAiApiMode::ChatCompletions;
    }
    match raw
        .to_ascii_lowercase()
        .replace(['-', ' ', '/'], "_")
        .as_str()
    {
        "responses" | "response" | "response_api" | "v1_responses" => OpenAiApiMode::Responses,
        "chat" | "chat_completions" | "chatcompletion" | "chat_completions_api" => {
            OpenAiApiMode::ChatCompletions
        }
        _ => OpenAiApiMode::ChatCompletions,
    }
}

fn normalize_anthropic_base_url(base_url: &str) -> Option<String> {
    let (parsed, cleaned_fallback) = parse_or_clean_base_url(base_url)?;
    if let Some(mut parsed) = parsed {
        let mut segments = collect_path_segments(parsed.path());
        if segments
            .last()
            .is_some_and(|segment| segment.eq_ignore_ascii_case(MESSAGES_RESOURCE))
        {
            segments.pop();
        }
        if !segments
            .last()
            .is_some_and(|segment| is_version_segment(segment))
        {
            segments.push("v1".to_string());
        }
        parsed.set_path(&format!("/{}", segments.join("/")));
        return Some(parsed.to_string().trim_end_matches('/').to_string());
    }

    let mut base = cleaned_fallback.trim_end_matches('/').to_string();
    if let Some(stripped) = base.strip_suffix("/messages") {
        base = stripped.trim_end_matches('/').to_string();
    }
    if base.is_empty() {
        return None;
    }
    if base
        .rsplit('/')
        .next()
        .is_none_or(|segment| !is_version_segment(segment))
    {
        base = format!("{base}/v1");
    }
    Some(base)
}

#[cfg(test)]
mod tests {
    use super::{
        build_openai_resource_endpoint, is_openai_compatible_provider,
        normalize_anthropic_base_url, normalize_base_url, normalize_provider,
        should_strip_openai_tool_schema,
    };
    use crate::services::llm::CHAT_COMPLETIONS_RESOURCE;

    #[test]
    fn normalize_base_url_keeps_existing_version_segment() {
        let normalized = normalize_base_url("https://open.bigmodel.cn/api/paas/v4/")
            .expect("normalized base url");
        assert_eq!(normalized, "https://open.bigmodel.cn/api/paas/v4");
    }

    #[test]
    fn normalize_base_url_strips_resource_suffixes() {
        let normalized = normalize_base_url("https://example.com/v1/chat/completions")
            .expect("normalized base url");
        assert_eq!(normalized, "https://example.com/v1");
    }

    #[test]
    fn build_openai_resource_endpoint_uses_detected_version_path() {
        let endpoint = build_openai_resource_endpoint(
            "https://open.bigmodel.cn/api/paas/v4/",
            CHAT_COMPLETIONS_RESOURCE,
        )
        .expect("chat endpoint");
        assert_eq!(
            endpoint,
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn normalize_provider_maps_anthropic_aliases() {
        assert_eq!(normalize_provider(Some("anthropic")), "anthropic");
        assert_eq!(normalize_provider(Some("claude")), "anthropic");
        assert_eq!(normalize_provider(Some("anthropic_api")), "anthropic");
    }

    #[test]
    fn is_openai_compatible_provider_excludes_anthropic() {
        assert!(!is_openai_compatible_provider("anthropic"));
        assert!(!is_openai_compatible_provider("claude"));
        assert!(is_openai_compatible_provider("openai_compatible"));
    }

    #[test]
    fn normalize_anthropic_base_url_adds_v1_and_strips_messages_suffix() {
        let normalized =
            normalize_anthropic_base_url("https://aiproxy.test/cosphere").expect("anthropic base");
        assert_eq!(normalized, "https://aiproxy.test/cosphere/v1");

        let normalized = normalize_anthropic_base_url("https://api.anthropic.com/v1/messages")
            .expect("anthropic messages endpoint");
        assert_eq!(normalized, "https://api.anthropic.com/v1");
    }

    #[test]
    fn should_strip_openai_tool_schema_only_for_openai_provider() {
        assert!(should_strip_openai_tool_schema(Some("openai")));
        assert!(!should_strip_openai_tool_schema(Some("openai_compatible")));
        assert!(!should_strip_openai_tool_schema(Some("groq")));
    }
}
