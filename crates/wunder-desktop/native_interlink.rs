//! Interlink façade for the native window: tunnel status, the remote approval
//! prompt queue, cloud-workspace browsing and pulling one cloud file into the
//! local workspace (互通方案 I9 / §13.4 验收 14). Everything maps onto the
//! engine's own pieces — the shared `interlink::client` for the tunnel side and
//! the `CloudService` user plane for cloud targets — so this layer only
//! projects typed views and does the one local write.

use super::NativeDesktop;
use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Serialize;
use serde_json::Value;
use std::{
    path::{Component, Path, PathBuf},
    time::Duration,
};
use wunder_server::interlink::client::{self as interlink_client, Decision, TunnelState};

/// Poll cadence while waiting for one cloud command to finish.
const CLOUD_POLL_INTERVAL: Duration = Duration::from_millis(300);
/// Bounded wait for one cloud command (the cloud executor is in-process fast).
const CLOUD_POLL_TIMEOUT: Duration = Duration::from_secs(15);
/// Longer bounded wait for one file pull: the read itself transfers bytes.
const CLOUD_PULL_TIMEOUT: Duration = Duration::from_secs(30);
/// Statuses of a command that can still move.
const OPEN_STATUSES: [&str; 4] = ["issued", "queued", "acked", "running"];
/// Pull ceiling of a cloud target: with no tunnel data plane the executor pins
/// one read to the inline cap (`interlink::client::stream::INLINE_MAX_BYTES`).
const CLOUD_INLINE_LIMIT: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NativeInterlinkStatus {
    pub state: String,
    pub device_id: Option<String>,
    pub channel_id: Option<String>,
    pub pending_approvals: usize,
    pub inflight_commands: usize,
    pub watched_threads: usize,
    /// Frames the tunnel had to drop because a bounded queue was full.
    pub dropped_frames: u64,
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

/// Outcome of one cloud → local pull: where the bytes landed and how many.
/// `local_path` is always relative to the local workspace root, so the panel
/// never shows a host path.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NativeInterlinkPull {
    pub local_path: String,
    pub bytes: u64,
}

/// Usage line of the cloud workspace root. `quota_bytes` stays `None` because
/// the interlink executor's stat carries used-only numbers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NativeInterlinkStats {
    pub files: u64,
    pub dirs: u64,
    pub used_bytes: u64,
    pub truncated: bool,
}

/// Outcome of one cloud write operation (L2: mkdir/move/delete/write). The
/// cloud hard-approves this level, so a run can park on an approval ticket
/// that the desktop decides itself.
#[derive(Debug, Clone, PartialEq)]
pub enum CloudOpOutcome {
    /// Terminal success; the payload is the affected cloud path.
    Done(String),
    /// The cloud parked the command: decide it with `interlink_cloud_confirm`.
    NeedsApproval { command_id: String },
    /// Terminal failure; the payload is the readable reason.
    Failed(String),
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
            dropped_frames: status.dropped_frames,
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
    pub fn interlink_decide_approval(
        &self,
        approval_id: &str,
        approve: bool,
        remember: bool,
    ) -> bool {
        let decision = if approve {
            Decision::Approve
        } else {
            Decision::Deny
        };
        interlink_client::decide_approval(approval_id, decision, remember)
    }

