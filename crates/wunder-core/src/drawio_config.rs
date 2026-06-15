use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawioConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub editor_url: Option<String>,
    #[serde(default = "default_drawio_max_file_bytes")]
    pub max_file_bytes: usize,
}

impl Default for DrawioConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            editor_url: None,
            max_file_bytes: default_drawio_max_file_bytes(),
        }
    }
}

impl DrawioConfig {
    pub fn enabled(&self) -> bool {
        env::var("WUNDER_DRAWIO_ENABLED")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .and_then(|value| match value.as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            })
            .unwrap_or(self.enabled)
    }

    pub fn editor_url(&self) -> Option<String> {
        clean_inline_or_env(self.editor_url.as_deref(), "WUNDER_DRAWIO_EDITOR_URL")
            .map(trim_trailing_slashes)
    }

    pub fn max_file_bytes(&self) -> usize {
        self.max_file_bytes.clamp(1024, 200 * 1024 * 1024)
    }
}

fn clean_inline_or_env(inline: Option<&str>, env_name: &str) -> Option<String> {
    inline
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| !(value.starts_with("${") && value.ends_with('}')))
        .map(str::to_string)
        .or_else(|| {
            env::var(env_name)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

fn trim_trailing_slashes(value: String) -> String {
    value.trim_end_matches('/').to_string()
}

fn default_drawio_max_file_bytes() -> usize {
    50 * 1024 * 1024
}
