use crate::config::Config;
use crate::i18n;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_SYSTEM_PACKS_ROOT: &str = "./data/prompt_templates";
const DEFAULT_USER_PROMPT_ROOT: &str = "./data/user_prompt_templates";
const PROMPTS_ROOT_ENV: &str = "WUNDER_PROMPTS_ROOT";

pub const DEFAULT_PACK_ID: &str = "default";

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
        return resolve_prompts_root();
    }
    resolve_system_packs_root(config).join(pack_id.trim())
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
    let settings_path = resolve_user_settings_path(config, user_id);
    let text = match std::fs::read_to_string(&settings_path) {
        Ok(value) => value,
        Err(_) => return DEFAULT_PACK_ID.to_string(),
    };
    let parsed = serde_json::from_str::<UserPromptTemplateSettingsFile>(&text)
        .unwrap_or_else(|_| UserPromptTemplateSettingsFile::default());
    let active = normalize_pack_id(parsed.active.as_deref());
    if active.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        return DEFAULT_PACK_ID.to_string();
    }
    if resolve_user_pack_root(config, user_id, &active).is_dir() {
        active
    } else {
        DEFAULT_PACK_ID.to_string()
    }
}

pub fn save_user_active_pack_id(
    config: &Config,
    user_id: &str,
    pack_id: &str,
) -> Result<(), String> {
    validate_pack_id(pack_id)?;
    let root = resolve_user_prompt_root(config, user_id);
    std::fs::create_dir_all(&root).map_err(|err| err.to_string())?;
    let settings = UserPromptTemplateSettingsFile {
        active: Some(normalize_pack_id(Some(pack_id))),
        updated_at: Some(now_ts()),
    };
    let text = serde_json::to_string_pretty(&settings).map_err(|err| err.to_string())?;
    std::fs::write(resolve_user_settings_path(config, user_id), text).map_err(|err| err.to_string())
}

fn resolve_user_settings_path(config: &Config, user_id: &str) -> PathBuf {
    resolve_user_prompt_root(config, user_id).join("settings.json")
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

fn resolve_prompts_root() -> PathBuf {
    let root = std::env::var(PROMPTS_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    normalize_prompts_root(root)
}

fn normalize_prompts_root(root: PathBuf) -> PathBuf {
    if root.join("prompts").is_dir() {
        return root;
    }
    let looks_like_prompts_dir = root
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("prompts"))
        .unwrap_or(false);
    if looks_like_prompts_dir && (root.join("zh").is_dir() || root.join("en").is_dir()) {
        if let Some(parent) = root.parent() {
            return parent.to_path_buf();
        }
    }
    root
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
