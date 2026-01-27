// i18n 支持：语言解析、上下文管理与翻译文本读取。
use parking_lot::RwLock;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::task_local;
use tracing::error;

#[derive(Clone, Debug)]
struct I18nState {
    default_language: String,
    supported_languages: Vec<String>,
    aliases: HashMap<String, String>,
    messages: HashMap<String, HashMap<String, String>>,
}

impl I18nState {
    fn new() -> Self {
        let mut aliases = HashMap::new();
        aliases.insert("zh".to_string(), "zh-CN".to_string());
        aliases.insert("zh-cn".to_string(), "zh-CN".to_string());
        aliases.insert("zh-hans".to_string(), "zh-CN".to_string());
        aliases.insert("zh-hans-cn".to_string(), "zh-CN".to_string());
        aliases.insert("en".to_string(), "en-US".to_string());
        aliases.insert("en-us".to_string(), "en-US".to_string());
        Self {
            default_language: "zh-CN".to_string(),
            supported_languages: vec!["zh-CN".to_string(), "en-US".to_string()],
            aliases,
            messages: HashMap::new(),
        }
    }
}

static I18N_STATE: OnceLock<RwLock<I18nState>> = OnceLock::new();

task_local! {
    static CURRENT_LANGUAGE: String;
}

const DEFAULT_I18N_MESSAGES_PATH: &str = "config/i18n.messages.json";

fn state() -> &'static RwLock<I18nState> {
    I18N_STATE.get_or_init(|| {
        let mut state = I18nState::new();
        let messages = load_messages_from_json().unwrap_or_default();
        if !messages.is_empty() {
            state.messages = messages;
        }
        RwLock::new(state)
    })
}

/// 初始化 i18n 配置，与配置文件保持一致。
pub fn configure_i18n(
    default_language: Option<String>,
    supported_languages: Option<Vec<String>>,
    aliases: Option<HashMap<String, String>>,
) {
    let mut guard = state().write();
    if let Some(value) = default_language {
        let cleaned = value.trim().to_string();
        if !cleaned.is_empty() {
            guard.default_language = cleaned;
        }
    }
    if let Some(values) = supported_languages {
        let cleaned: Vec<String> = values
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        if !cleaned.is_empty() {
            guard.supported_languages = cleaned;
        }
    }
    if let Some(extra) = aliases {
        for (key, value) in extra {
            let key = key.trim().to_lowercase();
            let value = value.trim().to_string();
            if key.is_empty() || value.is_empty() {
                continue;
            }
            guard.aliases.insert(key, value);
        }
    }
}

/// 在当前任务上下文中设置语言，返回可等待的执行结果。
pub async fn with_language<F, R>(language: String, fut: F) -> R
where
    F: Future<Output = R>,
{
    CURRENT_LANGUAGE.scope(language, fut).await
}

/// 获取当前上下文语言，未设置时回退默认值。
pub fn get_language() -> String {
    if let Ok(value) = CURRENT_LANGUAGE.try_with(|lang| lang.clone()) {
        return value;
    }
    get_default_language()
}

/// 获取默认语言。
pub fn get_default_language() -> String {
    state().read().default_language.clone()
}

/// 获取支持语言列表。
pub fn get_supported_languages() -> Vec<String> {
    state().read().supported_languages.clone()
}

/// 获取语言别名映射。
pub fn get_language_aliases() -> HashMap<String, String> {
    state().read().aliases.clone()
}

/// 输出 i18n 配置，供接口使用。
pub fn t(key: &str) -> String {
    t_with_params(key, &HashMap::new())
}

/// 翻译指定 key，并按占位符替换参数。
pub fn t_with_params(key: &str, params: &HashMap<String, String>) -> String {
    if key.trim().is_empty() {
        return "".to_string();
    }
    let language = get_language();
    let state = state().read();
    let entry = state.messages.get(key);
    let template = entry
        .and_then(|map| map.get(&language))
        .or_else(|| entry.and_then(|map| map.get(&state.default_language)))
        .map(|value| value.as_str())
        .unwrap_or(key);
    if params.is_empty() {
        return template.to_string();
    }
    format_template(template, params)
}

/// 使用指定语言翻译 key，忽略当前任务上下文语言。
pub fn t_with_params_in_language(
    key: &str,
    params: &HashMap<String, String>,
    language: &str,
) -> String {
    if key.trim().is_empty() {
        return "".to_string();
    }
    let normalized = normalize_language(Some(language), true);
    let state = state().read();
    let entry = state.messages.get(key);
    let template = entry
        .and_then(|map| map.get(&normalized))
        .or_else(|| entry.and_then(|map| map.get(&state.default_language)))
        .map(|value| value.as_str())
        .unwrap_or(key);
    if params.is_empty() {
        return template.to_string();
    }
    format_template(template, params)
}

