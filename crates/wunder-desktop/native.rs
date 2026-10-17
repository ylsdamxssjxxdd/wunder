//! In-process desktop façade for native UI frontends.
//!
//! This layer deliberately uses the same AppState, ThreadRuntime and
//! Orchestrator as the server. It only translates typed UI requests to runtime
//! calls; it does not duplicate scheduling or permission rules.

use crate::{
    args::DesktopArgs,
    runtime::{load_desktop_settings, resolve_winpty_library, DesktopRuntime},
};
use anyhow::{anyhow, Result};
use bytes::Bytes;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;
use wunder_server::DesktopTerminalSnapshot;
#[path = "native_trajectory.rs"]
mod trajectory;

#[path = "native_workflow.rs"]
mod native_workflow;

#[path = "native_message_stats.rs"]
mod message_stats;
pub use message_stats::NativeMessageStats;

#[path = "native_commands.rs"]
mod commands;
pub use commands::{NativeChatCommand, NativeGoal};

#[path = "native_context.rs"]
mod context;
pub use context::NativeContextUsage;

#[path = "native_terminal.rs"]
mod terminal;
pub use terminal::{
    NativeTerminal, NativeTerminalFrame, NativeTerminalSnapshot, NativeTerminalSpec, NativeTerminalSignal, NativeTerminalWatch,
};

#[path = "native_chat_turns.rs"]
mod chat_turns;
#[path = "native_chat_process.rs"]
mod chat_process;
pub use chat_process::{NativeProcessItem, NativeProcessPage};
#[path = "native_observer.rs"]
mod observer;
#[path = "native_queue.rs"]
mod queue;
#[path = "native_stream.rs"]
mod stream;
pub use crate::runtime::RuntimeToolStatus;
pub use chat_turns::{NativeChatRound, NativeChatTurn};
pub use observer::{NativeThreadUpdate, NativeThreadWatch};
pub use queue::{NativeQueueTurn, NATIVE_QUEUE_LIMIT};
pub use stream::{NativeChatEvent, NativeStream};
#[path = "native_catalog.rs"]
mod catalog;
#[path = "native_cloud.rs"]
mod cloud;
#[path = "native_cloud_threads.rs"]
mod cloud_threads;
pub use cloud_threads::{NativeCloudAgent, NativeCloudThread};

#[path = "native_interlink.rs"]
mod interlink;
pub use interlink::{
    CloudOpOutcome, NativeInterlinkApproval, NativeInterlinkEntry, NativeInterlinkListing,
    NativeInterlinkNode, NativeInterlinkPull, NativeInterlinkStats, NativeInterlinkStatus,
};
#[path = "native_companion_overlay.rs"]
mod companion_overlay;
#[path = "native_companions.rs"]
mod companions;
pub use companion_overlay::{
    clamp_companion_scale, CompanionAgentOverride, CompanionOverlayState, CompanionPlacement,
    COMPANION_SCALE_MAX, COMPANION_SCALE_MIN,
};
#[path = "native_navigation.rs"]
mod navigation;
pub use companions::CompanionSummaryRecord;
pub use navigation::NativeNavigationOrder;
#[path = "native_tool_manager.rs"]
mod tool_manager;
pub use tool_manager::{
    NativeKnowledgeConfig, NativeKnowledgeDraft, NativeMcpConfig, NativeMcpServerDraft,
    NativeToolFile, NativeToolFiles, NativeToolManager, NativeToolResource, ToolResourceKind,
};
#[path = "native_expert.rs"]
mod expert;
pub use expert::ExpertMemory;
#[path = "native_activity.rs"]
mod activity;
#[path = "native_channels.rs"]
mod channels;
#[path = "native_cron.rs"]
mod cron;
#[path = "native_profile.rs"]
mod profile;
#[path = "native_prompts.rs"]
mod prompts;
#[path = "native_settings.rs"]
mod settings;
#[path = "native_supplement.rs"]
mod supplement;
#[path = "native_workspace.rs"]
mod workspace;
pub use catalog::{AgentRecord, AgentSettingsEdit, ToolRecord, DEFAULT_AGENT_ID};
pub use channels::{
    ChannelAccountCard, ChannelAccountListing, ChannelBindingCard, ChannelCatalogItem,
    ChannelLogEntry, NativeChannelAccountEdit, NativeChannelBindingEdit, WeixinQrLoginStart,
    WeixinQrLoginStatus,
};
pub use cloud::{validate_cloud_password, CloudStatusView};
pub use cron::{CronRecord, CronRunRecord, NativeCronJobEdit};
pub use profile::NativeProfile;
pub use prompts::{PromptPackInfo, PromptSegmentContent};
pub use settings::{
    DesktopSettings, LanPeerRecord, LanSettings, ModelEdit, ModelProbeOutcome, ModelRecord,
};
pub use supplement::SupplementImportReport;
pub use workspace::{
    NativeWorkspace, WorkspaceDeleteSummary, WorkspaceEdit, WorkspacePathReport, WORKSPACE_COLORS,
    WORKSPACE_ICONS,
};

