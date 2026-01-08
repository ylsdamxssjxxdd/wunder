// i18n 支持：语言解析、上下文管理与翻译文本读取。
use regex::Regex;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{OnceLock, RwLock};
use tokio::task_local;

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

fn state() -> &'static RwLock<I18nState> {
    I18N_STATE.get_or_init(|| {
        let mut state = I18nState::new();
        if let Some(messages) = load_messages_from_python() {
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
    let mut guard = state().write().expect("i18n state poisoned");
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
    state()
        .read()
        .expect("i18n state poisoned")
        .default_language
        .clone()
}

/// 获取支持语言列表。
pub fn get_supported_languages() -> Vec<String> {
    state()
        .read()
        .expect("i18n state poisoned")
        .supported_languages
        .clone()
}

/// 获取语言别名映射。
pub fn get_language_aliases() -> HashMap<String, String> {
    state().read().expect("i18n state poisoned").aliases.clone()
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
    let state = state().read().expect("i18n state poisoned");
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

/// 获取某个翻译 key 的全部前缀，便于历史兼容。
pub fn get_known_prefixes(key: &str) -> Vec<String> {
    if key.trim().is_empty() {
        return Vec::new();
    }
    let state = state().read().expect("i18n state poisoned");
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
    let state = state().read().expect("i18n state poisoned");
    if let Some(mapped) = state.aliases.get(&lower) {
        return Some(mapped.clone());
    }
    if state.supported_languages.iter().any(|lang| lang == cleaned) {
        return Some(cleaned.to_string());
    }
    None
}

fn format_template(template: &str, params: &HashMap<String, String>) -> String {
    static FORMAT_RE: OnceLock<Regex> = OnceLock::new();
    let regex = FORMAT_RE
        .get_or_init(|| Regex::new(r"\{([a-zA-Z0-9_]+)(:[^}]+)?\}").expect("format regex invalid"));
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

fn load_messages_from_python() -> Option<HashMap<String, HashMap<String, String>>> {
    let path = std::path::Path::new("app/core/i18n.py");
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    parse_messages(&content)
}

fn parse_messages(text: &str) -> Option<HashMap<String, HashMap<String, String>>> {
    let mut result: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut in_messages = false;
    let mut current_key: Option<String> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.starts_with("_MESSAGES") && line.contains('{') {
            in_messages = true;
            continue;
        }
        if !in_messages {
            continue;
        }
        if line.starts_with('}') {
            if current_key.is_some() {
                current_key = None;
                continue;
            }
            break;
        }
        if line.starts_with('"') && line.contains("\": {") {
            if let Some(key) = parse_quoted_key(line) {
                current_key = Some(key.clone());
                result.entry(key).or_default();
            }
            continue;
        }
        if let Some(key) = current_key.clone() {
            if line.starts_with('"') {
                if let Some((lang, value)) = parse_lang_entry(line) {
                    result.entry(key.clone()).or_default().insert(lang, value);
                }
            }
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

fn parse_quoted_key(line: &str) -> Option<String> {
    let mut chars = line.chars();
    if chars.next()? != '"' {
        return None;
    }
    let mut key = String::new();
    for ch in chars.by_ref() {
        if ch == '"' {
            break;
        }
        key.push(ch);
    }
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

fn parse_lang_entry(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim().trim_end_matches(',');
    let mut parts = trimmed.splitn(2, ':');
    let key_part = parts.next()?.trim();
    let value_part = parts.next()?.trim();
    let lang = strip_quotes(key_part)?;
    let value = strip_quotes(value_part)?;
    Some((lang, value))
}

fn strip_quotes(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.len() < 2 {
        return None;
    }
    let quote = trimmed.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = trimmed.chars().last()?;
    if end != quote {
        return None;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    Some(inner.to_string())
}
