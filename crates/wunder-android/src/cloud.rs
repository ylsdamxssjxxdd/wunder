//! Cloud account, device directory, approvals and the cloud chat projection
//! used when the phone switches from 本机 to 云端.
//!
//! Everything here runs on the engine thread and stores Send-friendly values;
//! the UI tick converts `RowData` into Slint rows like the local transcript.

use crate::engine::Shared;
use crate::project::{
    RowData, KIND_BODY, KIND_ERROR, KIND_FOOTER, KIND_PROCESS, KIND_USER, STATUS_DONE,
    STATUS_FAILED, STATUS_RUNNING, STATUS_STOPPED,
};
use crate::MobileSession;
use std::sync::{Arc, Mutex};
use wunder_desktop::native::{
    NativeCloudAgent, NativeCloudThreadSnapshot, NativeInterlinkApproval,
};
use wunder_desktop::NativeDesktop;

pub const TARGET_LOCAL: i32 = 0;
pub const TARGET_CLOUD: i32 = 1;

#[derive(Clone, Default)]
pub struct CloudView {
    pub logged_in: bool,
    pub expired: bool,
    pub connection: String,
    pub server: String,
    pub username: String,
    pub balance: i64,
    pub daily_grant: i64,
    pub max_concurrent: u64,
    pub active_concurrent: u64,
    pub sync_enabled: bool,
    pub inbound_enabled: bool,
    pub full_shadow: bool,
    pub last_error: String,
    pub last_success_at: String,
    pub next_retry_at: String,
    pub device_name: String,
}

#[derive(Clone, Default)]
pub struct CloudActive {
    pub id: String,
    pub title: String,
    pub status: String,
    pub model: String,
    pub agent: String,
    pub rows: Vec<RowData>,
    pub loading: bool,
    pub error: String,
}

#[derive(Clone, Default)]
pub struct DeviceView {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub status: String,
    pub connected: bool,
    pub current: bool,
    pub selectable: bool,
}

#[derive(Clone, Default)]
pub struct ApprovalView {
    pub id: String,
    pub kind: String,
    pub risk: String,
    pub from_node: String,
    pub prompt: String,
}

