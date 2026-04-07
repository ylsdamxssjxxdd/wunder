use crate::config::Config;
use crate::core::repo_assets;
use crate::i18n;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_SYSTEM_PACKS_ROOT: &str = "./config/data/prompt_templates";
const DEFAULT_USER_PROMPT_ROOT: &str = "./config/data/user_prompt_templates";
const PROMPTS_ROOT_ENV: &str = "WUNDER_PROMPTS_ROOT";
const USER_ACTIVE_PACK_CACHE_MAX_ITEMS: usize = 512;

pub const DEFAULT_PACK_ID: &str = "default";
pub const DEFAULT_ZH_PACK_ID: &str = "default-zh";
pub const DEFAULT_EN_PACK_ID: &str = "default-en";

pub const SYSTEM_SEGMENTS: &[(&str, &str)] = &[
    ("role", "role.txt"),
    ("engineering", "engineering.txt"),
    ("tools_protocol", "tools_protocol.txt"),
    ("skills_protocol", "skills_protocol.txt"),
    ("memory", "memory.txt"),
    ("extra", "extra.txt"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserPromptTemplateSettingsFile {
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    updated_at: Option<f64>,
}

impl Default for UserPromptTemplateSettingsFile {
    fn default() -> Self {
        Self {
            active: Some(DEFAULT_PACK_ID.to_string()),
            updated_at: Some(now_ts()),
        }
    }
}

#[derive(Clone)]
struct UserActivePackCacheEntry {
    revision: u64,
    active_pack_id: String,
    follows_system_language_default: bool,
    language_default_pack_id: String,
}

fn user_active_pack_cache() -> &'static Mutex<HashMap<String, UserActivePackCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, UserActivePackCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn normalize_pack_id(raw: Option<&str>) -> String {
    let cleaned = raw.unwrap_or("").trim();
    if cleaned.is_empty() {
        return DEFAULT_PACK_ID.to_string();
    }
    cleaned.to_string()
}

pub fn validate_pack_id(pack_id: &str) -> Result<(), String> {
    let cleaned = pack_id.trim();
    if cleaned.is_empty() {
        return Err(i18n::t("error.param_required"));
    }
    if cleaned.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        return Ok(());
    }
    if cleaned.len() > 64 {
        return Err("pack_id too long".to_string());
    }
    if !cleaned
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err("pack_id contains invalid characters".to_string());
    }
    Ok(())
}

pub fn normalize_locale(raw: Option<&str>) -> String {
    let cleaned = raw.unwrap_or("").trim().to_ascii_lowercase();
    if cleaned.starts_with("en") {
        "en".to_string()
    } else if cleaned.starts_with("zh") {
        "zh".to_string()
    } else {
        let system = i18n::get_language().to_ascii_lowercase();
        if system.starts_with("en") {
            "en".to_string()
        } else {
            "zh".to_string()
        }
    }
}

pub fn resolve_segment_file_name(key: &str) -> Option<&'static str> {
    SYSTEM_SEGMENTS
        .iter()
        .find(|(segment_key, _)| segment_key.eq_ignore_ascii_case(key.trim()))
        .map(|(_, file)| *file)
}

pub fn resolve_segment_path(pack_root: &Path, locale: &str, key: &str) -> Result<PathBuf, String> {
    let Some(file_name) = resolve_segment_file_name(key) else {
        return Err(format!("unknown segment key: {key}"));
    };
    Ok(pack_root.join(format!("prompts/{locale}/system/{file_name}")))
}

pub fn resolve_system_active_pack_id(config: &Config) -> String {
    let active = config.prompt_templates.active.trim();
    if active.is_empty() {
        DEFAULT_PACK_ID.to_string()
    } else {
        active.to_string()
    }
}

pub fn resolve_system_pack_root(config: &Config, pack_id: &str) -> PathBuf {
    if pack_id.trim().eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        return resolve_default_prompt_pack_root();
    }
    resolve_system_packs_root(config).join(pack_id.trim())
}

pub fn resolve_default_user_pack_id_for_language(raw_language: Option<&str>) -> String {
    if normalize_locale(raw_language).eq_ignore_ascii_case("en") {
        DEFAULT_EN_PACK_ID.to_string()
    } else {
        DEFAULT_ZH_PACK_ID.to_string()
    }
}

pub fn resolve_default_user_pack_id() -> String {
    resolve_default_user_pack_id_for_language(None)
}

