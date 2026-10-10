//! Digest, policy and prompt helpers for remote commands (docs §3.1, §9.2, §9.3).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use wunder_core::interlink::{command_level, command_policy};

use super::secret::sha256_hex;

/// Keys whose values must never be persisted verbatim (docs §9.3).
const OPAQUE_KEYS: [&str; 13] = [
    "message",
    "content",
    "text",
    "body",
    "prompt",
    "args",
    "data",
    "answer",
    "question",
    "code",
    "script",
    "secret",
    "token",
];

/// Longest string kept in clear text inside a digest.
const PLAIN_STRING_MAX: usize = 48;
/// Deepest object/array level inspected when digesting arguments.
const DIGEST_DEPTH: usize = 2;

/// Digest of command arguments: structural, never the payload itself.
///
/// Shape: `{"kind":..,"level":..,"sha256":..,"fields":{..}}`. Short, non
/// sensitive values (workspace-relative paths, ids) stay readable so the ledger
/// is usable; everything else becomes a length + hash summary.
pub fn digest_args(kind: &str, args: &Value) -> String {
    let canonical = serde_json::to_string(args).unwrap_or_default();
    let mut root = Map::new();
    root.insert("kind".to_string(), Value::String(kind.to_string()));
    root.insert("level".to_string(), Value::String(command_level(kind).to_string()));
    root.insert("sha256".to_string(), Value::String(sha256_hex(canonical.as_bytes())));
    root.insert("fields".to_string(), digest_value(args, 0));
    serde_json::to_string(&Value::Object(root)).unwrap_or_else(|_| "{}".to_string())
}

fn digest_value(value: &Value, depth: usize) -> Value {
    match value {
        Value::Object(map) if depth < DIGEST_DEPTH => {
            let mut out = Map::new();
            for (key, inner) in map {
                let projected = if OPAQUE_KEYS.contains(&key.as_str()) {
                    summarize(inner)
                } else {
                    digest_value(inner, depth + 1)
                };
                out.insert(key.clone(), projected);
            }
            Value::Object(out)
        }
        Value::String(text) if text.len() <= PLAIN_STRING_MAX && !text.contains(char::is_whitespace) => {
            value.clone()
        }
        // Arrays are always summarized (bounded), never recursed element-wise.
        other => summarize(other),
    }
}

fn summarize(value: &Value) -> Value {
    let mut out = Map::new();
    match value {
        Value::String(text) => {
            out.insert("t".to_string(), Value::String("string".to_string()));
            out.insert("n".to_string(), Value::from(text.len()));
            out.insert("h".to_string(), Value::String(hash_prefix(text)));
        }
        Value::Object(map) => {
            out.insert("t".to_string(), Value::String("object".to_string()));
            out.insert("n".to_string(), Value::from(map.len()));
        }
        Value::Array(items) => {
            out.insert("t".to_string(), Value::String("array".to_string()));
            out.insert("n".to_string(), Value::from(items.len()));
        }
        Value::Number(number) => return Value::String(number.to_string()),
        Value::Bool(flag) => return Value::Bool(*flag),
        Value::Null => return Value::Null,
    }
    Value::Object(out)
}

fn hash_prefix(text: &str) -> String {
    sha256_hex(text.as_bytes())
        .chars()
        .take(16)
        .collect()
}

/// Risk level for one kind, as carried into approval tickets.
pub fn risk_of(kind: &str) -> &'static str {
    command_policy(kind).0
}

/// Admin per-device convergence (docs §3.1 `policy_overrides`, §9.2).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DevicePolicy {
    #[serde(default)]
    pub disabled_kinds: Vec<String>,
    #[serde(default)]
    pub disabled_caps: Vec<String>,
    #[serde(default)]
    pub force_approval_kinds: Vec<String>,
    /// `"minimal"` forces the privacy-reduced shadow for this node.
    #[serde(default)]
    pub shadow_mode: Option<String>,
}

