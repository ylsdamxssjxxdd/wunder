use crate::config::Config;
use std::path::PathBuf;

pub const BROWSER_DEPLOYMENT_AUTO: &str = "auto";
pub const BROWSER_DEPLOYMENT_DESKTOP_EMBEDDED: &str = "desktop-embedded";
pub const BROWSER_DEPLOYMENT_SERVER_EMBEDDED: &str = "server-embedded";
pub const BROWSER_DEPLOYMENT_SIDECAR: &str = "sidecar";
/// 浏览器进程跑在沙盒容器里：主进程只保留会话表与预热池，不启动 Python/Chromium。
pub const BROWSER_DEPLOYMENT_SANDBOX: &str = "sandbox";
pub const DEFAULT_BROWSER_PROFILE: &str = "managed";
pub const BROWSER_LAUNCH_MODE: &str = "launch";
pub const BROWSER_LAUNCH_MODE_ATTACH: &str = "attach";
pub const BROWSER_LAUNCH_MODE_CDP: &str = "cdp";

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
    pub prewarm_sessions: usize,
    pub max_snapshot_chars: usize,
    pub max_download_bytes: usize,
    pub python_path: Option<String>,
    pub browsers_path: Option<String>,
    pub launch_args: Vec<String>,
    pub launch_mode: String,
    pub channel: Option<String>,
    pub executable_path: Option<String>,
    pub attach_endpoint: Option<String>,
    pub cdp_fallback: bool,
    pub cdp_port: u16,
    pub allow_private_network: bool,
    pub hostname_allowlist: Vec<String>,
    pub deny_file_scheme: bool,
    pub docker_enabled: bool,
    pub docker_use_no_sandbox: bool,
    pub docker_disable_dev_shm_usage: bool,
    pub docker_downloads_root: Option<PathBuf>,
    /// Set only when `deployment = sandbox`: the endpoint hosting the bridge
    /// processes. `None` means the bridge runs as a local child process.
    pub sandbox_endpoint: Option<String>,
}

impl EffectiveBrowserConfig {
    pub fn profiles(&self) -> Vec<String> {
        vec![self.default_profile.clone()]
    }