fn project_cloud(status: wunder_desktop::CloudStatusView) -> CloudView {
    CloudView {
        logged_in: status.logged_in,
        expired: status.expired,
        connection: status.connection,
        server: status.server,
        username: status.username,
        balance: status.balance,
        daily_grant: status.daily_grant,
        max_concurrent: status.max_concurrent_calls,
        active_concurrent: status.concurrency_active,
        sync_enabled: status.preferences_sync_enabled,
        inbound_enabled: status.inbound_enabled,
        full_shadow: status.full_shadow,
        last_error: status.last_error,
        last_success_at: status.last_success_at,
        next_retry_at: status.next_retry_at,
        device_name: std::env::var("WUNDER_DEVICE_NAME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "wunder 安卓版".to_string()),
    }
}

fn apply_cloud(shared: &Arc<Mutex<Shared>>, view: CloudView) {
    if let Ok(mut state) = shared.lock() {
        state.cloud = view;
        state.cloud_rev = state.cloud_rev.wrapping_add(1);
        state.touch();
    }
}

pub fn refresh_account(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    match desktop.cloud_status() {
        Ok(status) => apply_cloud(shared, project_cloud(status)),
        Err(error) => set_error(shared, format!("无法读取账号状态：{error}")),
    }
}

pub fn refresh_remote_account(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    match desktop.cloud_refresh_account() {
        Ok(status) => {
            apply_cloud(shared, project_cloud(status));
            refresh_devices(desktop, shared);
        }
        Err(error) => set_error(shared, format!("账号刷新失败：{error}")),
    }
}

pub fn login(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    server: &str,
    username: &str,
    password: &str,
) {
    match desktop.cloud_login(server, username, password) {
        Ok(status) => {
            apply_cloud(shared, project_cloud(status));
            refresh_threads(desktop, shared);
            refresh_devices(desktop, shared);
            notify(shared, "已登录云端");
        }
        Err(error) => set_error(shared, format!("登录失败：{error}")),
    }
}

pub fn logout(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    match desktop.cloud_logout() {
        Ok(status) => {
            apply_cloud(shared, project_cloud(status));
            if let Ok(mut state) = shared.lock() {
                state.target = TARGET_LOCAL;
                state.target_rev = state.target_rev.wrapping_add(1);
                state.cloud_threads.clear();
                state.cloud_threads_rev = state.cloud_threads_rev.wrapping_add(1);
                state.cloud_active = CloudActive::default();
                state.cloud_active_rev = state.cloud_active_rev.wrapping_add(1);
                state.devices.clear();
                state.devices_rev = state.devices_rev.wrapping_add(1);
                state.approval = None;
                state.approval_rev = state.approval_rev.wrapping_add(1);
                state.touch();
            }
            notify(shared, "已退出云端");
        }
        Err(error) => set_error(shared, format!("退出失败：{error}")),
    }
}

pub fn set_sync(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, enabled: bool) {
    match desktop.cloud_set_sync_preferences(enabled) {
        Ok(status) => apply_cloud(shared, project_cloud(status)),
        Err(error) => set_error(shared, format!("同步设置失败：{error}")),
    }
}

pub fn set_sharing(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    enabled: bool,
    full_shadow: bool,
) {
    match desktop.cloud_set_local_sharing(enabled, full_shadow) {
        Ok(status) => {
            apply_cloud(shared, project_cloud(status));
            refresh_devices(desktop, shared);
        }
        Err(error) => set_error(shared, format!("远程访问设置失败：{error}")),
    }
}

pub fn switch_target(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, target: i32) {
    let target = target.clamp(TARGET_LOCAL, TARGET_CLOUD);
    let logged_in = shared
        .lock()
        .map(|state| state.cloud.logged_in)
        .unwrap_or(false);
    if target == TARGET_CLOUD && !logged_in {
        notify(shared, "请先在设置中登录云端");
        return;
    }
    if let Ok(mut state) = shared.lock() {
        state.target = target;
        state.target_rev = state.target_rev.wrapping_add(1);
        state.touch();
    }
    if target == TARGET_CLOUD {
        refresh_threads(desktop, shared);
    }
    refresh_devices(desktop, shared);
}

pub fn refresh_threads(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let logged_in = shared
        .lock()
        .map(|state| state.cloud.logged_in)
        .unwrap_or(false);
    if !logged_in {
        if let Ok(mut state) = shared.lock() {
            state.cloud_threads.clear();
            state.cloud_threads_rev = state.cloud_threads_rev.wrapping_add(1);
            state.touch();
        }
        return;
    }
    match desktop.cloud_threads(0) {
        Ok(threads) => {
            let active = shared
                .lock()
                .map(|state| state.cloud_active.id.clone())
                .unwrap_or_default();
            let sessions = threads
                .into_iter()
                .map(|thread| MobileSession {
                    id: thread.id.into(),
                    title: if thread.title.trim().is_empty() {
                        "云端会话".into()
                    } else {
                        thread.title.into()
                    },
                    preview: "云端".into(),
                    time: String::new().into(),
                    status: thread.status.into(),
                    active: false,
                })
                .map(|mut session| {
                    if session.id.as_str() == active {
                        session.active = true;
                    }
                    session
                })
                .collect::<Vec<_>>();
            if let Ok(mut state) = shared.lock() {
                state.cloud_threads = sessions;
                state.cloud_threads_rev = state.cloud_threads_rev.wrapping_add(1);
                state.touch();
            }
        }
        Err(error) => set_error(shared, format!("无法读取云端会话：{error}")),
    }
}

pub fn new_thread(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let agents: Vec<NativeCloudAgent> = desktop.cloud_agents().unwrap_or_default();
    let agent = agents
        .first()
        .map(|agent| agent.id.clone())
        .unwrap_or_default();
    match desktop.cloud_thread_create(&agent, "") {
        Ok(thread) => {
            refresh_threads(desktop, shared);
            select_thread(desktop, shared, &thread.id);
        }
        Err(error) => set_error(shared, format!("无法新建云端会话：{error}")),
    }
}

pub fn select_thread(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, id: &str) {
    if let Ok(mut state) = shared.lock() {
        state.cloud_active = CloudActive {
            id: id.to_string(),
            loading: true,
            ..CloudActive::default()
        };
        state.cloud_active_rev = state.cloud_active_rev.wrapping_add(1);
        state.touch();
    }
    match desktop.cloud_thread_snapshot(id) {
        Ok(view) => {
            let rows = rows_from_snapshot(&view);
            let title = shared
                .lock()
                .ok()
                .and_then(|state| {
                    state
                        .cloud_threads
                        .iter()
                        .find(|thread| thread.id == id)
                        .map(|thread| thread.title.to_string())
                })
                .unwrap_or_else(|| "云端会话".to_string());
            if let Ok(mut state) = shared.lock() {
                state.cloud_active = CloudActive {
                    id: id.to_string(),
                    title,
                    status: view.status,
                    model: view.model_key,
                    agent: view.agent_id,
                    rows,
                    loading: false,
                    error: String::new(),
                };
                state.cloud_active_rev = state.cloud_active_rev.wrapping_add(1);
                state.touch();
            }
        }
        Err(error) => set_active_error(shared, format!("无法打开云端会话：{error}")),
    }
}

pub fn poll(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let (id, logged_in) = shared
        .lock()
        .map(|state| (state.cloud_active.id.clone(), state.cloud.logged_in))
        .unwrap_or_default();
    if !logged_in || id.is_empty() {
        return;
    }
    match desktop.cloud_thread_snapshot(&id) {
        Ok(view) => {
            let rows = rows_from_snapshot(&view);
            if let Ok(mut state) = shared.lock() {
                if state.cloud_active.id != id {
                    return;
                }
                state.cloud_active.rows = rows;
                state.cloud_active.status = view.status;
                state.cloud_active.model = view.model_key;
                state.cloud_active.agent = view.agent_id;
                state.cloud_active.loading = false;
                state.cloud_active.error.clear();
                state.cloud_active_rev = state.cloud_active_rev.wrapping_add(1);
                state.touch();
            }
        }
        Err(error) => set_active_error(shared, format!("云端会话刷新失败：{error}")),
    }
}

pub fn fork(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, user_round: i32) {
    let id = shared
        .lock()
        .map(|state| state.cloud_active.id.clone())
        .unwrap_or_default();
    if id.is_empty() || user_round <= 0 {
        return;
    }
    match desktop.cloud_thread_fork(&id, user_round as i64) {
        Ok(thread) => {
            refresh_threads(desktop, shared);
            select_thread(desktop, shared, &thread.id);
            notify(shared, "已创建分叉线程");
        }
        Err(error) => notify(shared, format!("无法分叉云端会话：{error}")),
    }
}

pub fn send(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, text: String) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    let (id, agent, logged_in) = shared
        .lock()
        .map(|state| {
            (
                state.cloud_active.id.clone(),
                state.cloud_active.agent.clone(),
                state.cloud.logged_in,
            )
        })
        .unwrap_or_default();
    if !logged_in {
        notify(shared, "请先登录云端");
        return;
    }
    if id.is_empty() {
        notify(shared, "请先选择或新建云端会话");
        return;
    }
    match desktop.cloud_thread_send(&id, &agent, &text) {
        Ok(_) => poll(desktop, shared),
        Err(error) => notify(shared, format!("云端发送失败：{error}")),
    }
}