    /// Device fleet as the account sees it, cloud node included.
    pub fn interlink_devices(&self) -> Result<Vec<NativeInterlinkNode>> {
        let data = self
            .runtime
            .block_on(self.state().cloud.interlink_nodes())?;
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
    /// `offset` pages one directory (the sidebar file tree loads more in
    /// place instead of refetching from zero).
    pub fn interlink_cloud_listing(
        &self,
        path: &str,
        offset: u64,
    ) -> Result<NativeInterlinkListing> {
        let path = path.trim();
        let path = if path.is_empty() { "." } else { path };
        let cloud = self.state().cloud.clone();
        let issue = self.runtime.block_on(cloud.interlink_issue_command(
            "cloud",
            "workspace.list",
            serde_json::json!({ "path": path, "offset": offset, "limit": 200 }),
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
        match self.wait_cloud_command(&cloud, &command_id, deadline) {
            Ok(record) => Ok(listing_from_record(&record, path, &command_id)),
            Err(error) => Ok(NativeInterlinkListing {
                status: "failed".to_string(),
                path: path.to_string(),
                entries: Vec::new(),
                total: 0,
                error: Some(error.to_string()),
                command_id: Some(command_id),
            }),
        }
    }

    /// Pull one cloud workspace file into the local workspace: a single
    /// `workspace.read` on the cloud target, the inline base64 result and one
    /// bounded write under the engine's workspace root. Runs on the caller's
    /// background thread; blocks up to [`CLOUD_PULL_TIMEOUT`].
    pub fn interlink_cloud_pull(
        &self,
        cloud_path: &str,
        local_path: &str,
    ) -> Result<NativeInterlinkPull> {
        let cloud_path = cloud_path.trim();
        if cloud_path.is_empty() {
            bail!("未选择云端文件");
        }
        let local_rel = match local_path.trim() {
            "" => file_tail(cloud_path),
            given => given.to_string(),
        };
        self.pull_cloud(cloud_path, &local_rel, false)
    }

    /// Open one cloud file with the OS default application: pull it into the
    /// local workspace first, then shell out. The local copy is a pull cache,
    /// so an existing file is refreshed instead of rejected — opening the same
    /// cloud file twice must not read as an error.
    pub fn interlink_cloud_open(&self, cloud_path: &str) -> Result<NativeInterlinkPull> {
        let cloud_path = cloud_path.trim();
        if cloud_path.is_empty() {
            bail!("未选择云端文件");
        }
        let local_rel = validate_local_rel(&file_tail(cloud_path))?;
        let pulled = self.pull_cloud(cloud_path, &local_rel, true)?;
        let state = self.state();
        let root = state.workspace.ensure_user_root(self.user_id())?;
        let root = root.canonicalize().unwrap_or(root);
        self.open_workspace_resource(&root.to_string_lossy(), &pulled.local_path)?;
        Ok(pulled)
    }

    /// Shared pull body: resolve the fenced local target, read the cloud file
    /// and write the bytes. `overwrite` distinguishes an explicit pull (never
    /// clobbers) from the open flow (the local file is a cache of the cloud
    /// one).
    fn pull_cloud(
        &self,
        cloud_path: &str,
        local_rel: &str,
        overwrite: bool,
    ) -> Result<NativeInterlinkPull> {
        let local_rel = validate_local_rel(local_rel)?;
        let state = self.state();
        let root = state.workspace.ensure_user_root(self.user_id())?;
        let target =
            resolve_local_target(state.workspace.as_ref(), self.user_id(), &root, &local_rel)?;
        if target.exists() && !overwrite {
            bail!("本地工作区已有同名文件：{local_rel}");
        }

        let cloud = state.cloud.clone();
        let issue = self.runtime.block_on(cloud.interlink_issue_command(
            "cloud",
            "workspace.read",
            serde_json::json!({ "path": cloud_path }),
        ))?;
        let command_id = clean(issue.get("command_id"));
        if command_id.is_empty() {
            bail!("云端未返回命令 id");
        }
        if clean(issue.get("approval_state")) == "pending" {
            bail!("云端要求先批准该次读取（命令 {command_id}），批准后重试");
        }
        let deadline = std::time::Instant::now() + CLOUD_PULL_TIMEOUT;
        let record = self.wait_cloud_command(&cloud, &command_id, deadline)?;
        let encoded = read_payload(&record).map_err(|failure| anyhow!(pull_reason(&failure)))?;
        if encoded.len() as u64 > (CLOUD_INLINE_LIMIT + CLOUD_INLINE_LIMIT / 3) as u64 {
            bail!(pull_reason(&PullFailure {
                code: "PAYLOAD_TOO_LARGE".to_string(),
                summary: format!("云端回传 {} 字节", encoded.len()),
            }));
        }
        let bytes = decode_inline(&encoded)?;
        if bytes.len() as u64 > CLOUD_INLINE_LIMIT {
            bail!(pull_reason(&PullFailure {
                code: "PAYLOAD_TOO_LARGE".to_string(),
                summary: format!("实际 {} 字节", bytes.len()),
            }));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, &bytes)?;
        Ok(NativeInterlinkPull {
            local_path: local_rel.to_string(),
            bytes: bytes.len() as u64,
        })
    }

    /// Usage snapshot of the cloud workspace root: one `workspace.stat` on the
    /// cloud target, projected for the sidebar's usage line. The interlink
    /// executor does not carry a quota figure, so the panel renders used-only
    /// stats.
    pub fn interlink_cloud_stats(&self) -> Result<NativeInterlinkStats> {
        let cloud = self.state().cloud.clone();
        let issue = self.runtime.block_on(cloud.interlink_issue_command(
            "cloud",
            "workspace.stat",
            serde_json::json!({ "path": "", "recent_limit": 0 }),
        ))?;
        let command_id = clean(issue.get("command_id"));
        if command_id.is_empty() {
            return Err(anyhow!("cloud command issue returned no id"));
        }
        if clean(issue.get("approval_state")) == "pending" {
            bail!("云端要求先批准该次读取");
        }
        let deadline = std::time::Instant::now() + CLOUD_POLL_TIMEOUT;
        let record = self.wait_cloud_command(&cloud, &command_id, deadline)?;
        stats_from_record(&record)
    }

    /// Decide a cloud-target approval ticket as the account owner.
    pub fn interlink_cloud_decide(&self, command_id: &str, approve: bool) -> Result<()> {
        let cloud = self.state().cloud.clone();
        self.runtime
            .block_on(cloud.interlink_decide(command_id.trim(), approve))?;
        Ok(())
    }

    /// Create one directory in the cloud workspace (`workspace.mkdir`, L2).
    pub fn interlink_cloud_mkdir(&self, path: &str) -> Result<CloudOpOutcome> {
        self.run_cloud_op(
            "workspace.mkdir",
            serde_json::json!({ "path": cloud_path(path) }),
        )
    }

    /// Rename (move within the same parent) one cloud entry
    /// (`workspace.move`, L2). `name` is the bare new name.
    pub fn interlink_cloud_rename(&self, source: &str, name: &str) -> Result<CloudOpOutcome> {
        let source = cloud_path(source);
        let name = name.trim().trim_matches('/').to_string();
        anyhow::ensure!(!name.is_empty(), "名称不能为空");
        anyhow::ensure!(
            !name.contains('/') && !name.contains('\\'),
            "名称不能包含路径分隔符"
        );
        anyhow::ensure!(!name.contains(".."), "名称不能包含路径穿越");
        let destination = match source.rsplit_once('/') {
            Some((parent, _)) => format!("{parent}/{name}"),
            None => name,
        };
        self.run_cloud_op(
            "workspace.move",
            serde_json::json!({ "source": source, "destination": destination }),
        )
    }

    /// Delete one cloud entry (file or directory, recursive)
    /// (`workspace.delete`, L2).
    pub fn interlink_cloud_delete(&self, path: &str) -> Result<CloudOpOutcome> {
        self.run_cloud_op(
            "workspace.delete",
            serde_json::json!({ "path": cloud_path(path) }),
        )
    }

    /// Write one text file into the cloud workspace (`workspace.write`, L2):
    /// uploads from the sidebar and new-file creation share this path. The
    /// interlink write carries text content only, bounded by the inline cap.
    pub fn interlink_cloud_write(&self, path: &str, content: &str) -> Result<CloudOpOutcome> {
        anyhow::ensure!(
            content.len() as u64 <= CLOUD_INLINE_LIMIT,
            "云端单次写入上限 {} KiB，该内容超出上限",
            CLOUD_INLINE_LIMIT / 1024
        );
        self.run_cloud_op(
            "workspace.write",
            serde_json::json!({ "path": cloud_path(path), "content": content }),
        )
    }

    /// Approve one parked cloud command and wait for its terminal state.
    pub fn interlink_cloud_confirm(&self, command_id: &str) -> Result<CloudOpOutcome> {
        let command_id = command_id.trim();
        anyhow::ensure!(!command_id.is_empty(), "缺少待确认的云端命令");
        let cloud = self.state().cloud.clone();
        self.runtime
            .block_on(cloud.interlink_decide(command_id, true))?;
        let deadline = std::time::Instant::now() + CLOUD_POLL_TIMEOUT;
        Ok(cloud_op_outcome(
            &self.wait_cloud_command(&cloud, command_id, deadline)?,
        ))
    }

    /// Stage one cloud file for a drag-out: pull it into the local workspace
    /// (refreshing the cached copy) and return the absolute local path the
    /// OS drag carries. Blocks up to [`CLOUD_PULL_TIMEOUT`].
    pub fn interlink_cloud_stage(&self, cloud_path: &str) -> Result<std::path::PathBuf> {
        let cloud_path = cloud_path.trim();
        if cloud_path.is_empty() {
            bail!("未选择云端文件");
        }
        let local_rel = validate_local_rel(&file_tail(cloud_path))?;
        let pulled = self.pull_cloud(cloud_path, &local_rel, true)?;
        let state = self.state();
        let root = state.workspace.ensure_user_root(self.user_id())?;
        let root = root.canonicalize().unwrap_or(root);
        Ok(root.join(&pulled.local_path))
    }

    /// Archive one cloud directory (or the whole root when `cloud_path` is
    /// empty) into a local zip through the cloud REST plane, then hand it to
    /// the OS. The interlink tunnel cannot carry a zip blob, so this path
    /// mirrors what the web's 打包下载 does instead of issuing a command.
    pub fn interlink_cloud_archive(
        &self,
        cloud_path: &str,
        name: &str,
    ) -> Result<NativeInterlinkPull> {
        let local_rel = validate_local_rel(&archive_file_name(name))?;
        let state = self.state();
        let root = state.workspace.ensure_user_root(self.user_id())?;
        let target =
            resolve_local_target(state.workspace.as_ref(), self.user_id(), &root, &local_rel)?;
        let cloud = state.cloud.clone();
        let bytes = self
            .runtime
            .block_on(cloud.cloud_workspace_archive(cloud_path))?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, &bytes)?;
        let root = root.canonicalize().unwrap_or(root);
        let _ = self.open_workspace_resource(&root.to_string_lossy(), &local_rel);
        Ok(NativeInterlinkPull {
            local_path: local_rel,
            bytes: bytes.len() as u64,
        })
    }

    /// Clear every entry under the cloud workspace root, mirroring the web's
    /// 清空 action (`POST /wunder/workspace/clear`).
    pub fn interlink_cloud_clear(&self) -> Result<()> {
        let cloud = self.state().cloud.clone();
        self.runtime.block_on(cloud.cloud_workspace_clear())?;
        Ok(())
    }

    /// Issue one cloud write command and fold the ledger answer into the
    /// shared outcome: parked approvals surface instead of being swallowed,
    /// because the desktop is the approver for its own requests.
    fn run_cloud_op(&self, kind: &str, args: Value) -> Result<CloudOpOutcome> {
        let cloud = self.state().cloud.clone();
        let issue = self
            .runtime
            .block_on(cloud.interlink_issue_command("cloud", kind, args))?;
        let command_id = clean(issue.get("command_id"));
        if command_id.is_empty() {
            return Err(anyhow!("cloud command issue returned no id"));
        }
        if clean(issue.get("approval_state")) == "pending" {
            return Ok(CloudOpOutcome::NeedsApproval { command_id });
        }
        let deadline = std::time::Instant::now() + CLOUD_POLL_TIMEOUT;
        Ok(cloud_op_outcome(&self.wait_cloud_command(
            &cloud,
            &command_id,
            deadline,
        )?))
    }

    /// Poll one cloud command to its terminal state, bounded by `deadline`.
    fn wait_cloud_command(
        &self,
        cloud: &wunder_server::cloud::CloudService,
        command_id: &str,
        deadline: std::time::Instant,
    ) -> Result<Value> {
        loop {
            let record = self.runtime.block_on(cloud.interlink_command(command_id))?;
            let status = clean(record.get("status"));
            if !OPEN_STATUSES.contains(&status.as_str()) {
                return Ok(record);
            }
            if std::time::Instant::now() >= deadline {
                return Err(anyhow!("云端命令超时（{status}），请稍后重试"));
            }
            std::thread::sleep(CLOUD_POLL_INTERVAL);
        }
    }
}

/// Ledger record → the inline base64 payload of a succeeded `workspace.read`.
fn read_payload(record: &Value) -> Result<String, PullFailure> {
    if clean(record.get("status")) != "succeeded" {
        return Err(PullFailure {
            code: clean(record.get("error_code")),
            summary: clean(record.get("error_summary")),
        });
    }
    let encoded = payload_with(record, "inline")
        .get("inline")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if encoded.is_empty() {
        return Err(PullFailure {
            code: "RESULT_EXPIRED".to_string(),
            summary: "云端未回传文件内容".to_string(),
        });
    }
    Ok(encoded)
}

/// The cloud executor's payload can sit at `data.result` or, because the hub
/// wraps it once more, at `data.result.result`; both shapes are accepted.
fn payload_with<'a>(record: &'a Value, key: &str) -> &'a Value {
    const NO_PAYLOAD: Value = Value::Null;
    let outer = record.get("result");
    [
        outer,
        outer.and_then(|value| value.get("result")),
        Some(record),
    ]
    .into_iter()
    .flatten()
    .find(|candidate| candidate.get(key).is_some())
    .unwrap_or(&NO_PAYLOAD)
}