    pub fn runs_in_sandbox(&self) -> bool {
        self.sandbox_endpoint.is_some()
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
    // 沙盒部署下浏览器进程不在本进程树里，主进程只负责转发行命令。
    let sandbox_endpoint = if deployment == BROWSER_DEPLOYMENT_SANDBOX {
        crate::sandbox::browser_host_endpoint(config)
    } else {
        None
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
    let mut effective = EffectiveBrowserConfig {
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
        // A pooled browser is only useful if it can be handed to a session, so
        // the pool never outgrows the session table.
        prewarm_sessions: config
            .browser
            .limits
            .prewarm_sessions
            .min(if legacy_runtime {
                config.tools.browser.max_sessions.max(1)
            } else {
                config.browser.limits.max_sessions.max(1)
            }),
        max_snapshot_chars: config.browser.limits.max_snapshot_chars.clamp(1, 40000),
        max_download_bytes: config.browser.limits.max_download_bytes.max(1024),
        python_path: if legacy_runtime {
            trim_option(config.tools.browser.python_path.clone())
        } else {
            trim_option(config.browser.playwright.python_path.clone())
                .or_else(|| trim_option(config.tools.browser.python_path.clone()))
        },
        browsers_path: trim_option(config.browser.playwright.browsers_path.clone()),
        launch_args,
        launch_mode: if legacy_runtime {
            BROWSER_LAUNCH_MODE.to_string()
        } else {
            normalize_browser_launch_mode(&config.browser.playwright.mode)
        },
        channel: if legacy_runtime {
            None
        } else {
            trim_option(config.browser.playwright.channel.clone())
        },
        executable_path: if legacy_runtime {
            None
        } else {
            trim_option(config.browser.playwright.executable_path.clone())
        },
        attach_endpoint: if legacy_runtime {
            None
        } else {
            trim_option(config.browser.playwright.attach_endpoint.clone())
        },
        // legacy desktop 配置面没有 CDP 字段，与默认值保持一致：回退开启、端口自动。
        cdp_fallback: if legacy_runtime {
            true
        } else {
            config.browser.playwright.cdp_fallback
        },
        cdp_port: if legacy_runtime {
            0
        } else {
            config.browser.playwright.cdp_port
        },
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
        sandbox_endpoint,
    };
    apply_launch_executable_fallback(&mut effective);
    effective
}

/// Launch 模式下，channel 与 executable_path 均未解析出值时探测本机可用浏览器
/// 兜底，避免首次调用直接撞 "Executable doesn't exist"。legacy desktop 上方把
/// channel/executable_path 强制置 None，是因为旧配置面（tools.browser.*）没有
/// 这两个字段、不应沿用新配置面的残留值；探测在组装之后运行，只填补仍为 None
/// 的情形，与该清空逻辑互补而非冲突，显式配置（非 legacy）始终优先。
fn apply_launch_executable_fallback(effective: &mut EffectiveBrowserConfig) {
    if effective.launch_mode != BROWSER_LAUNCH_MODE
        || effective.channel.is_some()
        || effective.executable_path.is_some()
    {
        return;
    }
    // 沙盒部署的浏览器不在本机，注入本机路径只会把远端启动引向错误目标。
    if effective.sandbox_endpoint.is_some() {
        return;
    }
    if let Some(path) = super::probe::probe_launch_executable(effective.browsers_path.as_deref()) {
        effective.executable_path = Some(path.to_string_lossy().into_owned());
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
    } else if trimmed.eq_ignore_ascii_case(BROWSER_DEPLOYMENT_SANDBOX) {
        BROWSER_DEPLOYMENT_SANDBOX.to_string()
    } else {
        BROWSER_DEPLOYMENT_AUTO.to_string()
    }
}

fn normalize_browser_launch_mode(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case(BROWSER_LAUNCH_MODE_ATTACH) {
        BROWSER_LAUNCH_MODE_ATTACH.to_string()
    } else if trimmed.eq_ignore_ascii_case(BROWSER_LAUNCH_MODE_CDP) {
        BROWSER_LAUNCH_MODE_CDP.to_string()
    } else {
        BROWSER_LAUNCH_MODE.to_string()
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
        assert!(!effective.runs_in_sandbox());
    }

    #[test]
    fn explicit_channel_and_executable_path_are_never_overridden() {
        let mut config = Config::default();
        config.server.mode = "api".to_string();
        config.browser.enabled = true;
        config.tools.browser.enabled = true;
        config.browser.playwright.channel = Some("msedge".to_string());
        config.browser.playwright.executable_path = Some("custom/browser.exe".to_string());
        let effective = effective_browser_config(&config);
        assert_eq!(effective.channel.as_deref(), Some("msedge"));
        assert_eq!(effective.executable_path.as_deref(), Some("custom/browser.exe"));
    }

    #[test]
    fn attach_mode_never_probes_a_launch_executable() {
        let mut config = Config::default();
        config.server.mode = "api".to_string();
        config.browser.enabled = true;
        config.tools.browser.enabled = true;
        config.browser.playwright.mode = "attach".to_string();
        config.browser.playwright.attach_endpoint = Some("http://127.0.0.1:9222".to_string());
        let effective = effective_browser_config(&config);
        assert_eq!(effective.launch_mode, "attach");
        assert!(effective.executable_path.is_none());
    }

    #[test]
    fn sandbox_deployment_targets_the_sandbox_endpoint() {
        let mut config = Config::default();
        config.server.mode = "api".to_string();
        config.browser.enabled = true;
        config.tools.browser.enabled = true;
        config.browser.deployment = "sandbox".to_string();
        let effective = effective_browser_config(&config);
        assert_eq!(effective.deployment, "sandbox");
        assert!(effective.runs_in_sandbox());
        // The endpoint always resolves to something: WUNDER_SANDBOX_ENDPOINT
        // when set, otherwise the shipped default.
        assert!(effective.sandbox_endpoint.is_some());
    }
}