pub fn is_builtin_user_pack_id(pack_id: &str) -> bool {
    let cleaned = pack_id.trim();
    cleaned.eq_ignore_ascii_case(DEFAULT_PACK_ID)
        || cleaned.eq_ignore_ascii_case(DEFAULT_ZH_PACK_ID)
        || cleaned.eq_ignore_ascii_case(DEFAULT_EN_PACK_ID)
}

pub fn builtin_user_pack_locale(pack_id: &str) -> Option<&'static str> {
    let cleaned = pack_id.trim();
    if cleaned.eq_ignore_ascii_case(DEFAULT_ZH_PACK_ID) {
        Some("zh")
    } else if cleaned.eq_ignore_ascii_case(DEFAULT_EN_PACK_ID) {
        Some("en")
    } else {
        None
    }
}

pub fn resolve_user_prompt_root(config: &Config, user_id: &str) -> PathBuf {
    resolve_user_prompt_templates_root(config).join(safe_user_prompt_key(user_id))
}

pub fn resolve_user_packs_root(config: &Config, user_id: &str) -> PathBuf {
    resolve_user_prompt_root(config, user_id).join("packs")
}

pub fn resolve_user_pack_root(config: &Config, user_id: &str, pack_id: &str) -> PathBuf {
    resolve_user_packs_root(config, user_id).join(pack_id.trim())
}

pub fn safe_user_prompt_key(user_id: &str) -> String {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return "anonymous".to_string();
    }
    let mut output = String::with_capacity(cleaned.len());
    for ch in cleaned.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.trim().is_empty() {
        "anonymous".to_string()
    } else {
        output
    }
}

pub fn load_user_active_pack_id(config: &Config, user_id: &str) -> String {
    let cache_key = safe_user_prompt_key(user_id);
    let revision = crate::prompting::system_prompt_templates_revision();
    let language_default_pack_id = resolve_default_user_pack_id();
    if let Some(entry) = user_active_pack_cache()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .get(&cache_key)
        .cloned()
    {
        if entry.revision == revision
            && (!entry.follows_system_language_default
                || entry.language_default_pack_id == language_default_pack_id)
        {
            return entry.active_pack_id;
        }
    }

    let settings_path = resolve_user_settings_path(config, user_id);
    let text = match std::fs::read_to_string(&settings_path) {
        Ok(value) => value,
        Err(_) => {
            store_cached_user_active_pack_id(
                &cache_key,
                revision,
                &language_default_pack_id,
                true,
                &language_default_pack_id,
            );
            return language_default_pack_id;
        }
    };
    let parsed = serde_json::from_str::<UserPromptTemplateSettingsFile>(&text)
        .unwrap_or_else(|_| UserPromptTemplateSettingsFile::default());
    let active = normalize_pack_id(parsed.active.as_deref());
    let (resolved, follows_system_language_default) =
        if active.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
            (language_default_pack_id.clone(), true)
        } else if builtin_user_pack_locale(&active).is_some() {
            (active, false)
        } else if resolve_user_pack_root(config, user_id, &active).is_dir() {
            (active, false)
        } else {
            (language_default_pack_id.clone(), true)
        };
    store_cached_user_active_pack_id(
        &cache_key,
        revision,
        &resolved,
        follows_system_language_default,
        &language_default_pack_id,
    );
    resolved
}

pub fn save_user_active_pack_id(
    config: &Config,
    user_id: &str,
    pack_id: &str,
) -> Result<(), String> {
    validate_pack_id(pack_id)?;
    let root = resolve_user_prompt_root(config, user_id);
    std::fs::create_dir_all(&root).map_err(|err| err.to_string())?;
    let active = normalize_pack_id(Some(pack_id));
    let settings = UserPromptTemplateSettingsFile {
        active: Some(active),
        updated_at: Some(now_ts()),
    };
    let text = serde_json::to_string_pretty(&settings).map_err(|err| err.to_string())?;
    std::fs::write(resolve_user_settings_path(config, user_id), text)
        .map_err(|err| err.to_string())?;
    clear_cached_user_active_pack_id(user_id);
    Ok(())
}

fn resolve_user_settings_path(config: &Config, user_id: &str) -> PathBuf {
    resolve_user_prompt_root(config, user_id).join("settings.json")
}

fn clear_cached_user_active_pack_id(user_id: &str) {
    user_active_pack_cache()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .remove(&safe_user_prompt_key(user_id));
}