#[derive(Debug, Clone)]
pub struct NativeChatInput {
    pub session_id: String,
    pub content: String,
    pub client_message_id: Option<String>,
    pub reasoning_effort: Option<String>,
    /// Local UI attachments retain their original data URL until the shared
    /// runtime persists them into the workspace attachment store.
    pub attachments: Vec<NativeChatAttachment>,
}

#[derive(Debug, Clone)]
pub struct NativeChatAttachment {
    pub name: String,
    pub content: String,
    pub content_type: String,
}

#[derive(Debug, Clone)]
pub struct NativeSession {
    pub goal: NativeGoal,
    pub id: String,
    pub title: String,
    pub updated_at: f64,
    pub agent_id: Option<String>,
    pub workspace_id: Option<String>,
    pub workspace_name: String,
    pub workspace_color: String,
    pub consumed_tokens: i64,
    pub tool_calls: i64,
    pub model_request_count: i64,
    pub quota_used: i64,
    pub runtime_status: String,
    pub locked: bool,
    pub reasoning_effort: String,
    /// Thread model override (workspace meta). `None` means the thread follows
    /// the agent configuration.
    pub session_model_name: Option<String>,
    /// Remote origin of the thread (`cloud` / `device:<id>`), from the
    /// session row's `spawned_by`; `None` for locally created threads.
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NativeMessage {
    pub turn_id: String,
    pub text: String,
    pub mine: bool,
    pub created_at: f64,
    pub state: String,
    pub stats_status: String,
    pub user_round: i64,
    pub stats: NativeMessageStats,
}

/// Compact, bounded tool-call presentation for native frontends. The raw
/// durable payload remains in storage; this projection contains only the
/// fields needed to render the collapsed row and its local detail view.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeWorkflowEntry {
    pub id: String,
    pub title: String,
    pub preview: String,
    pub detail: String,
    pub state: String,
    pub tokens: String,
    pub duration: String,
    pub sections: Vec<NativeWorkflowSection>,
    /// Structured file diffs for the timeline patch card; empty for every tool
    /// that does not change files.
    pub patch_files: Vec<NativePatchFile>,
}

/// One child run of a parent turn, projected from the parent thread log's
/// `subagent_run` item. The card carries identity, live status and counters
/// only; the child thread itself is loaded separately for the detail view.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NativeSubagentCard {
    pub item_id: String,
    /// The child session; the detail view loads its turns directly.
    pub session_id: String,
    pub run_id: String,
    pub title: String,
    pub status: String,
    pub terminal: bool,
    pub failed: bool,
    pub latest_message: String,
    pub error_message: String,
    pub can_terminate: bool,
    pub tool_calls: i64,
    pub model_requests: i64,
    pub context_tokens: i64,
    pub credits: i64,
}

