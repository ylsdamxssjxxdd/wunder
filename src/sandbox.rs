use crate::config::Config;
use crate::i18n;
use crate::user_tools::UserToolBindings;
use crate::workspace::WorkspaceManager;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::env;
use std::sync::OnceLock;
use std::time::Duration;
use tracing::warn;
use url::Url;

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| reqwest::Client::new())
}

fn normalize_endpoint(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains("://") {
        let url = Url::parse(trimmed).ok()?;
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }
        return Some(trimmed.to_string());
    }
    let prefixed = format!("http://{trimmed}");
    if Url::parse(&prefixed).is_ok() {
        return Some(prefixed);
    }
    None
}

fn endpoint_host(endpoint: &str) -> Option<String> {
    Url::parse(endpoint)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "0.0.0.0" | "::1")
}

fn sandbox_endpoint_candidates(config: &Config) -> Vec<String> {
    fn push(candidates: &mut Vec<String>, seen: &mut HashSet<String>, raw: &str) {
        let Some(normalized) = normalize_endpoint(raw) else {
            return;
        };
        if !seen.insert(normalized.clone()) {
            return;
        }
        candidates.push(normalized);
    }

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(value) = env::var("WUNDER_SANDBOX_ENDPOINT") {
        push(&mut candidates, &mut seen, &value);
    }
    push(&mut candidates, &mut seen, &config.sandbox.endpoint);

    let mut has_loopback = false;
    let mut has_sandbox_host = false;
    for endpoint in &candidates {
        let Some(host) = endpoint_host(endpoint) else {
            continue;
        };
        if is_loopback_host(&host) {
            has_loopback = true;
        }
        if host.eq_ignore_ascii_case("sandbox") {
            has_sandbox_host = true;
        }
    }

    if has_loopback {
        push(&mut candidates, &mut seen, "http://sandbox:9001");
    }
    if has_sandbox_host {
        push(&mut candidates, &mut seen, "http://127.0.0.1:9001");
    }

    candidates
}

fn normalize_container_path(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let replaced = trimmed.replace('\\', "/");
    let looks_like_windows_drive = replaced.len() >= 2
        && replaced.as_bytes()[1] == b':'
        && replaced
            .chars()
            .next()
            .map(|ch| ch.is_ascii_alphabetic())
            .unwrap_or(false);
    if looks_like_windows_drive {
        return None;
    }
    Some(replaced)
}

fn collect_allow_paths(config: &Config, bindings: Option<&UserToolBindings>) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();

    let mut push_path = |raw: &str| {
        let Some(normalized) = normalize_container_path(raw) else {
            return;
        };
        if !seen.insert(normalized.clone()) {
            return;
        }
        output.push(normalized);
    };

    for raw in &config.security.allow_paths {
        push_path(raw);
    }
    for raw in &config.skills.paths {
        push_path(raw);
    }
    if let Some(bindings) = bindings {
        for source in bindings.skill_sources.values() {
            let raw = source.root.to_string_lossy().to_string();
            push_path(&raw);
        }
    }
    output
}

pub fn sandbox_enabled(config: &Config) -> bool {
    config.sandbox.mode.trim().eq_ignore_ascii_case("sandbox")
}

