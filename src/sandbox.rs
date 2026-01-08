use crate::config::Config;
use crate::i18n;
use crate::user_tools::UserToolBindings;
use crate::workspace::WorkspaceManager;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;
use tracing::warn;

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| reqwest::Client::new())
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
    let endpoint = config.sandbox.endpoint.trim().trim_end_matches('/');
    if endpoint.is_empty() {
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

    let url = format!("{endpoint}/sandboxes/execute_tool");
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
    let response = http_client()
        .post(url.clone())
        .timeout(Duration::from_secs(timeout_s))
        .json(&payload)
        .send()
        .await;
    let response = match response {
        Ok(resp) => resp,
        Err(err) => {
            warn!("sandbox request failed: {err}");
            return json!({
                "ok": false,
                "data": { "endpoint": endpoint, "detail": err.to_string() },
                "error": "sandbox request failed",
                "sandbox": true,
            });
        }
    };

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let parsed = serde_json::from_str::<Value>(&body).unwrap_or_else(|_| json!({ "raw": body }));
    if !status.is_success() {
        return json!({
            "ok": false,
            "data": { "endpoint": endpoint, "status": status.as_u16(), "response": parsed },
            "error": "sandbox returned non-success status",
            "sandbox": true,
        });
    }

    let ok = parsed.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let error = parsed
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut data = parsed.get("data").cloned().unwrap_or_else(|| json!({}));
    if let Value::Object(ref mut map) = data {
        if let Some(debug_events) = parsed.get("debug_events") {
            map.insert("debug_events".to_string(), debug_events.clone());
        }
    }

    json!({
        "ok": ok,
        "data": data,
        "error": error,
        "sandbox": true,
    })
}