impl NativeSubagentCard {
    /// Project one card out of a `subagent_run` item payload. `payload` is the
    /// durable item payload with the child runtime under `runtime`; missing
    /// fields degrade to defaults instead of failing the whole turn.
    pub fn from_payload(payload: &Value) -> Option<Self> {
        let runtime = payload.get("runtime").unwrap_or(&Value::Null);
        let session_id = runtime["session_id"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        if session_id.is_empty() {
            return None;
        }
        let metrics = &runtime["metrics"];
        Some(Self {
            item_id: payload["item_id"].as_str().unwrap_or_default().to_string(),
            session_id,
            run_id: runtime["run_id"].as_str().unwrap_or_default().to_string(),
            title: runtime["title"]
                .as_str()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or("子智能体")
                .trim()
                .to_string(),
            status: runtime["status"].as_str().unwrap_or_default().to_string(),
            terminal: runtime["terminal"].as_bool().unwrap_or(false),
            failed: runtime["failed"].as_bool().unwrap_or(false),
            latest_message: runtime["latest_message"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            error_message: runtime["error_message"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            can_terminate: runtime["can_terminate"].as_bool().unwrap_or(false),
            tool_calls: metrics["tool_calls"].as_i64().unwrap_or(0),
            model_requests: metrics["model_request_count"].as_i64().unwrap_or(0),
            context_tokens: metrics["context_tokens"].as_i64().unwrap_or(0),
            credits: metrics["account_credits_consumed"].as_i64().unwrap_or(0),
        })
    }

    /// Whether the child has run to an end the card can settle on.
    pub fn is_running(&self) -> bool {
        !self.terminal && !matches!(self.status.as_str(), "closed" | "cancelled" | "canceled")
    }
}

/// One file of a bounded patch projection (§7.4). Line numbers stay as strings
/// because a pending preview has none.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NativePatchFile {
    pub path: String,
    pub action: String,
    pub added: String,
    pub deleted: String,
    pub lines: Vec<NativePatchLine>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NativePatchLine {
    pub text: String,
    pub kind: String,
    pub number: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NativeWorkflowSection {
    pub title: String,
    pub meta: String,
    pub kind: String,
    pub lines: Vec<NativeWorkflowLine>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NativeWorkflowLine {
    pub ratio: f32,
    pub text: String,
    pub kind: String,
    pub number: String,
}

pub struct NativeDesktop {
    runtime: Arc<Runtime>,
    desktop: DesktopRuntime,
    settings_lock: std::sync::Mutex<()>,
    terminal: NativeTerminal,
    instance_id: String,
    open_threads: Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>>,
    lease_heartbeat_stop: Arc<AtomicBool>,
}

impl NativeDesktop {
    pub fn start() -> Result<Self> {
        Self::start_with_args(DesktopArgs::native_defaults())
    }

    pub fn start_with_args(mut args: DesktopArgs) -> Result<Self> {
        wunder_server::rustls_provider::install_process_default_provider();
        args.native_runtime = true;
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(8)
                .enable_all()
                .build()?,
        );
        wunder_server::cloud::shared().set_client_kind("desktop");
        let desktop = runtime.block_on(DesktopRuntime::init(&args))?;
        // Startup self-healing probe: refresh the session, re-synthesize the
        // cloud models and fire one heartbeat in the background. Returns
        // immediately; never blocks startup and never logs secrets.
        runtime.block_on(
            desktop
                .state
                .cloud
                .startup_probe(&desktop.state.config_store),
        );
        // Interlink tunnel (互通方案 I9): one shared client per process. A
        // no-session start stays a cheap poller; the loop picks up the session
        // after a later login on its own. `start` takes a blocking lock, so it
        // must run outside the async executor's worker threads.
        {
            let state = Arc::clone(&desktop.state);
            let user_id = desktop.user_id.clone();
            // Interlink startup may take a blocking lock while it restores its
            // durable ledger. Do not wait for it before returning the native
            // desktop handle: the UI can render immediately and the tunnel
            // publishes its state asynchronously when ready.
            runtime.spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    wunder_server::interlink::client::start_with_options(
                        state,
                        wunder_server::interlink::client::InterlinkLocalOptions {
                            local_user_id: user_id,
                            workspace_id: None,
                            session_base_dir: wunder_server::cloud::session::wunder_home_dir(),
                        },
                    );
                })
                .await;
            });
        }
        // A console for the shell, when the host ships winpty next to its git.
        let settings = load_desktop_settings(&desktop.settings_path).unwrap_or_default();
        let terminal = NativeTerminal::new(
            Arc::clone(&runtime),
            Arc::clone(&desktop.state.storage),
            resolve_winpty_library(&desktop.app_dir, &settings),
        );
        let instance_id = uuid::Uuid::new_v4().simple().to_string();
        let open_threads: Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let lease_heartbeat_stop = Arc::new(AtomicBool::new(false));
        let heartbeat_threads = Arc::clone(&open_threads);
        let heartbeat_stop = Arc::clone(&lease_heartbeat_stop);
        let heartbeat_storage = Arc::clone(&desktop.state.storage);
        let heartbeat_instance = instance_id.clone();
        thread::Builder::new()
            .name("wunder-desktop-thread-lease".into())
            .spawn(move || {
                while !heartbeat_stop.load(Ordering::Acquire) {
                    thread::sleep(std::time::Duration::from_secs(10));
                    if heartbeat_stop.load(Ordering::Acquire) {
                        break;
                    }
                    let sessions = heartbeat_threads
                        .lock()
                        .map(|set| {
                            set.iter()
                                .map(|(session, token)| (session.clone(), *token))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    for (session, token) in sessions {
                        let _ = heartbeat_storage.touch_thread_open_lease(
                            &session,
                            &heartbeat_instance,
                            token,
                            30.0,
                        );
                    }
                }
            })
            .ok();
        Ok(Self {
            runtime,
            desktop,
            settings_lock: std::sync::Mutex::new(()),
            terminal,
            instance_id,
            open_threads,
            lease_heartbeat_stop,
        })
    }

    fn ensure_thread_open(&self, session_id: &str) -> Result<()> {
        let session_id = session_id.trim();
        anyhow::ensure!(!session_id.is_empty(), "线程不存在");
        let mut leases = self
            .open_threads
            .lock()
            .map_err(|_| anyhow!("thread lease state unavailable"))?;
        if let Some(&fencing_token) = leases.get(session_id) {
            if self.state().storage.touch_thread_open_lease(
                session_id,
                &self.instance_id,
                fencing_token,
                30.0,
            )? {
                return Ok(());
            }
            return Err(anyhow!(
                "THREAD_IN_USE: thread lease expired; close and reopen the thread"
            ));
        }
        use wunder_server::storage::ThreadOpenLeaseStatus;
        match self.state().storage.try_acquire_thread_open_lease(
            session_id,
            self.user_id(),
            &self.instance_id,
            "desktop",
            30.0,
        )? {
            ThreadOpenLeaseStatus::Acquired { fencing_token } => {
                leases.insert(session_id.to_string(), fencing_token);
                Ok(())
            }
            ThreadOpenLeaseStatus::Busy => Err(anyhow!(
                "THREAD_IN_USE: 线程正在由其他蛹或舵机实例打开，关闭占用实例后重试"
            )),
        }
    }

    pub fn open_session(&self, session_id: &str) -> Result<()> {
        self.ensure_thread_open(session_id)
    }

    pub fn close_session(&self, session_id: &str) -> Result<()> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Ok(());
        }
        let mut leases = self
            .open_threads
            .lock()
            .map_err(|_| anyhow!("thread lease state unavailable"))?;
        let fencing_token = leases.get(session_id).copied();
        if let Some(fencing_token) = fencing_token {
            self.state().storage.release_thread_open_lease(
                session_id,
                &self.instance_id,
                fencing_token,
            )?;
            leases.remove(session_id);
        }
        Ok(())
    }