impl DevicePolicy {
    /// Parse the stored JSON; unparsable content degrades to "no overrides"
    /// rather than granting anything.
    pub fn parse(raw: Option<&str>) -> Self {
        raw.and_then(|text| serde_json::from_str::<DevicePolicy>(text).ok())
            .unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn disables(&self, kind: &str) -> bool {
        self.disabled_kinds.iter().any(|item| item == kind)
    }

    pub fn disables_cap(&self, capability: &str) -> bool {
        self.disabled_caps.iter().any(|item| item == capability)
    }

    pub fn forces_approval(&self, kind: &str) -> bool {
        self.force_approval_kinds.iter().any(|item| item == kind)
    }

    pub fn forces_minimal_shadow(&self) -> bool {
        self.shadow_mode.as_deref() == Some("minimal")
    }
}

/// A short relative path or thread id, safe to show in a prompt or digest.
pub fn short_target(kind: &str, args: &Value) -> String {
    for key in ["path", "local_thread_id", "thread_id", "agent"] {
        if let Some(text) = args.get(key).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return format!("{key}={}", trimmed.chars().take(64).collect::<String>());
            }
        }
    }
    format!("kind={kind}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wunder_core::interlink::{CMD_THREAD_MESSAGE, CMD_WORKSPACE_READ};

    #[test]
    fn digest_keeps_short_paths_and_hides_message_bodies() {
        let digest = digest_args(
            CMD_THREAD_MESSAGE,
            &json!({"local_thread_id": "th_1", "message": "summarize the daily report"}),
        );
        let value: Value = serde_json::from_str(&digest).expect("digest json");
        assert_eq!(value["level"], "L1");
        assert_eq!(value["fields"]["local_thread_id"], "th_1");
        let message = &value["fields"]["message"];
        assert_eq!(message["t"], "string", "{message:?}");
        assert!(message.get("h").is_some());
        assert!(!digest.contains("summarize the daily report"));
    }

    #[test]
    fn digest_is_stable_and_bound_to_the_kind() {
        let args = json!({"path": "notes/today.md"});
        let a = digest_args(CMD_WORKSPACE_READ, &args);
        let b = digest_args(CMD_WORKSPACE_READ, &args);
        assert_eq!(a, b);
        assert_ne!(a, digest_args("workspace.stat", &args));
    }

    #[test]
    fn device_policy_round_trips_and_fails_closed() {
        let policy = DevicePolicy {
            disabled_kinds: vec![CMD_WORKSPACE_READ.to_string()],
            disabled_caps: vec!["tool.exec".to_string()],
            force_approval_kinds: vec!["node.summary".to_string()],
            shadow_mode: Some("minimal".to_string()),
        };
        let json = policy.to_json();
        assert_eq!(DevicePolicy::parse(Some(&json)), policy);
        assert!(DevicePolicy::parse(Some("not json")).disabled_kinds.is_empty());
        assert_eq!(DevicePolicy::parse(None), DevicePolicy::default());
        assert!(policy.disables(CMD_WORKSPACE_READ));
        assert!(!policy.disables("workspace.list"));
        assert!(policy.disables_cap("tool.exec"));
        assert!(!policy.disables_cap("query.basic"));
        assert!(policy.forces_minimal_shadow());
        assert!(!DevicePolicy::default().forces_minimal_shadow());
    }

    #[test]
    fn sentences_and_nested_values_are_summarized() {
        let digest = digest_args("workspace.search", &json!({"q": "two words here"}));
        let value: Value = serde_json::from_str(&digest).expect("json");
        assert_eq!(value["fields"]["q"]["t"], "string");

        let nested = digest_args("tool.exec", &json!({"argv": ["ls", "-la"], "limit": 5}));
        let value: Value = serde_json::from_str(&nested).expect("json");
        assert_eq!(value["fields"]["limit"], "5");
        assert_eq!(value["fields"]["argv"]["t"], "array");
        assert_eq!(value["fields"]["argv"]["n"], 2);
    }

    #[test]
    fn short_target_prefers_the_path_field() {
        assert_eq!(
            short_target(CMD_WORKSPACE_READ, &json!({"path": "a/b.md"})),
            "path=a/b.md"
        );
        assert_eq!(short_target("node.summary", &json!({})), "kind=node.summary");
    }
}
