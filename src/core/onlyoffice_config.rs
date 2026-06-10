use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlyOfficeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub document_server_url: Option<String>,
    #[serde(default)]
    pub internal_document_server_url: Option<String>,
    #[serde(default)]
    pub api_url: Option<String>,
    #[serde(default)]
    pub public_base_url: Option<String>,
    #[serde(default)]
    pub jwt_secret: Option<String>,
    #[serde(default = "default_onlyoffice_jwt_header")]
    pub jwt_header: String,
    #[serde(default = "default_onlyoffice_token_ttl_s")]
    pub token_ttl_s: u64,
    #[serde(default = "default_onlyoffice_request_timeout_s")]
    pub request_timeout_s: u64,
    #[serde(default = "default_onlyoffice_max_download_bytes")]
    pub max_download_bytes: usize,
}

impl Default for OnlyOfficeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            document_server_url: None,
            internal_document_server_url: None,
            api_url: None,
            public_base_url: None,
            jwt_secret: None,
            jwt_header: default_onlyoffice_jwt_header(),
            token_ttl_s: default_onlyoffice_token_ttl_s(),
            request_timeout_s: default_onlyoffice_request_timeout_s(),
            max_download_bytes: default_onlyoffice_max_download_bytes(),
        }
    }
}

impl OnlyOfficeConfig {
    pub fn document_server_url(&self) -> Option<String> {
        clean_inline_or_env(
            self.document_server_url.as_deref(),
            "WUNDER_ONLYOFFICE_DOCUMENT_SERVER_URL",
        )
        .map(trim_trailing_slashes)
    }

    pub fn internal_document_server_url(&self) -> Option<String> {
        clean_inline_or_env(
            self.internal_document_server_url.as_deref(),
            "WUNDER_ONLYOFFICE_INTERNAL_DOCUMENT_SERVER_URL",
        )
        .map(trim_trailing_slashes)
    }

    pub fn api_url(&self) -> Option<String> {
        clean_inline_or_env(self.api_url.as_deref(), "WUNDER_ONLYOFFICE_API_URL").or_else(|| {
            self.document_server_url()
                .map(|base| format!("{base}/web-apps/apps/api/documents/api.js"))
        })
    }

    pub fn public_base_url(&self) -> Option<String> {
        clean_inline_or_env(self.public_base_url.as_deref(), "WUNDER_PUBLIC_BASE_URL")
            .or_else(|| clean_inline_or_env(None, "WUNDER_ONLYOFFICE_PUBLIC_BASE_URL"))
            .map(trim_trailing_slashes)
    }

    pub fn jwt_secret(&self) -> Option<String> {
        clean_inline_or_env(self.jwt_secret.as_deref(), "WUNDER_ONLYOFFICE_JWT_SECRET")
    }

    pub fn token_ttl_s(&self) -> u64 {
        self.token_ttl_s.clamp(60, 24 * 60 * 60)
    }

    pub fn request_timeout_s(&self) -> u64 {
        self.request_timeout_s.clamp(5, 300)
    }

    pub fn max_download_bytes(&self) -> usize {
        self.max_download_bytes.clamp(1024, 1024 * 1024 * 1024)
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

fn default_onlyoffice_jwt_header() -> String {
    "Authorization".to_string()
}

fn default_onlyoffice_token_ttl_s() -> u64 {
    3600
}

fn default_onlyoffice_request_timeout_s() -> u64 {
    60
}

fn default_onlyoffice_max_download_bytes() -> usize {
    1024 * 1024 * 1024
}