    pub fn user_id(&self) -> &str {
        &self.desktop.user_id
    }

    pub fn state(&self) -> &Arc<wunder_server::state::AppState> {
        &self.desktop.state
    }

    /// Threads of one workspace, newest activity first. `workspace_id` empty
    /// means every active thread of the user.
    pub fn list_sessions(&self, workspace_id: Option<&str>) -> Result<Vec<NativeSession>> {
        let (records, _) = match workspace_id.map(str::trim).filter(|id| !id.is_empty()) {
            Some(workspace_id) => self.desktop.state.storage.list_chat_sessions_by_workspace(
                &self.desktop.user_id,
                workspace_id,
                Some("active"),
                0,
                100,
            )?,
            None => self.desktop.state.storage.list_work_chat_sessions(
                &self.desktop.user_id,
                None,
                Some("active"),
                0,
                100,
            )?,
        };
        let goals = self.state().storage.list_session_goals(
            self.user_id(),
            &records
                .iter()
                .map(|record| record.session_id.clone())
                .collect::<Vec<_>>(),
        )?;
        let workspaces = self.workspace_lookup();
        Ok(records
            .into_iter()
            .map(|record| {
                let goal = goals
                    .iter()
                    .find(|goal| goal.session_id == record.session_id);
                let mut session = self.session_with_stats(record, &workspaces);
                if let Some(goal) = goal {
                    let activation = self
                        .state()
                        .kernel
                        .orchestrator
                        .goal_handle()
                        .activation(&goal.session_id)
                        .as_str()
                        .to_string();
                    session.goal = NativeGoal {
                        objective: goal.objective.clone(),
                        phase: goal.phase.clone(),
                        rounds_started: goal.rounds_started,
                        max_goal_rounds: goal.max_goal_rounds,
                        activation,
                    };
                }
                session
            })
            .collect())
    }