/// 使用指定语言翻译 key（无参数）。
pub fn t_in_language(key: &str, language: &str) -> String {
    t_with_params_in_language(key, &HashMap::new(), language)
}

/// 获取某个翻译 key 的全部前缀，便于历史兼容。
pub fn get_known_prefixes(key: &str) -> Vec<String> {
    if key.trim().is_empty() {
        return Vec::new();
    }
    let state = state().read();
    let entry = match state.messages.get(key) {
        Some(entry) => entry,
        None => return Vec::new(),
    };
    let mut seen = HashMap::new();
    let mut output = Vec::new();
    for value in entry.values() {
        if seen.contains_key(value) {
            continue;
        }
        seen.insert(value.clone(), true);
        output.push(value.clone());
    }
    output
}

/// 解析语言码并按别名规范化。
pub fn normalize_language(raw: Option<&str>, fallback: bool) -> String {
    let raw = raw.unwrap_or("").trim();
    if raw.is_empty() {
        return if fallback {
            get_default_language()
        } else {
            "".to_string()
        };
    }
    for part in raw.split(',') {
        let code = part.split(';').next().unwrap_or("").trim();
        if let Some(normalized) = normalize_language_code(code) {
            return normalized;
        }
    }
    if fallback {
        get_default_language()
    } else {
        "".to_string()
    }
}

/// 按候选列表解析语言。
pub fn resolve_language<I>(candidates: I) -> String
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    for candidate in candidates {
        let text = candidate.as_ref().trim();
        if text.is_empty() {
            continue;
        }
        let normalized = normalize_language(Some(text), false);
        if !normalized.is_empty() {
            return normalized;
        }
    }
    get_default_language()
}

fn normalize_language_code(value: &str) -> Option<String> {
    let cleaned = value.trim();
    if cleaned.is_empty() {
        return None;
    }
    let lower = cleaned.to_lowercase();
    let state = state().read();
    if let Some(mapped) = state.aliases.get(&lower) {
        return Some(mapped.clone());
    }
    if state.supported_languages.iter().any(|lang| lang == cleaned) {
        return Some(cleaned.to_string());
    }
    None
}

fn format_template(template: &str, params: &HashMap<String, String>) -> String {
    static FORMAT_RE: OnceLock<Option<Regex>> = OnceLock::new();
    let Some(regex) = FORMAT_RE
        .get_or_init(|| compile_regex(r"\{([a-zA-Z0-9_]+)(:[^}]+)?\}", "format_placeholder"))
        .as_ref()
    else {
        return template.to_string();
    };
    regex
        .replace_all(template, |caps: &regex::Captures| {
            let key = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let fmt = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let Some(value) = params.get(key) else {
                return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            };
            if fmt.starts_with(':') {
                if let Some(output) = format_with_spec(value, &fmt[1..]) {
                    return output;
                }
            }
            value.clone()
        })
        .to_string()
}

fn compile_regex(pattern: &str, label: &str) -> Option<Regex> {
    match Regex::new(pattern) {
        Ok(regex) => Some(regex),
        Err(err) => {
            error!("invalid i18n regex {label}: {err}");
            None
        }
    }
}

fn format_with_spec(value: &str, spec: &str) -> Option<String> {
    if !spec.ends_with('d') {
        return None;
    }
    let width: usize = spec
        .trim_end_matches('d')
        .trim_start_matches('0')
        .parse()
        .ok()?;
    let number: i64 = value.parse().ok()?;
    if width == 0 {
        return Some(number.to_string());
    }
    Some(format!("{number:0width$}", width = width))
}

fn resolve_messages_path() -> PathBuf {
    let env_path = std::env::var("WUNDER_I18N_MESSAGES_PATH")
        .ok()
        .unwrap_or_default();
    let env_path = env_path.trim();
    if env_path.is_empty() {
        PathBuf::from(DEFAULT_I18N_MESSAGES_PATH)
    } else {
        PathBuf::from(env_path)
    }
}

fn load_messages_from_json() -> Option<HashMap<String, HashMap<String, String>>> {
    let path = resolve_messages_path();
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let content = content.trim_start_matches('\u{FEFF}');
    parse_json_messages(content)
}

fn parse_json_messages(text: &str) -> Option<HashMap<String, HashMap<String, String>>> {
    let value: Value = serde_json::from_str(text).ok()?;
    let Value::Object(map) = value else {
        return None;
    };
    let mut output: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (key, item) in map {
        let Value::Object(lang_map) = item else {
            continue;
        };
        let mut translations = HashMap::new();
        for (lang, value) in lang_map {
            if let Value::String(text) = value {
                if !text.trim().is_empty() {
                    translations.insert(lang, text);
                }
            }
        }
        if !translations.is_empty() {
            output.insert(key, translations);
        }
    }
    if output.is_empty() {
        None
    } else {
        Some(output)
    }
}
