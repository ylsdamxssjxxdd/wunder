use crate::config::Config;
use crate::i18n;
use crate::user_tools::UserToolBindings;
use crate::workspace::WorkspaceManager;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::env;
use std::path::Path;
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

fn looks_like_windows_drive(value: &str) -> bool {
    value.len() >= 2
        && value.as_bytes()[1] == b':'
        && value
            .chars()
            .next()
            .map(|ch| ch.is_ascii_alphabetic())
            .unwrap_or(false)
}

fn normalize_container_path(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let replaced = trimmed.replace('\\', "/");
    if looks_like_windows_drive(&replaced) {
        return None;
    }
    Some(replaced)
}

fn clean_posix_relative(value: &str) -> String {
    let mut segments = Vec::new();
    for part in value.split('/') {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            segments.pop();
            continue;
        }
        segments.push(part);
    }
    segments.join("/")
}

fn join_posix(base: &str, child: &str) -> String {
    let base = base.trim_end_matches('/');
    let child = child.trim_start_matches('/');
    if base.is_empty() || base == "/" {
        if child.is_empty() {
            "/".to_string()
        } else {
            format!("/{child}")
        }
    } else if child.is_empty() {
        base.to_string()
    } else {
        format!("{base}/{child}")
    }
}

fn strip_root_prefix<'a>(value: &'a str, root: &str) -> Option<&'a str> {
    if value == root {
        return Some("");
    }
    if value.starts_with(root) {
        let remainder = &value[root.len()..];
        if remainder.starts_with('/') || remainder.starts_with('\\') {
            return Some(remainder);
        }
    }
    None
}

fn replace_root_in_text(text: &str, from_root: &str, to_root: &str) -> String {
    if from_root.is_empty() || from_root == to_root || !text.contains(from_root) {
        return text.to_string();
    }
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(from_root) {
        let (before, after) = rest.split_at(index);
        output.push_str(before);
        let remainder = &after[from_root.len()..];
        let boundary = remainder.is_empty()
            || matches!(
                remainder.chars().next().unwrap(),
                '/' | '\\' | '"' | '\'' | ' ' | '\n' | '\r' | '\t' | ')' | ']' | '}' | ';' | ','
            );
        if boundary {
            output.push_str(to_root);
        } else {
            output.push_str(from_root);
        }
        rest = remainder;
    }
    output.push_str(rest);
    output
}

fn resolve_container_workspace_root(
    config: &Config,
    workspace: &WorkspaceManager,
    user_id: &str,
) -> String {
    let container_root = normalize_container_path(&config.sandbox.container_root)
        .unwrap_or_else(|| "/workspaces".to_string());
    let container_root = container_root.trim_end_matches('/');
    let container_root = if container_root.is_empty() {
        "/".to_string()
    } else {
        container_root.to_string()
    };

    let public_root = workspace
        .public_root(user_id)
        .to_string_lossy()
        .replace('\\', "/");
    let safe_id = public_root
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("anonymous");

    let workspace_root_raw = config.workspace.root.trim();
    let workspace_root_norm = normalize_container_path(workspace_root_raw);
    let base_root = match workspace_root_norm {
        Some(root) if root.starts_with('/') => {
            let trimmed = root.trim_end_matches('/');
            if trimmed.is_empty() {
                container_root.to_string()
            } else if container_root == "/" || trimmed.starts_with(&container_root) {
                trimmed.to_string()
            } else {
                join_posix(&container_root, "workspaces")
            }
        }
        Some(root) => {
            let rel = clean_posix_relative(&root);
            if rel.is_empty() {
                container_root.to_string()
            } else {
                join_posix(&container_root, &rel)
            }
        }
        None => join_posix(&container_root, "workspaces"),
    };

    join_posix(&base_root, safe_id)
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

    if let Err(err) = workspace.ensure_user_root(user_id) {
        return json!({
            "ok": false,
            "data": { "detail": err.to_string() },
            "error": "failed to prepare workspace",
            "sandbox": true,
        });
    }

    let public_root = workspace
        .public_root(user_id)
        .to_string_lossy()
        .replace('\\', "/");
    let container_workspace_root = resolve_container_workspace_root(config, workspace, user_id);

    let allow_paths = collect_allow_paths(config, user_tool_bindings);
    let deny_globs = config.security.deny_globs.clone();
    let allow_commands = config.security.allow_commands.clone();

    let mut mapped_args = if args.is_object() {
        args.clone()
    } else {
        json!({ "raw": args })
    };
    if matches!(tool, "执行命令" | "ptc") {
        if let Value::Object(ref mut map) = mapped_args {
            if let Some(Value::String(workdir)) = map.get("workdir").cloned() {
                let trimmed = workdir.trim();
                let path = Path::new(trimmed);
                if path.is_absolute() {
                    if let Some(rest) = strip_root_prefix(trimmed, &public_root) {
                        let mapped = format!("{container_workspace_root}{rest}");
                        map.insert("workdir".to_string(), Value::String(mapped));
                    }
                }
            }
            if let Some(Value::String(content)) = map.get("content").cloned() {
                let rewritten =
                    replace_root_in_text(&content, &public_root, &container_workspace_root);
                if rewritten != content {
                    map.insert("content".to_string(), Value::String(rewritten));
                }
            }
        }
    }

    let payload = json!({
        "user_id": user_id,
        "session_id": session_id,
        "language": i18n::get_language(),
        "tool": tool,
        "args": mapped_args,
        "workspace_root": container_workspace_root,
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
        let data = rewrite_sandbox_paths(&public_root, &container_workspace_root, tool, data);

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

fn rewrite_sandbox_paths(
    public_root: &str,
    container_root: &str,
    tool: &str,
    mut data: Value,
) -> Value {
    if !matches!(tool, "ptc" | "执行命令") {
        return data;
    }
    replace_paths_in_value(&mut data, container_root, public_root);
    data
}

fn replace_paths_in_value(value: &mut Value, from_root: &str, to_root: &str) {
    match value {
        Value::String(text) => {
            let replaced = replace_root_in_text(text, from_root, to_root);
            if replaced != *text {
                *text = replaced;
            }
        }
        Value::Array(items) => {
            for item in items {
                replace_paths_in_value(item, from_root, to_root);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                replace_paths_in_value(item, from_root, to_root);
            }
        }
        _ => {}
    }
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