    /// Workspace name/color keyed by id for session projection.
    fn workspace_lookup(
        &self,
    ) -> std::collections::HashMap<String, wunder_server::storage::WorkspaceRecord> {
        self.state()
            .storage
            .list_workspaces(self.user_id())
            .unwrap_or_default()
            .into_iter()
            .map(|record| (record.workspace_id.clone(), record))
            .collect()
    }

    fn session_with_stats(
        &self,
        record: wunder_server::storage::ChatSessionRecord,
        workspaces: &std::collections::HashMap<String, wunder_server::storage::WorkspaceRecord>,
    ) -> NativeSession {
        let session_id = record.session_id.clone();
        let overview = self
            .desktop
            .state
            .monitor
            .get_log_overview(&record.session_id);
        let reasoning_effort = self
            .desktop
            .state
            .workspace
            .load_session_reasoning_effort(self.user_id(), &record.session_id);
        let session_model_name = self
            .desktop
            .state
            .workspace
            .load_session_model_override(self.user_id(), &record.session_id);
        let workspace = record
            .workspace_id
            .as_deref()
            .and_then(|id| workspaces.get(id));
        NativeSession {
            goal: NativeGoal::default(),
            id: session_id.clone(),
            title: record.title,
            updated_at: record.updated_at,
            agent_id: record.agent_id,
            workspace_id: record.workspace_id,
            workspace_name: workspace
                .map(|record| record.name.clone())
                .unwrap_or_default(),
            workspace_color: workspace
                .map(|record| record.color.clone())
                .unwrap_or_default(),
            consumed_tokens: overview
                .as_ref()
                .and_then(|v| v.get("consumed_tokens"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            tool_calls: overview
                .as_ref()
                .and_then(|v| v.get("tool_calls"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            model_request_count: overview
                .as_ref()
                .and_then(|v| v.get("model_request_count"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            quota_used: overview
                .as_ref()
                .and_then(|v| v.get("quota_used").or_else(|| v.get("model_request_count")))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            runtime_status: overview
                .as_ref()
                .and_then(|v| v.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("done")
                .to_string(),
            origin: record
                .spawned_by
                .as_deref()
                .and_then(|source| source.strip_prefix("remote:"))
                .map(str::to_string),
            locked: self
                .state()
                .storage
                .get_thread_open_lease(&session_id, self.user_id())
                .ok()
                .flatten()
                .is_some_and(|lease| lease.owner_instance_id != self.instance_id),
            reasoning_effort,
            session_model_name,
        }
    }

    pub fn save_session_reasoning_effort(&self, session_id: &str, value: &str) -> Result<String> {
        self.ensure_thread_open(session_id)?;
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Err(anyhow!("线程不存在"));
        }
        if self
            .state()
            .user_store
            .get_chat_session(self.user_id(), session_id)?
            .is_none()
        {
            return Err(anyhow!("线程不存在"));
        }
        self.desktop
            .state
            .workspace
            .save_session_reasoning_effort(self.user_id(), session_id, value)
            .map_err(Into::into)
    }

    /// Pins the thread's model override (empty clears it). The model key is
    /// validated against the engine's LLM catalogue; a stale key written
    /// earlier degrades to the agent/global chain at turn time.
    pub fn set_session_model(&self, session_id: &str, model_key: &str) -> Result<()> {
        self.ensure_thread_open(session_id)?;
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Err(anyhow!("线程不存在"));
        }
        if self
            .state()
            .user_store
            .get_chat_session(self.user_id(), session_id)?
            .is_none()
        {
            return Err(anyhow!("线程不存在"));
        }
        let model_key = model_key.trim();
        if !model_key.is_empty() {
            let config = self.runtime.block_on(self.state().config_store.get());
            let valid = config.llm.models.get(model_key).is_some_and(|model| {
                matches!(
                    model.model_type.as_deref().unwrap_or("llm"),
                    "llm" | "LLM" | "Llm"
                )
            });
            if !valid {
                return Err(anyhow!("模型不存在或不可用"));
            }
        }
        self.desktop
            .state
            .workspace
            .save_session_model_override(
                self.user_id(),
                session_id,
                (!model_key.is_empty()).then_some(model_key),
            )
            .map_err(Into::into)
    }

    pub fn get_session(&self, session_id: &str) -> Result<(NativeSession, Vec<NativeMessage>)> {
        let (session, turns) = self.get_session_turns(session_id)?;
        Ok((
            session,
            turns
                .into_iter()
                .flat_map(|turn| [turn.user, turn.assistant])
                .collect(),
        ))
    }

    pub fn get_session_info(&self, session_id: &str) -> Result<NativeSession> {
        let record = self
            .state()
            .user_store
            .get_chat_session(self.user_id(), session_id.trim())?
            .ok_or_else(|| anyhow!("chat session not found"))?;
        self.ensure_thread_open(session_id)?;
        let workspaces = self.workspace_lookup();
        Ok(self.session_with_stats(record, &workspaces))
    }

    pub fn get_session_turns(
        &self,
        session_id: &str,
    ) -> Result<(NativeSession, Vec<NativeChatTurn>)> {
        let cleaned = session_id.trim();
        self.ensure_thread_open(cleaned)?;
        let record = self
            .desktop
            .state
            .user_store
            .get_chat_session(&self.desktop.user_id, cleaned)?
            .ok_or_else(|| anyhow!("chat session not found"))?;
        self.ensure_thread_open(cleaned)?;
        let turns = self.load_chat_turns(cleaned)?;
        let workspaces = self.workspace_lookup();
        Ok((self.session_with_stats(record, &workspaces), turns))
    }

    /// Create a thread in the given workspace (or the default workspace when
    /// none is provided). The single built-in agent owns every thread.
    pub fn create_session(&self, workspace_id: Option<&str>) -> Result<NativeSession> {
        self.runtime
            .block_on(wunder_server::agent_management::owned(
                self.state(),
                self.user_id(),
                "__default__",
            ))?;
        let workspace_id = match workspace_id.map(str::trim).filter(|id| !id.is_empty()) {
            Some(id) => {
                self.state()
                    .storage
                    .get_workspace(self.user_id(), id)?
                    .ok_or_else(|| anyhow!("工作区不存在"))?;
                id.to_string()
            }
            None => self
                .state()
                .storage
                .list_workspaces(self.user_id())?
                .first()
                .map(|record| record.workspace_id.clone())
                .ok_or_else(|| anyhow!("工作区不存在"))?,
        };
        let now = now_ts();
        let id = format!("native_{}", uuid::Uuid::new_v4().simple());
        let record = wunder_server::storage::ChatSessionRecord {
            session_id: id,
            user_id: self.desktop.user_id.clone(),
            title: "新会话".to_string(),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: None,
            workspace_id: Some(workspace_id),
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        };
        self.desktop.state.user_store.upsert_chat_session(&record)?;
        let workspaces = self.workspace_lookup();
        Ok(self.session_with_stats(record, &workspaces))
    }

    pub fn rename_session(&self, session_id: &str, title: &str) -> Result<()> {
        let title = title.trim();
        if title.is_empty() || title.chars().count() > 120 || title.chars().any(char::is_control) {
            return Err(anyhow!("线程名称无效"));
        }
        self.desktop.state.user_store.update_chat_session_title(
            &self.desktop.user_id,
            session_id.trim(),
            title,
            now_ts(),
        )
    }

    pub fn archive_session(&self, session_id: &str) -> Result<()> {
        let mut record = self
            .desktop
            .state
            .user_store
            .get_chat_session(&self.desktop.user_id, session_id.trim())?
            .ok_or_else(|| anyhow!("线程不存在"))?;
        record.status = "archived".into();
        record.updated_at = now_ts();
        self.desktop.state.user_store.upsert_chat_session(&record)
    }

    pub fn session_detail_page(
        &self,
        session_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        self.ensure_thread_open(session_id)?;
        self.desktop
            .state
            .monitor
            .get_detail_page(session_id.trim(), offset, limit)
            .ok_or_else(|| anyhow!("线程日志不存在"))
    }

    pub fn cancel_chat(&self, session_id: &str) -> Result<()> {
        self.ensure_thread_open(session_id)?;
        self.runtime.block_on(stream::cancel_chat(
            &self.desktop.state,
            &self.desktop.user_id,
            session_id,
        ))
    }

    /// The child-run cards of one parent thread, as the parent thread log's
    /// `subagent_run` items report them. Bounded by the runtime list limit.
    pub fn subagent_cards(&self, parent_session: &str) -> Result<Vec<NativeSubagentCard>> {
        let parent = parent_session.trim();
        anyhow::ensure!(!parent.is_empty(), "missing parent thread identity");
        let state = self.state();
        let items = wunder_server::list_parent_subagents(
            state.storage.as_ref(),
            Some(&state.monitor),
            self.user_id(),
            parent,
            None,
        )?;
        Ok(items
            .iter()
            // The list payloads carry the runtime fields at the top level;
            // only the thread item nests them under `runtime`.
            .filter_map(|item| NativeSubagentCard::from_payload(&json!({"runtime": item})))
            .collect())
    }

    /// Interrupt or close one child run of the parent thread. The runtime owns
    /// the permission and settlement semantics; the desktop only forwards.
    pub fn subagent_control(
        &self,
        parent_session: &str,
        action: &str,
        child_session: &str,
    ) -> Result<()> {
        let parent = parent_session.trim();
        let child = child_session.trim();
        anyhow::ensure!(!parent.is_empty(), "missing parent thread identity");
        anyhow::ensure!(!child.is_empty(), "missing child thread identity");
        anyhow::ensure!(
            matches!(action.trim(), "interrupt" | "terminate"),
            "unsupported subagent action"
        );
        self.ensure_thread_open(parent)?;
        let state = self.state();
        wunder_server::control_parent_subagents(
            state.storage.as_ref(),
            Some(&state.monitor),
            self.user_id(),
            parent,
            action.trim(),
            &[child.to_string()],
            None,
        )?;
        Ok(())
    }

    pub fn send_chat(&self, input: NativeChatInput) -> Result<NativeStream> {
        if NativeChatCommand::parse(&input.content).is_some() {
            return Err(anyhow!("请通过原生命令入口执行斜杠命令"));
        }
        if input.session_id.trim().is_empty()
            || (input.content.trim().is_empty() && input.attachments.is_empty())
        {
            return Err(anyhow!(
                "chat session and content or attachment are required"
            ));
        }
        self.ensure_thread_open(&input.session_id)?;
        let token = self
            .open_threads
            .lock()
            .map_err(|_| anyhow!("thread lease unavailable"))?
            .get(&input.session_id)
            .copied()
            .ok_or_else(|| anyhow!("THREAD_IN_USE"))?;
        Ok(stream::start(
            &self.runtime,
            self.desktop.state.clone(),
            self.desktop.user_id.clone(),
            input,
            json!({"owner":self.instance_id,"token":token}),
        ))
    }

    /// Start (or reuse) a terminal session bound to the desktop user.
    pub fn terminal_start(&self, spec: &NativeTerminalSpec) -> Result<DesktopTerminalSnapshot> {
        self.terminal.start(self.user_id(), spec)
    }

    pub fn terminal_watch(&self, session_id: &str, terminal_id: &str) -> Result<NativeTerminalWatch> {
        self.terminal.watch(self.user_id(), session_id, terminal_id)
    }

    /// Fetch the incremental output frame since `last_seq` for `terminal_id`.
    pub fn terminal_poll(
        &self,
        session_id: &str,
        terminal_id: &str,
        last_seq: u64,
    ) -> Result<NativeTerminalFrame> {
        self.terminal
            .poll(self.user_id(), session_id, terminal_id, last_seq)
    }

    /// Send raw bytes (already encoded) to the terminal stdin.
    pub fn terminal_send(&self, session_id: &str, terminal_id: &str, input: &[u8]) -> Result<()> {
        self.terminal
            .send(self.user_id(), session_id, terminal_id, input)
    }

    /// Submit a complete command line; the terminal service owns line endings
    /// and echo for the selected shell/backend.
    pub fn terminal_send_command(
        &self,
        session_id: &str,
        terminal_id: &str,
        command: &str,
    ) -> Result<()> {
        self.terminal
            .send_command(self.user_id(), session_id, terminal_id, command)
    }

    /// Interrupt the running command: Ctrl-C on a console, otherwise the shell
    /// process is killed because piped stdin carries no signal path.
    pub fn terminal_cancel(&self, session_id: &str, terminal_id: &str) -> Result<()> {
        self.terminal
            .cancel(self.user_id(), session_id, terminal_id)
    }

    /// Drop the terminal record (also kills the process).
    pub fn terminal_close(&self, session_id: &str, terminal_id: &str) -> Result<()> {
        self.terminal.close(self.user_id(), session_id, terminal_id)
    }

    /// List live terminal snapshots for `session_id`.
    pub fn terminal_list(&self, session_id: &str) -> Vec<DesktopTerminalSnapshot> {
        self.terminal.list(self.user_id(), session_id)
    }

    /// Report the panel's cell grid to the shell so a console backend can
    /// reflow its lines; piped shells ignore it.
    pub fn terminal_resize(
        &self,
        session_id: &str,
        terminal_id: &str,
        cols: i32,
        rows: i32,
    ) -> Result<()> {
        self.terminal.resize(
            self.user_id(),
            session_id,
            terminal_id,
            cols.max(1) as u16,
            rows.max(1) as u16,
        )
    }

    /// Durable transcript of the session scope, raw shell output oldest first.
    /// The native UI replays it through its ANSI renderer on (re)entry so the
    /// panel shows the same history after a restart.
    pub fn terminal_transcript(&self, session_id: &str) -> String {
        self.terminal.transcript(self.user_id(), session_id)
    }

    /// Returns whether the desktop runtime has a configured default ASR model.
    /// The check is deliberately synchronous at this façade boundary so UI code
    /// can reject an empty recording before starting device capture.
    pub fn has_asr_model(&self) -> bool {
        let config = self.runtime.block_on(self.state().config_store.get());
        wunder_server::multimodal_models::resolve_asr_model(&config, None).is_some()
    }

    /// Transcribe an in-memory recording through the shared multimodal runtime.
    /// No local HTTP or bridge transport is involved; the model provider call,
    /// when configured remotely, remains inside the existing runtime service.
    pub fn transcribe_audio(
        &self,
        filename: String,
        content_type: String,
        audio_bytes: Vec<u8>,
    ) -> Result<String> {
        if audio_bytes.is_empty() {
            return Err(anyhow!("录音为空"));
        }
        let config = self.runtime.block_on(self.state().config_store.get());
        let response =
            self.runtime
                .block_on(wunder_server::multimodal_models::transcribe_audio(
                    &config,
                    wunder_server::multimodal_models::AudioTranscriptionRequest {
                        audio_bytes: Bytes::from(audio_bytes),
                        filename,
                        content_type,
                        model_name: None,
                        language: None,
                        prompt: None,
                        response_format: None,
                        temperature: None,
                    },
                ))?;
        let text = response.text.trim().to_string();
        if text.is_empty() {
            return Err(anyhow!("语音识别没有返回文字"));
        }
        Ok(text)
    }
}

impl Drop for NativeDesktop {
    fn drop(&mut self) {
        self.lease_heartbeat_stop.store(true, Ordering::Release);
        let sessions = self
            .open_threads
            .lock()
            .map(|mut set| set.drain().collect::<Vec<_>>())
            .unwrap_or_default();
        for (session_id, fencing_token) in sessions {
            let _ = self.desktop.state.storage.release_thread_open_lease(
                &session_id,
                &self.instance_id,
                fencing_token,
            );
        }
    }
}

fn message_from_value(value: Value) -> Option<NativeMessage> {
    if value.pointer("/meta/type").and_then(Value::as_str) == Some("model_context_internal") {
        return None;
    }
    let role = value.get("role").and_then(Value::as_str)?;
    if role != "user" && role != "assistant" {
        return None;
    }
    // Desktop history reads the storage record directly (rather than the web
    // transcript mapper), so accept all persisted locations used by older
    // records and by the current message_stats metadata.
    let stats = value
        .get("stats")
        .or_else(|| value.get("message_stats"))
        .or_else(|| value.pointer("/meta/message_stats"))
        .unwrap_or(&Value::Null);
    Some(NativeMessage {
        turn_id: value
            .get("turn_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        text: value.get("content").and_then(Value::as_str)?.to_string(),
        mine: role == "user",
        created_at: value
            .get("created_at")
            .and_then(Value::as_f64)
            .unwrap_or_default(),
        state: value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        stats_status: if role == "assistant" {
            value
                .get("status")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("任务完成")
                .to_string()
        } else {
            String::new()
        },
        user_round: value
            .get("user_round")
            .or_else(|| value.get("userRound"))
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        stats: NativeMessageStats::from_payload(stats),
    })
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs_f64())
        .unwrap_or_default()
}
