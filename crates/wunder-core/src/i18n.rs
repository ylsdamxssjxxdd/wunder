use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

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

const DEFAULT_I18N_MESSAGES_PATH: &str = "config/i18n.messages.json";
const DEFAULT_I18N_MESSAGES_EMBED: &str = include_str!("../../../config/i18n.messages.json");

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

fn read_state() -> std::sync::RwLockReadGuard<'static, I18nState> {
    state().read().unwrap_or_else(|err| err.into_inner())
}

fn write_state() -> std::sync::RwLockWriteGuard<'static, I18nState> {
    state().write().unwrap_or_else(|err| err.into_inner())
}

pub fn configure_i18n(
    default_language: Option<String>,
    supported_languages: Option<Vec<String>>,
    aliases: Option<HashMap<String, String>>,
) {
    let mut guard = write_state();
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

pub fn get_default_language() -> String {
    read_state().default_language.clone()
}

pub fn get_supported_languages() -> Vec<String> {
    read_state().supported_languages.clone()
}

pub fn get_language_aliases() -> HashMap<String, String> {
    read_state().aliases.clone()
}

pub fn t(key: &str) -> String {
    t_with_params(key, &HashMap::new())
}

pub fn t_with_params(key: &str, params: &HashMap<String, String>) -> String {
    let language = get_default_language();
    t_with_params_in_language(key, params, &language)
}

pub fn t_with_params_in_language(
    key: &str,
    params: &HashMap<String, String>,
    language: &str,
) -> String {
    if key.trim().is_empty() {
        return String::new();
    }
    let normalized = normalize_language(Some(language), true);
    let state = read_state();
    let template = find_template(&state.messages, key, &normalized, &state.default_language)
        .or_else(|| {
            find_template(
                embedded_messages(),
                key,
                &normalized,
                &state.default_language,
            )
        })
        .unwrap_or_else(|| key.to_string());
    if params.is_empty() {
        return template;
    }
    format_template(&template, params)
}

pub fn t_in_language(key: &str, language: &str) -> String {
    t_with_params_in_language(key, &HashMap::new(), language)
}

pub fn get_known_prefixes(key: &str) -> Vec<String> {
    if key.trim().is_empty() {
        return Vec::new();
    }
    let state = read_state();
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

pub fn normalize_language(raw: Option<&str>, fallback: bool) -> String {
    let raw = raw.unwrap_or("").trim();
    if raw.is_empty() {
        return if fallback {
            get_default_language()
        } else {
            String::new()
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
        String::new()
    }
}

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
    let state = read_state();
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
        .get_or_init(|| Regex::new(r"\{([a-zA-Z0-9_]+)(:[^}]+)?\}").ok())
        .as_ref()
    else {
        return template.to_string();
    };
    regex
        .replace_all(template, |caps: &regex::Captures| {
            let key = caps.get(1).map(|item| item.as_str()).unwrap_or("");
            let fmt = caps.get(2).map(|item| item.as_str()).unwrap_or("");
            let Some(value) = params.get(key) else {
                return caps
                    .get(0)
                    .map(|item| item.as_str())
                    .unwrap_or("")
                    .to_string();
            };
            if let Some(spec) = fmt.strip_prefix(':') {
                if let Some(output) = format_with_spec(value, spec) {
                    return output;
                }
            }
            value.clone()
        })
        .to_string()
}

fn embedded_messages() -> &'static HashMap<String, HashMap<String, String>> {
    static EMBEDDED: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();
    EMBEDDED.get_or_init(|| {
        let content = DEFAULT_I18N_MESSAGES_EMBED.trim_start_matches('\u{FEFF}');
        parse_json_messages(content).unwrap_or_default()
    })
}

fn find_template(
    messages: &HashMap<String, HashMap<String, String>>,
    key: &str,
    language: &str,
    default_language: &str,
) -> Option<String> {
    let entry = messages.get(key)?;
    entry
        .get(language)
        .or_else(|| entry.get(default_language))
        .cloned()
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
    if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let content = content.trim_start_matches('\u{FEFF}');
                if let Some(messages) = parse_json_messages(content) {
                    return Some(messages);
                }
                eprintln!("i18n messages parse failed: {}", path.display());
            }
            Err(err) => {
                eprintln!("i18n messages read failed: {}, {}", path.display(), err);
            }
        }
    }
    let embedded = DEFAULT_I18N_MESSAGES_EMBED.trim_start_matches('\u{FEFF}');
    parse_json_messages(embedded)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_language_accepts_aliases_and_accept_language_header() {
        assert_eq!(
            normalize_language(Some("en-US,en;q=0.9,zh-CN;q=0.8"), true),
            "en-US"
        );
        assert_eq!(normalize_language(Some("zh-hans"), true), "zh-CN");
    }

    #[test]
    fn t_with_params_formats_numeric_width() {
        let mut params = HashMap::new();
        params.insert("value".to_string(), "7".to_string());
        let formatted = format_template("id-{value:03d}", &params);
        assert_eq!(formatted, "id-007");
    }
}