pub fn stop(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let id = shared
        .lock()
        .map(|state| state.cloud_active.id.clone())
        .unwrap_or_default();
    if id.is_empty() {
        return;
    }
    match desktop.cloud_thread_cancel(&id) {
        Ok(()) => poll(desktop, shared),
        Err(error) => notify(shared, format!("停止失败：{error}")),
    }
}

pub fn rename(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, id: &str, title: &str) {
    match desktop.cloud_thread_rename(id, title) {
        Ok(()) => refresh_threads(desktop, shared),
        Err(error) => notify(shared, format!("重命名失败：{error}")),
    }
}

pub fn archive(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, id: &str) {
    match desktop.cloud_thread_archive(id) {
        Ok(()) => {
            if let Ok(mut state) = shared.lock() {
                if state.cloud_active.id == id {
                    state.cloud_active = CloudActive::default();
                    state.cloud_active_rev = state.cloud_active_rev.wrapping_add(1);
                }
            }
            refresh_threads(desktop, shared);
            notify(shared, "云端会话已归档");
        }
        Err(error) => notify(shared, format!("归档失败：{error}")),
    }
}

pub fn refresh_devices(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let (logged_in, target) = shared
        .lock()
        .map(|state| (state.cloud.logged_in, state.target))
        .unwrap_or_default();
    let mut devices = Vec::new();
    let local_name = std::env::var("WUNDER_DEVICE_NAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "本机".to_string());
    devices.push(DeviceView {
        id: "local".into(),
        kind: "phone".into(),
        label: local_name.clone(),
        status: "online".into(),
        connected: true,
        current: target == TARGET_LOCAL,
        selectable: true,
    });
    if logged_in {
        let local_device_id = desktop.interlink_status().device_id.unwrap_or_default();
        match desktop.interlink_devices() {
            Ok(nodes) => {
                for node in nodes {
                    if !local_device_id.is_empty()
                        && node.node_id == format!("device:{local_device_id}")
                    {
                        continue;
                    }
                    if node.node_id == "cloud" {
                        devices.push(DeviceView {
                            id: "cloud".into(),
                            kind: "cloud".into(),
                            label: "云端".into(),
                            status: node.status,
                            connected: node.connected,
                            current: target == TARGET_CLOUD,
                            selectable: true,
                        });
                    } else if node.label == local_name && !node.connected {
                        // A previous registration of this same device can
                        // linger in the directory as an offline shadow.
                    } else {
                        devices.push(DeviceView {
                            id: node.node_id.clone(),
                            kind: node.node_type,
                            label: node.label,
                            status: node.status,
                            connected: node.connected,
                            current: false,
                            selectable: false,
                        });
                    }
                }
            }
            Err(error) => set_error(shared, format!("无法读取设备列表：{error}")),
        }
    }
    if let Ok(mut state) = shared.lock() {
        state.devices = devices;
        state.devices_rev = state.devices_rev.wrapping_add(1);
        state.touch();
    }
}

pub fn poll_approvals(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let pending: Vec<NativeInterlinkApproval> = desktop.interlink_pending_approvals();
    let next = pending.first().map(|approval| ApprovalView {
        id: approval.approval_id.clone(),
        kind: approval.kind.clone(),
        risk: approval.risk.clone(),
        from_node: approval.from_node.clone(),
        prompt: approval.prompt.clone(),
    });
    let changed = {
        let state = shared.lock().ok();
        state
            .map(|state| {
                state.approval.as_ref().map(|current| current.id.as_str())
                    != next.as_ref().map(|next| next.id.as_str())
            })
            .unwrap_or(false)
    };
    if changed {
        if let Ok(mut state) = shared.lock() {
            state.approval = next;
            state.approval_rev = state.approval_rev.wrapping_add(1);
            state.touch();
        }
    }
}

pub fn decide_approval(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    id: &str,
    approve: bool,
) {
    let _ = desktop.interlink_decide_approval(id, approve, false);
    if let Ok(mut state) = shared.lock() {
        state.approval = None;
        state.approval_rev = state.approval_rev.wrapping_add(1);
        state.touch();
    }
    notify(
        shared,
        if approve {
            "已允许远程操作"
        } else {
            "已拒绝远程操作"
        },
    );
}

fn rows_from_snapshot(snapshot: &NativeCloudThreadSnapshot) -> Vec<RowData> {
    let mut rows = Vec::new();
    for chat in &snapshot.rows {
        let root = chat.root_turn_id.as_str();
        if !chat.user_text.trim().is_empty() {
            let mut user = RowData::new(KIND_USER, format!("cloud-user-{root}"));
            user.text = chat.user_text.clone();
            rows.push(user);
        }
        let steps = chat.process_count.unwrap_or(chat.workflow_count);
        if steps > 0 || !chat.reasoning.is_empty() {
            let mut process = RowData::new(KIND_PROCESS, format!("cloud-process-{root}"));
            process.text = format!("工作过程 · {steps} 步");
            process.summary = status_label(&chat.status).to_string();
            process.status_kind = status_kind(&chat.status);
            process.foldable = false;
            rows.push(process);
        }
        if !chat.assistant_text.trim().is_empty() {
            let mut body = RowData::new(KIND_BODY, format!("cloud-answer-{root}"));
            body.text = chat.assistant_text.clone();
            body.blocks_src = chat.assistant_text.clone();
            body.status_kind = status_kind(&chat.status);
            rows.push(body);
        }
        if !chat.failure_message.trim().is_empty() {
            let mut error = RowData::new(KIND_ERROR, format!("cloud-error-{root}"));
            error.error_title = "执行失败".into();
            error.text = chat.failure_message.clone();
            error.status_kind = STATUS_FAILED;
            rows.push(error);
        }
        if chat.user_round > 0 {
            let status = status_kind(&chat.status);
            let mut footer = RowData::new(KIND_FOOTER, format!("cloud-mobile-footer-{root}"));
            footer.text = chat.assistant_text.clone();
            footer.user_round = chat.user_round;
            footer.status_kind = status;
            footer.stats = vec![("hashtag".into(), chat.user_round.to_string())];
            if status != STATUS_DONE {
                let key = match status {
                    STATUS_RUNNING => "status-running",
                    STATUS_FAILED => "status-error",
                    _ => "status-stopped",
                };
                footer
                    .stats
                    .insert(0, (key.into(), status_label(&chat.status).into()));
            }
            rows.push(footer);
        }
    }
    rows
}

fn status_kind(status: &str) -> i32 {
    match status {
        "completed" | "succeeded" => STATUS_DONE,
        "running" | "queued" | "issued" | "acked" | "thinking" => STATUS_RUNNING,
        "canceled" | "stopped" => STATUS_STOPPED,
        "failed" | "timeout" => STATUS_FAILED,
        _ => STATUS_DONE,
    }
}

fn status_label(status: &str) -> &'static str {
    match status_kind(status) {
        STATUS_RUNNING => "运行中",
        STATUS_FAILED => "失败",
        STATUS_STOPPED => "已停止",
        _ => "已完成",
    }
}