/// Decide the local save target through the engine's workspace resolution and
/// re-check the boundary: `resolve_path` passes absolute input through, so a
/// second containment test is the thing that keeps the write inside the root.
fn resolve_local_target(
    workspace: &wunder_server::workspace::WorkspaceManager,
    user_id: &str,
    root: &Path,
    rel: &str,
) -> Result<PathBuf> {
    let target = workspace.resolve_path(user_id, rel)?;
    if target.is_dir() {
        bail!("本地目标已是文件夹：{rel}");
    }
    if !wunder_server::path_utils::is_within_root(root, &target) {
        bail!("保存路径超出本地工作区：{rel}");
    }
    Ok(target)
}

/// Keep the save target inside the workspace: no absolute or drive-prefixed
/// path, no parent traversal, no reserved characters, at most 8 segments.
fn validate_local_rel(raw: &str) -> Result<String> {
    let raw = raw.trim().replace('\\', "/");
    let raw = raw.trim_matches('/');
    if raw.is_empty() {
        bail!("本地保存路径为空");
    }
    let path = Path::new(raw);
    let mut parts = Vec::new();
    for part in path.components() {
        match part {
            Component::Normal(segment) => {
                let text = segment.to_string_lossy().to_string();
                if text.contains("..") || text.chars().any(|ch| "<>:\"|?*\u{0}".contains(ch)) {
                    bail!("本地保存路径含非法字符：{raw}");
                }
                parts.push(text);
            }
            _ => bail!("本地保存路径必须在工作区内：{raw}"),
        }
    }
    if parts.len() > 8 {
        bail!("本地保存路径层级过深：{raw}");
    }
    Ok(parts.join("/"))
}