pub async fn execute_tool(
    config: &Config,
    workspace: &WorkspaceManager,
    user_id: &str,
    session_id: &str,
    tool: &str,
    args: &Value,
    user_tool_bindings: Option<&UserToolBindings>,
) -> Value {
    let endpoints = sandbox_endpoint_candidates(config);
    if endpoints.is_empty() {
        return json!({
            "ok": false,
            "data": {},
            "error": "sandbox endpoint is empty",
            "sandbox": true,
        });
    }

    let workspace_root = match workspace.ensure_user_root(user_id) {
        Ok(path) => path,
        Err(err) => {
            return json!({
                "ok": false,
                "data": { "detail": err.to_string() },
                "error": "failed to prepare workspace",
                "sandbox": true,
            });
        }
    };

    let allow_paths = collect_allow_paths(config, user_tool_bindings);
    let deny_globs = config.security.deny_globs.clone();
    let allow_commands = config.security.allow_commands.clone();

    let workspace_root = workspace_root.to_string_lossy().replace('\\', "/");
    let workspace_root =
        normalize_container_path(&workspace_root).unwrap_or_else(|| "./".to_string());

    let payload = json!({
        "user_id": user_id,
        "session_id": session_id,
        "language": i18n::get_language(),
        "tool": tool,
        "args": if args.is_object() { args.clone() } else { json!({ "raw": args }) },
        "workspace_root": workspace_root,
        "allow_paths": allow_paths,
        "deny_globs": deny_globs,
        "allow_commands": allow_commands,
        "container_root": config.sandbox.container_root,
        "image": config.sandbox.image,
        "network": config.sandbox.network,
        "readonly_rootfs": config.sandbox.readonly_rootfs,
        "idle_ttl_s": config.sandbox.idle_ttl_s,
        "resources": {
            "cpu": config.sandbox.resources.cpu,
            "memory_mb": config.sandbox.resources.memory_mb,
            "pids": config.sandbox.resources.pids,
        }
    });

    let timeout_s = config.sandbox.timeout_s.max(1);
    let mut last_error = json!({});

    for endpoint in &endpoints {
        let url = format!("{endpoint}/sandboxes/execute_tool");
        let response = http_client()
            .post(url)
            .timeout(Duration::from_secs(timeout_s))
            .json(&payload)
            .send()
            .await;

        let response = match response {
            Ok(resp) => resp,
            Err(err) => {
                warn!("sandbox request failed for {endpoint}: {err}");
                last_error = json!({ "endpoint": endpoint, "detail": err.to_string() });
                continue;
            }
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let parsed =
            serde_json::from_str::<Value>(&body).unwrap_or_else(|_| json!({ "raw": body }));
        if !status.is_success() {
            last_error = json!({
                "endpoint": endpoint,
                "status": status.as_u16(),
                "response": parsed,
            });
            continue;
        }

        let ok = parsed.get("ok").and_then(Value::as_bool).unwrap_or(false);
        let error = parsed
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let data = parsed.get("data").cloned().unwrap_or_else(|| json!({}));

        return json!({
            "ok": ok,
            "data": data,
            "error": error,
            "sandbox": true,
        });
    }

    json!({
        "ok": false,
        "data": { "tried_endpoints": endpoints, "last_error": last_error },
        "error": "sandbox request failed",
        "sandbox": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_env_var<F: FnOnce() -> R, R>(key: &str, value: Option<&str>, f: F) -> R {
        let original = env::var(key).ok();
        match value {
            Some(value) => env::set_var(key, value),
            None => env::remove_var(key),
        }
        let result = f();
        match original {
            Some(value) => env::set_var(key, value),
            None => env::remove_var(key),
        }
        result
    }

    #[test]
    fn test_normalize_endpoint() {
        assert_eq!(
            normalize_endpoint("http://sandbox:9001/").as_deref(),
            Some("http://sandbox:9001")
        );
        assert_eq!(
            normalize_endpoint("sandbox:9001").as_deref(),
            Some("http://sandbox:9001")
        );
        assert_eq!(normalize_endpoint("").as_deref(), None);
        assert_eq!(normalize_endpoint("ftp://example.com").as_deref(), None);
    }

    #[test]
    fn test_sandbox_endpoint_candidates_adds_fallback() {
        with_env_var("WUNDER_SANDBOX_ENDPOINT", None, || {
            let mut config = Config::default();
            config.sandbox.endpoint = "http://127.0.0.1:9001".to_string();

            let candidates = sandbox_endpoint_candidates(&config);
            assert!(candidates
                .iter()
                .any(|item| item == "http://127.0.0.1:9001"));
            assert!(candidates.iter().any(|item| item == "http://sandbox:9001"));
        });
    }
}