fn set_error(shared: &Arc<Mutex<Shared>>, message: String) {
    if let Ok(mut state) = shared.lock() {
        state.cloud.last_error = message.clone();
        state.cloud_rev = state.cloud_rev.wrapping_add(1);
        state.touch();
    }
    notify(shared, message);
}

fn set_active_error(shared: &Arc<Mutex<Shared>>, message: String) {
    if let Ok(mut state) = shared.lock() {
        state.cloud_active.loading = false;
        state.cloud_active.error = message.clone();
        state.cloud_active_rev = state.cloud_active_rev.wrapping_add(1);
        state.touch();
    }
    notify(shared, message);
}

fn notify(shared: &Arc<Mutex<Shared>>, message: impl Into<String>) {
    if let Ok(mut state) = shared.lock() {
        state.notify(message);
        state.touch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_snapshot_projects_a_forkable_footer_with_the_server_round() {
        let snapshot = NativeCloudThreadSnapshot {
            status: "completed".into(),
            model_key: String::new(),
            agent_id: String::new(),
            rows: vec![wunder_desktop::native::NativeCloudChatRow {
                root_turn_id: "fixture-turn".into(),
                status: "completed".into(),
                user_round: 4,
                user_text: "fixture question".into(),
                assistant_text: "fixture answer".into(),
                reasoning: String::new(),
                workflow_count: 0,
                process_count: None,
                failure_message: String::new(),
            }],
        };
        let rows = rows_from_snapshot(&snapshot);
        let footer = rows
            .iter()
            .find(|row| row.kind == KIND_FOOTER)
            .expect("cloud footer");
        assert_eq!(footer.user_round, 4);
        assert_eq!(footer.status_kind, STATUS_DONE);
        assert_eq!(footer.text, "fixture answer");
    }
}
