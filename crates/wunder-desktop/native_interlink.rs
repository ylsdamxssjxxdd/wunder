//! Interlink façade for the native window: tunnel status, the remote approval
//! prompt queue and cloud-workspace browsing (互通方案 I9). Everything maps
//! onto the engine's own pieces — the shared `interlink::client` for the
//! tunnel side and the `CloudService` user plane for cloud targets — so this
//! layer only projects typed views.

use super::NativeDesktop;
use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use wunder_server::interlink::client::{
    self as interlink_client, Decision, TunnelState,
};

/// Poll cadence while waiting for one cloud command to finish.
const CLOUD_POLL_INTERVAL: Duration = Duration::from_millis(300);
/// Bounded wait for one cloud command (the cloud executor is in-process fast).
const CLOUD_POLL_TIMEOUT: Duration = Duration::from_secs(15);
/// Statuses of a command that can still move.
const OPEN_STATUSES: [&str; 3] = ["issued", "acked", "running"];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NativeInterlinkStatus {
    pub state: String,
    pub device_id: Option<String>,
    pub channel_id: Option<String>,
    pub pending_approvals: usize,
    pub inflight_commands: usize,
    pub watched_threads: usize,
    pub last_error: Option<String>,
    pub next_retry_at: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NativeInterlinkApproval {
    pub approval_id: String,
    pub command_id: String,
    pub kind: String,
    pub level: String,
    pub risk: String,
    pub from_node: String,
    pub prompt: String,
    pub expires_at: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NativeInterlinkNode {
    pub node_id: String,
    pub node_type: String,
    pub label: String,
    pub status: String,
    pub connected: bool,
    pub shadow_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NativeInterlinkEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub updated_time: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NativeInterlinkListing {
    /// `succeeded` / `pending` (awaits an approval ticket) / `failed`.
    pub status: String,
    pub path: String,
    pub entries: Vec<NativeInterlinkEntry>,
    pub total: u64,
    pub error: Option<String>,
    /// Set when `status == "pending"`: decide it with `interlink_cloud_decide`.
    pub command_id: Option<String>,
}

fn clean(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

impl NativeDesktop {
    /// Tunnel sub-state for the sidebar/status badge.
    pub fn interlink_status(&self) -> NativeInterlinkStatus {
        let status = interlink_client::status();
        NativeInterlinkStatus {
            state: match status.state {
                TunnelState::Connecting => "connecting".to_string(),
                TunnelState::Connected => "connected".to_string(),
                TunnelState::Reconnecting => "reconnecting".to_string(),
                TunnelState::Disabled => "disabled".to_string(),
            },
            device_id: status.device_id,
            channel_id: status.channel_id,
            pending_approvals: status.pending_approvals,
            inflight_commands: status.inflight_commands,
            watched_threads: status.watched_threads,
            last_error: status.last_error,
            next_retry_at: status.next_retry_at,
        }
    }

    /// Prompts waiting for a local decision (docs §7.3); drained by the UI
    /// poller and answered through `interlink_decide_approval`.
    pub fn interlink_pending_approvals(&self) -> Vec<NativeInterlinkApproval> {
        interlink_client::pending_approvals()
            .into_iter()
            .map(|pending| NativeInterlinkApproval {
                approval_id: pending.approval_id,
                command_id: pending.command_id,
                kind: pending.kind,
                level: pending.level,
                risk: pending.risk,
                from_node: pending.from_node,
                prompt: pending.prompt,
                expires_at: pending.expires_at,
            })
            .collect()
    }

    /// Answer one local prompt. `remember` extends the L1 scope memory only.
    pub fn interlink_decide_approval(&self, approval_id: &str, approve: bool, remember: bool) -> bool {
        let decision = if approve {
            Decision::Approve
        } else {
            Decision::Deny
        };
        interlink_client::decide_approval(approval_id, decision, remember)
    }

    /// Device fleet as the account sees it, cloud node included.
    pub fn interlink_devices(&self) -> Result<Vec<NativeInterlinkNode>> {
        let data = self.runtime.block_on(self.state().cloud.interlink_nodes())?;
        let nodes = data
            .get("nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(nodes
            .iter()
            .map(|node| NativeInterlinkNode {
                node_id: clean(node.get("node_id")),
                node_type: clean(node.get("node_type")),
                label: clean(node.get("label")),
                status: clean(node.get("status")),
                connected: node
                    .get("connected")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                shadow_revision: node
                    .get("shadow_revision")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
            })
            .filter(|node| !node.node_id.is_empty())
            .collect())
    }

    /// Browse the cloud workspace of the logged-in account: issue one
    /// `workspace.list` to the cloud node and wait for its terminal state.
    pub fn interlink_cloud_listing(&self, path: &str) -> Result<NativeInterlinkListing> {
        let path = path.trim();
        let path = if path.is_empty() { "." } else { path };
        let cloud = self.state().cloud.clone();
        let issue = self
            .runtime
            .block_on(cloud.interlink_issue_command(
                "cloud",
                "workspace.list",
                serde_json::json!({ "path": path, "limit": 200 }),
            ))?;
        let command_id = clean(issue.get("command_id"));
        if command_id.is_empty() {
            return Err(anyhow!("cloud command issue returned no id"));
        }
        let approval_state = clean(issue.get("approval_state"));
        if approval_state == "pending" {
            return Ok(NativeInterlinkListing {
                status: "pending".to_string(),
                path: path.to_string(),
                entries: Vec::new(),
                total: 0,
                error: None,
                command_id: Some(command_id),
            });
        }
        let deadline = std::time::Instant::now() + CLOUD_POLL_TIMEOUT;
        loop {
            let record = self
                .runtime
                .block_on(cloud.interlink_command(&command_id))?;
            let status = clean(record.get("status"));
            if OPEN_STATUSES.contains(&status.as_str()) {
                if std::time::Instant::now() >= deadline {
                    return Ok(NativeInterlinkListing {
                        status: "failed".to_string(),
                        path: path.to_string(),
                        entries: Vec::new(),
                        total: 0,
                        error: Some("cloud command timed out".to_string()),
                        command_id: Some(command_id),
                    });
                }
                std::thread::sleep(CLOUD_POLL_INTERVAL);
                continue;
            }
            return Ok(listing_from_record(&record, path, &command_id));
        }
    }

    /// Decide a cloud-target approval ticket as the account owner.
    pub fn interlink_cloud_decide(&self, command_id: &str, approve: bool) -> Result<()> {
        let cloud = self.state().cloud.clone();
        self.runtime
            .block_on(cloud.interlink_decide(command_id.trim(), approve))?;
        Ok(())
    }
}

/// Ledger record → listing rows. Free function so tests pin the projection
/// without a started `DesktopRuntime`.
fn listing_from_record(record: &Value, path: &str, command_id: &str) -> NativeInterlinkListing {
    let status = clean(record.get("status"));
    if status != "succeeded" {
        return NativeInterlinkListing {
            status: "failed".to_string(),
            path: path.to_string(),
            entries: Vec::new(),
            total: 0,
            error: Some(format!(
                "{} {}",
                clean(record.get("error_code")),
                clean(record.get("error_summary"))
            )),
            command_id: Some(command_id.to_string()),
        };
    }
    let result = record.get("result").cloned().unwrap_or(Value::Null);
    let entries = result
        .get("entries")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let name = clean(row.get("name"));
                    if name.is_empty() {
                        return None;
                    }
                    Some(NativeInterlinkEntry {
                        name: name.clone(),
                        path: clean(row.get("path")),
                        kind: clean(row.get("type")),
                        size: row.get("size").and_then(Value::as_u64).unwrap_or(0),
                        updated_time: clean(row.get("updated_time")),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    NativeInterlinkListing {
        status: "succeeded".to_string(),
        path: result
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(path)
            .to_string(),
        total: result.get("total").and_then(Value::as_u64).unwrap_or(0),
        entries,
        error: None,
        command_id: Some(command_id.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn listing_rows_project_from_the_ledger_record() {
        let record = json!({
            "status": "succeeded",
            "result": {
                "path": "docs",
                "total": 2,
                "entries": [
                    {"name": "a.md", "path": "docs/a.md", "type": "file", "size": 12,
                     "updated_time": "2026-10-10 09:00:00"},
                    {"name": "", "type": "file"}
                ]
            }
        });
        let listing = listing_from_record(&record, "docs", "cmd-1");
        assert_eq!(listing.status, "succeeded");
        assert_eq!(listing.path, "docs");
        assert_eq!(listing.total, 2);
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].name, "a.md");
        assert_eq!(listing.entries[0].kind, "file");
        assert_eq!(listing.entries[0].size, 12);
    }

    #[test]
    fn failed_records_carry_the_error_pair() {
        let record = json!({
            "status": "failed",
            "error_code": "PATH_NOT_FOUND",
            "error_summary": "no such directory"
        });
        let listing = listing_from_record(&record, "x", "cmd-2");
        assert_eq!(listing.status, "failed");
        assert!(listing
            .error
            .as_deref()
            .unwrap_or("")
            .contains("PATH_NOT_FOUND"));
        assert!(listing.entries.is_empty());
    }

    #[test]
    fn empty_result_payload_stays_an_empty_listing() {
        let listing = listing_from_record(&json!({"status": "succeeded"}), "docs", "cmd-3");
        assert_eq!(listing.status, "succeeded");
        assert!(listing.entries.is_empty());
        assert_eq!(listing.path, "docs");
        assert_eq!(listing.total, 0);
    }
}