fn store_cached_user_active_pack_id(
    cache_key: &str,
    revision: u64,
    active_pack_id: &str,
    follows_system_language_default: bool,
    language_default_pack_id: &str,
) {
    let mut cache = user_active_pack_cache()
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    if cache.len() >= USER_ACTIVE_PACK_CACHE_MAX_ITEMS && !cache.contains_key(cache_key) {
        cache.clear();
    }
    cache.insert(
        cache_key.to_string(),
        UserActivePackCacheEntry {
            revision,
            active_pack_id: active_pack_id.to_string(),
            follows_system_language_default,
            language_default_pack_id: language_default_pack_id.to_string(),
        },
    );
}

fn resolve_system_packs_root(config: &Config) -> PathBuf {
    let root = config.prompt_templates.root.trim();
    let selected = if root.is_empty() {
        DEFAULT_SYSTEM_PACKS_ROOT
    } else {
        root
    };
    let path = PathBuf::from(selected);
    if path.is_absolute() {
        path
    } else {
        resolve_prompts_root().join(path)
    }
}

fn resolve_user_prompt_templates_root(_config: &Config) -> PathBuf {
    let path = PathBuf::from(DEFAULT_USER_PROMPT_ROOT);
    if path.is_absolute() {
        path
    } else {
        resolve_prompts_root().join(path)
    }
}

pub fn resolve_prompts_root() -> PathBuf {
    for candidate in resolve_prompts_root_candidates() {
        let normalized = normalize_prompts_root(candidate);
        if repo_assets::looks_like_repo_root(&normalized) {
            return normalized;
        }
    }
    normalize_prompts_root(PathBuf::from("."))
}

pub fn resolve_default_prompt_pack_root() -> PathBuf {
    repo_assets::default_prompt_pack_root(&resolve_prompts_root())
}

fn resolve_prompts_root_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    push_candidate(
        &mut candidates,
        std::env::var(PROMPTS_ROOT_ENV)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
    );
    push_candidate(&mut candidates, std::env::current_dir().ok());
    if let Ok(exe) = std::env::current_exe() {
        if let Some(app_dir) = exe.parent() {
            push_candidate(&mut candidates, Some(app_dir.to_path_buf()));
            push_candidate(&mut candidates, Some(app_dir.join("resources")));
            if let Some(parent) = app_dir.parent() {
                push_candidate(&mut candidates, Some(parent.join("Resources")));
            }
        }
    }
    push_candidate(
        &mut candidates,
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
    );
    candidates
}

fn push_candidate(candidates: &mut Vec<PathBuf>, candidate: Option<PathBuf>) {
    let Some(path) = candidate else {
        return;
    };
    if path.as_os_str().is_empty() {
        return;
    }
    if candidates.iter().any(|item| item == &path) {
        return;
    }
    candidates.push(path);
}

fn normalize_prompts_root(root: PathBuf) -> PathBuf {
    repo_assets::normalize_repo_root_candidate(&root)
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::{
        builtin_user_pack_locale, is_builtin_user_pack_id, normalize_locale,
        resolve_default_user_pack_id_for_language, DEFAULT_EN_PACK_ID, DEFAULT_PACK_ID,
        DEFAULT_ZH_PACK_ID,
    };

    #[test]
    fn resolve_default_user_pack_id_for_language_tracks_language() {
        assert_eq!(
            resolve_default_user_pack_id_for_language(Some("zh-CN")),
            DEFAULT_ZH_PACK_ID
        );
        assert_eq!(
            resolve_default_user_pack_id_for_language(Some("en-US")),
            DEFAULT_EN_PACK_ID
        );
        assert_eq!(
            resolve_default_user_pack_id_for_language(Some("en")),
            DEFAULT_EN_PACK_ID
        );
    }

    #[test]
    fn builtin_user_pack_helpers_cover_builtin_ids() {
        assert!(is_builtin_user_pack_id(DEFAULT_PACK_ID));
        assert!(is_builtin_user_pack_id(DEFAULT_ZH_PACK_ID));
        assert!(is_builtin_user_pack_id(DEFAULT_EN_PACK_ID));
        assert_eq!(builtin_user_pack_locale(DEFAULT_ZH_PACK_ID), Some("zh"));
        assert_eq!(builtin_user_pack_locale(DEFAULT_EN_PACK_ID), Some("en"));
        assert_eq!(builtin_user_pack_locale(DEFAULT_PACK_ID), None);
    }

    #[test]
    fn normalize_locale_maps_non_empty_input() {
        assert_eq!(normalize_locale(Some("zh-Hans")), "zh");
        assert_eq!(normalize_locale(Some("en-GB")), "en");
    }
}
