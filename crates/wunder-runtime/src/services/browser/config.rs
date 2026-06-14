use crate::config::Config;
use std::path::PathBuf;

pub const BROWSER_DEPLOYMENT_AUTO: &str = "auto";
pub const BROWSER_DEPLOYMENT_DESKTOP_EMBEDDED: &str = "desktop-embedded";
pub const BROWSER_DEPLOYMENT_SERVER_EMBEDDED: &str = "server-embedded";
pub const BROWSER_DEPLOYMENT_SIDECAR: &str = "sidecar";
pub const DEFAULT_BROWSER_PROFILE: &str = "managed";

#[derive(Debug, Clone)]
pub struct EffectiveBrowserConfig {
    pub enabled: bool,
    pub tool_visible: bool,
    pub deployment: String,
    pub default_profile: String,
    pub control_host: String,
    pub control_port: u16,
    pub control_auth_token: Option<String>,
    pub control_public_base_url: Option<String>,
    pub headless: bool,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub timeout_secs: u64,
    pub idle_timeout_secs: u64,
    pub max_sessions: usize,
    pub max_tabs_per_session: usize,
    pub max_snapshot_chars: usize,
    pub max_download_bytes: usize,
    pub python_path: Option<String>,
    pub browsers_path: Option<String>,
    pub launch_args: Vec<String>,
    pub allow_private_network: bool,
    pub hostname_allowlist: Vec<String>,
    pub deny_file_scheme: bool,
    pub docker_enabled: bool,
    pub docker_use_no_sandbox: bool,
    pub docker_disable_dev_shm_usage: bool,
    pub docker_downloads_root: Option<PathBuf>,
}

impl EffectiveBrowserConfig {
    pub fn profiles(&self) -> Vec<String> {
        vec![self.default_profile.clone()]
    }
}

pub fn browser_runtime_enabled(config: &Config) -> bool {
    config.browser.enabled || legacy_browser_runtime_enabled(config)
}

pub fn browser_tools_enabled(config: &Config) -> bool {
    config.tools.browser.enabled && browser_runtime_enabled(config)
}

pub fn effective_browser_config(config: &Config) -> EffectiveBrowserConfig {
    let legacy_runtime = legacy_browser_runtime_enabled(config) && !config.browser.enabled;
    let deployment = if legacy_runtime {
        BROWSER_DEPLOYMENT_DESKTOP_EMBEDDED.to_string()
    } else {
        normalize_browser_deployment(&config.browser.deployment)
    };
    let allow_private_network = if legacy_runtime {
        true
    } else {
        config.browser.security.allow_private_network
    };
    let headless = if config.browser.docker.enabled && config.browser.docker.force_headless {
        true
    } else if legacy_runtime {
        config.tools.browser.headless
    } else {
        config.browser.playwright.headless
    };
    let mut launch_args = if legacy_runtime {
        Vec::new()
    } else {
        config.browser.playwright.launch_args.clone()
    };
    if config.browser.docker.enabled {
        launch_args.extend(config.browser.docker.extra_launch_args.clone());
    }
    EffectiveBrowserConfig {
        enabled: browser_runtime_enabled(config),
        tool_visible: browser_tools_enabled(config),
        deployment,
        default_profile: normalize_profile_name(&config.browser.default_profile),
        control_host: normalize_host(&config.browser.control.host),
        control_port: config.browser.control.port.max(1),
        control_auth_token: trim_option(config.browser.control.auth_token.clone()),
        control_public_base_url: trim_option(config.browser.control.public_base_url.clone()),
        headless,
        viewport_width: if legacy_runtime {
            config.tools.browser.viewport_width.max(1)
        } else {
            config.browser.playwright.viewport_width.max(1)
        },
        viewport_height: if legacy_runtime {
            config.tools.browser.viewport_height.max(1)
        } else {
            config.browser.playwright.viewport_height.max(1)
        },
        timeout_secs: if legacy_runtime {
            config.tools.browser.timeout_secs.max(1)
        } else {
            config.browser.playwright.timeout_secs.max(1)
        },
        idle_timeout_secs: if legacy_runtime {
            config.tools.browser.idle_timeout_secs
        } else {
            config.browser.limits.idle_timeout_secs
        }
        .max(1),
        max_sessions: if legacy_runtime {
            config.tools.browser.max_sessions.max(1)
        } else {
            config.browser.limits.max_sessions.max(1)
        },
        max_tabs_per_session: config.browser.limits.max_tabs_per_session.max(1),
        max_snapshot_chars: config.browser.limits.max_snapshot_chars.max(512),
        max_download_bytes: config.browser.limits.max_download_bytes.max(1024),
        python_path: if legacy_runtime {
            trim_option(config.tools.browser.python_path.clone())
        } else {
            trim_option(config.browser.playwright.python_path.clone())
                .or_else(|| trim_option(config.tools.browser.python_path.clone()))
        },
        browsers_path: trim_option(config.browser.playwright.browsers_path.clone()),
        launch_args,
        allow_private_network,
        hostname_allowlist: config
            .browser
            .security
            .hostname_allowlist
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect(),
        deny_file_scheme: if legacy_runtime {
            false
        } else {
            config.browser.security.deny_file_scheme
        },
        docker_enabled: config.browser.docker.enabled,
        docker_use_no_sandbox: config.browser.docker.use_no_sandbox,
        docker_disable_dev_shm_usage: config.browser.docker.disable_dev_shm_usage,
        docker_downloads_root: normalize_path(&config.browser.docker.downloads_root),
    }
}

fn legacy_browser_runtime_enabled(config: &Config) -> bool {
    config.server.mode.trim().eq_ignore_ascii_case("desktop") && config.tools.browser.enabled
}

fn normalize_browser_deployment(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case(BROWSER_DEPLOYMENT_DESKTOP_EMBEDDED) {
        BROWSER_DEPLOYMENT_DESKTOP_EMBEDDED.to_string()
    } else if trimmed.eq_ignore_ascii_case(BROWSER_DEPLOYMENT_SERVER_EMBEDDED) {
        BROWSER_DEPLOYMENT_SERVER_EMBEDDED.to_string()
    } else if trimmed.eq_ignore_ascii_case(BROWSER_DEPLOYMENT_SIDECAR) {
        BROWSER_DEPLOYMENT_SIDECAR.to_string()
    } else {
        BROWSER_DEPLOYMENT_AUTO.to_string()
    }
}

fn normalize_profile_name(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        DEFAULT_BROWSER_PROFILE.to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_host(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "127.0.0.1".to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_path(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn trim_option(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{browser_runtime_enabled, browser_tools_enabled, effective_browser_config};
    use crate::config::Config;

    #[test]
    fn legacy_desktop_browser_stays_available() {
        let mut config = Config::default();
        config.server.mode = "desktop".to_string();
        config.tools.browser.enabled = true;
        assert!(browser_runtime_enabled(&config));
        assert!(browser_tools_enabled(&config));
        let effective = effective_browser_config(&config);
        assert_eq!(effective.deployment, "desktop-embedded");
        assert!(effective.allow_private_network);
    }

    #[test]
    fn top_level_browser_supports_server_mode() {
        let mut config = Config::default();
        config.server.mode = "api".to_string();
        config.browser.enabled = true;
        config.tools.browser.enabled = true;
        assert!(browser_runtime_enabled(&config));
        assert!(browser_tools_enabled(&config));
        let effective = effective_browser_config(&config);
        assert_eq!(effective.deployment, "auto");
    }
}