/// Default local target for a cloud path: the file name only, so the pull
/// never mirrors the remote tree outside the workspace root.
fn file_tail(cloud_path: &str) -> String {
    cloud_path
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Local zip name for one archive request: the entry's own name, or
/// `workspace` for the root, always with a single `.zip` suffix.
fn archive_file_name(name: &str) -> String {
    let base = name.trim().replace('\\', "/");
    let base = base.trim_matches('/').trim();
    let base = if base.is_empty() { "workspace" } else { base };
    if base.ends_with(".zip") {
        base.to_string()
    } else {
        format!("{base}.zip")
    }
}

/// Normalize one cloud workspace path: trimmed, separators unified, leading
/// and trailing slashes dropped, the root collapses to `.` (the executor's
/// root marker).
fn cloud_path(raw: &str) -> String {
    let trimmed = raw.trim().replace('\\', "/");
    let trimmed = trimmed.trim_matches('/');
    if trimmed.is_empty() {
        ".".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Ledger record → one cloud write outcome. Parked approvals never reach
/// here: `run_cloud_op` surfaces them as [`CloudOpOutcome::NeedsApproval`]
/// before any wait, and `interlink_cloud_confirm` waits only after deciding.
fn cloud_op_outcome(record: &Value) -> CloudOpOutcome {
    if clean(record.get("status")) == "succeeded" {
        let result = payload_with(record, "path");
        CloudOpOutcome::Done(clean(result.get("path")))
    } else {
        let code = clean(record.get("error_code"));
        let summary = clean(record.get("error_summary"));
        if code.is_empty() && summary.is_empty() {
            CloudOpOutcome::Failed("云端操作失败".to_string())
        } else {
            CloudOpOutcome::Failed(format!("{code} {summary}").trim().to_string())
        }
    }
}

/// Structured failure of one pull, mapped from the ledger error pair.
#[derive(Debug, Clone, PartialEq)]
struct PullFailure {
    code: String,
    summary: String,
}

/// Human sentence for one pull failure; the panel shows it verbatim.
fn pull_reason(failure: &PullFailure) -> String {
    let limit = CLOUD_INLINE_LIMIT / 1024;
    match failure.code.as_str() {
        "PAYLOAD_TOO_LARGE" => format!(
            "云端单次拉取上限 {limit} KiB，该文件超出上限（{}）。大文件请改用互通设备通道",
            failure.summary
        ),
        "PATH_NOT_FOUND" => "云端没有这个文件".to_string(),
        "PATH_REJECTED" => "云端拒绝该路径（超出云端工作区范围）".to_string(),
        "PATH_REQUIRED" => "云端路径无效".to_string(),
        "NOT_A_FILE" => "云端路径不是文件".to_string(),
        "READ_FAILED" => "云端读取失败".to_string(),
        "RESULT_EXPIRED" => "云端结果已过期，请重新拉取".to_string(),
        "" if !failure.summary.is_empty() => failure.summary.clone(),
        "" => "云端未返回失败原因".to_string(),
        other => format!("云端返回失败 {other} {}", failure.summary),
    }
}

/// Inline base64 → bytes; one decode, one buffer.
fn decode_inline(encoded: &str) -> Result<Vec<u8>> {
    BASE64
        .decode(encoded.as_bytes())
        .map_err(|error| anyhow!("云端回传内容无法解码：{error}"))
}

/// Ledger record → the stat triple of a succeeded `workspace.stat`.
fn stats_from_record(record: &Value) -> Result<NativeInterlinkStats> {
    if clean(record.get("status")) != "succeeded" {
        bail!(
            "{} {}",
            clean(record.get("error_code")),
            clean(record.get("error_summary"))
        );
    }
    let result = payload_with(record, "files");
    Ok(NativeInterlinkStats {
        files: result.get("files").and_then(Value::as_u64).unwrap_or(0),
        dirs: result.get("dirs").and_then(Value::as_u64).unwrap_or(0),
        used_bytes: result
            .get("used_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        truncated: result
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
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
    let result = payload_with(record, "entries");
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

    /// The hub wraps the executor payload once more, so the rows live at
    /// `data.result.result`; the listing must read them there too.
    #[test]
    fn listing_reads_the_nested_hub_shape() {
        let record = json!({
            "status": "succeeded",
            "result": {
                "status": "succeeded",
                "result": {"path": "notes", "total": 1, "entries": [
                    {"name": "a.md", "path": "notes/a.md", "type": "file", "size": 8}
                ]}
            }
        });
        let listing = listing_from_record(&record, "notes", "cmd-4");
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.total, 1);
    }

    #[test]
    fn inline_payload_is_read_from_either_nesting() {
        let nested = json!({
            "status": "succeeded",
            "result": { "result": { "inline": BASE64.encode(b"ship").to_string() } }
        });
        let flat = json!({
            "status": "succeeded",
            "result": { "inline": BASE64.encode(b"ship").to_string() }
        });
        for record in [nested, flat] {
            let encoded = read_payload(&record).expect("inline payload");
            assert_eq!(decode_inline(&encoded).expect("decode"), b"ship");
        }
    }

    #[test]
    fn structured_read_failures_get_a_readable_reason() {
        let record = json!({
            "status": "failed",
            "error_code": "PAYLOAD_TOO_LARGE",
            "error_summary": "file is 4096 bytes, the local ceiling is 1048576"
        });
        let reason = pull_reason(&read_payload(&record).unwrap_err());
        assert!(reason.contains("上限"), "{reason}");
        assert!(reason.contains("4096"), "{reason}");
        assert!(pull_reason(&PullFailure {
            code: "PATH_NOT_FOUND".to_string(),
            summary: String::new(),
        })
        .contains("云端没有这个文件"));
        assert!(pull_reason(&PullFailure {
            code: "RESULT_EXPIRED".to_string(),
            summary: String::new(),
        })
        .contains("过期"));
        assert!(pull_reason(&PullFailure {
            code: String::new(),
            summary: String::new(),
        })
        .contains("未返回失败原因"));
        assert!(pull_reason(&PullFailure {
            code: "EXEC_UNAVAILABLE".to_string(),
            summary: "cloud node is offline".to_string(),
        })
        .contains("EXEC_UNAVAILABLE"));
    }

    #[test]
    fn missing_inline_on_a_succeeded_record_is_expired_result() {
        let failure = read_payload(&json!({"status": "succeeded", "result": {}})).unwrap_err();
        assert_eq!(failure.code, "RESULT_EXPIRED");
    }

    #[test]
    fn local_target_rejects_escape_absolute_and_reserved_shapes() {
        for raw in [
            "",
            "/",
            "..",
            "../outside",
            "notes/../../outside",
            "C:/somewhere/else",
            "notes/<bad>.md",
            "notes/a:b.md",
            "a/b/c/d/e/f/g/h/i",
        ] {
            assert!(validate_local_rel(raw).is_err(), "{raw} must be rejected");
        }
        assert_eq!(validate_local_rel(" notes/a.md ").unwrap(), "notes/a.md");
        assert_eq!(validate_local_rel("a.md").unwrap(), "a.md");
        assert_eq!(validate_local_rel("./a.md").unwrap(), "a.md");
        assert_eq!(validate_local_rel("/a.md").unwrap(), "a.md");
        assert_eq!(validate_local_rel("a\\b.md").unwrap(), "a/b.md");
    }

    #[test]
    fn the_engine_boundary_check_is_what_rejects_a_escaped_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical root");
        let inside = root.join("notes").join("a.md");
        assert!(wunder_server::path_utils::is_within_root(&root, &inside));
        let escaped = root.join("..").join("outside.md");
        assert!(!wunder_server::path_utils::is_within_root(&root, &escaped));
        assert!(!wunder_server::path_utils::is_within_root(
            &root,
            &root.parent().expect("parent").join("outside.md")
        ));
    }

    #[test]
    fn default_local_name_is_the_cloud_file_tail() {
        assert_eq!(file_tail("notes/a.md"), "a.md");
        assert_eq!(file_tail("/notes/a.md"), "a.md");
        assert_eq!(file_tail(r"notes\sub\a.md"), "a.md");
        assert_eq!(file_tail("a.md"), "a.md");
        assert_eq!(file_tail("notes/"), "");
    }

    #[test]
    fn oversized_inline_fails_before_writing() {
        let bytes = vec![b'x'; (CLOUD_INLINE_LIMIT + 1) as usize];
        let encoded = BASE64.encode(&bytes);
        assert!(encoded.len() as u64 > (CLOUD_INLINE_LIMIT + CLOUD_INLINE_LIMIT / 3) as u64);
        let reason = pull_reason(&PullFailure {
            code: "PAYLOAD_TOO_LARGE".to_string(),
            summary: format!("云端回传 {} 字节", encoded.len()),
        });
        assert!(reason.contains("上限"), "{reason}");
    }

    #[test]
    fn stat_rows_project_from_the_ledger_record() {
        let flat = json!({
            "status": "succeeded",
            "result": {"files": 12, "dirs": 3, "used_bytes": 4096, "truncated": true}
        });
        for record in [
            flat,
            json!({
                "status": "succeeded",
                "result": {"result": {"files": 12, "dirs": 3, "used_bytes": 4096}}
            }),
        ] {
            let stats = stats_from_record(&record).expect("stats");
            assert_eq!(stats.files, 12);
            assert_eq!(stats.dirs, 3);
            assert_eq!(stats.used_bytes, 4096);
        }
        let truncated = stats_from_record(&json!({
            "status": "succeeded",
            "result": {"files": 1, "dirs": 0, "used_bytes": 2, "truncated": true}
        }))
        .expect("stats");
        assert!(truncated.truncated);
    }

    #[test]
    fn failed_stat_records_carry_the_error_pair() {
        let error = stats_from_record(&json!({
            "status": "failed",
            "error_code": "WORKSPACE_ERROR",
            "error_summary": "missing root"
        }))
        .unwrap_err();
        assert!(error.to_string().contains("WORKSPACE_ERROR"));
    }

    #[test]
    fn cloud_paths_normalize_to_the_executor_root_marker() {
        assert_eq!(cloud_path(""), ".");
        assert_eq!(cloud_path("  "), ".");
        assert_eq!(cloud_path("/notes/"), "notes");
        assert_eq!(cloud_path(r"\notes\sub\"), "notes/sub");
        assert_eq!(cloud_path("notes/a.md"), "notes/a.md");
    }

    #[test]
    fn cloud_op_outcome_folds_the_ledger_answer() {
        let done = cloud_op_outcome(&json!({
            "status": "succeeded",
            "result": {"path": "/notes/plan.md"}
        }));
        assert_eq!(done, CloudOpOutcome::Done("notes/plan.md".to_string()));
        let failed = cloud_op_outcome(&json!({
            "status": "failed",
            "error_code": "PATH_NOT_FOUND",
            "error_summary": "no such entry"
        }));
        assert_eq!(
            failed,
            CloudOpOutcome::Failed("PATH_NOT_FOUND no such entry".to_string())
        );
        let silent = cloud_op_outcome(&json!({"status": "failed"}));
        assert_eq!(silent, CloudOpOutcome::Failed("云端操作失败".to_string()));
    }

    #[test]
    fn rename_targets_the_same_parent() {
        let destination = |source: &str, name: &str| match source.rsplit_once('/') {
            Some((parent, _)) => format!("{parent}/{name}"),
            None => name.to_string(),
        };
        assert_eq!(destination("notes/a.md", "b.md"), "notes/b.md");
        assert_eq!(destination("a.md", "b.md"), "b.md");
    }
}
