use crate::args::GlobalArgs;

pub fn resolve_cli_language(global: &GlobalArgs) -> String {
    let mut candidates = Vec::new();
    if let Some(language) = global
        .language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        candidates.push(language.to_string());
    }
    wunder_server::i18n::resolve_language(candidates.iter().map(String::as_str))
}

pub fn is_zh_language(language: &str) -> bool {
    language.trim().to_ascii_lowercase().starts_with("zh")
}

pub fn tr(language: &str, zh: &str, en: &str) -> String {
    if is_zh_language(language) {
        zh.to_string()
    } else {
        en.to_string()
    }
}
