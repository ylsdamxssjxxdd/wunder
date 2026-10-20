//! In-process desktop façade for native UI frontends.
//!
//! This layer deliberately uses the same AppState, ThreadRuntime and
//! Orchestrator as the server. It only translates typed UI requests to runtime
//! calls; it does not duplicate scheduling or permission rules.

use crate::{args::DesktopArgs, runtime::DesktopRuntime};
use anyhow::{anyhow, Result};
use bytes::Bytes;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;
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

#[path = "native_chat_process.rs"]
mod chat_process;
#[path = "native_chat_turns.rs"]
mod chat_turns;
pub use chat_process::{NativeProcessItem, NativeProcessPage};
#[path = "native_observer.rs"]
mod observer;
#[path = "native_queue.rs"]
mod queue;
#[path = "native_stream.rs"]
mod stream;
pub use crate::runtime::RuntimeToolStatus;
pub use chat_turns::{NativeChatRound, NativeChatTurn, NativePlanStep, NativePlanView};
pub use observer::{NativeThreadUpdate, NativeThreadWatch};
pub use queue::{NativeQueueTurn, NATIVE_QUEUE_LIMIT};
pub use stream::{NativeChatEvent, NativeStream};
#[path = "native_catalog.rs"]
mod catalog;
#[path = "native_cloud.rs"]
mod cloud;
#[path = "native_cloud_threads.rs"]
mod cloud_threads;
#[path = "native_external.rs"]
mod external;
pub use cloud_threads::{NativeCloudAgent, NativeCloudThread, NativeCloudThreadView};

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

/// One bounded thread-open snapshot. `change_cursor` is captured before the
/// history page, so a watcher can replay every commit made while a slow disk
/// was reading that page. Replaying an already present root is harmless; a
/// later commit being skipped is not.
#[derive(Debug, Clone)]
pub struct NativeChatSnapshot {
    pub session: NativeSession,
    pub turns: Vec<NativeChatTurn>,
    pub change_cursor: i64,
}

/// A short lived, bounded projection cache for thread switching.  The
/// observer still replays changes from `change_cursor`, so this cache is only
/// a fast first paint and never the source of truth.
#[derive(Debug, Clone)]
struct CachedNativeChatSnapshot {
    snapshot: NativeChatSnapshot,
    cached_at: Instant,
}

const CHAT_SNAPSHOT_CACHE_LIMIT: usize = 8;
const CHAT_SNAPSHOT_CACHE_TTL: Duration = Duration::from_secs(30);

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
    pub attachments: Vec<NativeMessageAttachment>,
}

/// Bounded attachment card for a user message: enough to render a thumbnail
/// or file row and to open the stored file from the workspace.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NativeMessageAttachment {
    pub name: String,
    pub content_type: String,
    pub public_path: String,
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

    /// Project one card out of a live `subagent_dispatch_item_update` payload.
    /// The dispatch item is flat (the durable card nests the same fields under
    /// `runtime`), so it uses its own mapping but degrades the same way: an
    /// item without a child session is not renderable.
    pub fn from_dispatch_item(payload: &Value) -> Option<Self> {
        let session_id = payload["session_id"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        if session_id.is_empty() {
            return None;
        }
        let state = &payload["agent_state"];
        let status = payload["status"]
            .as_str()
            .or_else(|| state["status"].as_str())
            .unwrap_or_default()
            .to_string();
        let failed = payload["failed"].as_bool().unwrap_or(false)
            || matches!(status.as_str(), "failed" | "error");
        let error_message = payload["error_message"]
            .as_str()
            .or_else(|| payload["error"].as_str())
            .unwrap_or_default()
            .to_string();
        let latest_message = payload["latest_message"]
            .as_str()
            .or_else(|| payload["result_preview"].as_str())
            .or_else(|| state["message"].as_str())
            .unwrap_or_default()
            .to_string();
        let metrics = &payload["metrics"];
        Some(Self {
            item_id: payload["item_id"].as_str().unwrap_or_default().to_string(),
            session_id,
            run_id: payload["run_id"].as_str().unwrap_or_default().to_string(),
            title: payload["label"]
                .as_str()
                .or_else(|| payload["spawn_label"].as_str())
                .or_else(|| payload["title"].as_str())
                .filter(|title| !title.trim().is_empty())
                .unwrap_or("子智能体")
                .trim()
                .to_string(),
            status,
            terminal: payload["terminal"].as_bool().unwrap_or(false),
            failed,
            latest_message,
            error_message,
            can_terminate: payload["can_terminate"].as_bool().unwrap_or(false),
            tool_calls: metrics["tool_calls"]
                .as_i64()
                .or_else(|| payload["tool_calls"].as_i64())
                .unwrap_or(0),
            model_requests: metrics["model_request_count"]
                .as_i64()
                .or_else(|| payload["model_request_count"].as_i64())
                .unwrap_or(0),
            context_tokens: metrics["context_tokens"].as_i64().unwrap_or(0),
            credits: metrics["account_credits_consumed"].as_i64().unwrap_or(0),
        })
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
    instance_id: String,
    open_threads: Arc<Mutex<HashMap<String, u64>>>,
    /// Recent thread headers are reused when the user switches away and back.
    /// Keeping this on the façade avoids a second HDD history decode while the
    /// durable observer catches up from the saved cursor.
    chat_snapshot_cache: Arc<Mutex<HashMap<String, CachedNativeChatSnapshot>>>,
    /// Every local selection/use bumps this counter. Delayed selection cleanup
    /// captures a value and only releases a lease if nobody returned to that
    /// thread in the meantime.
    lease_activity: Arc<Mutex<HashMap<String, u64>>>,
    /// Monotonic source for activity generations. Keeping this separate from
    /// the bounded per-open map prevents a delayed close from matching a
    /// reopened thread merely because its old entry was removed and recreated.
    lease_generation: Arc<AtomicU64>,
    /// Last successful ownership confirmation. Reopening a thread during this
    /// short healthy interval is pure in-memory work; after it expires we
    /// verify the fencing token before trusting the cached projection.
    lease_confirmed: Arc<Mutex<HashMap<String, Instant>>>,
    /// Close requests issued while a model run is still active are deferred;
    /// dropping the UI lease at that boundary made A -> B -> A fail because
    /// the runtime execution lock correctly prevented a new owner.
    lease_reapers: Arc<Mutex<HashMap<String, u64>>>,
    /// Serializes short local lease transitions. SQLite's fencing protects
    /// processes; this prevents an opener and a delayed local reaper from
    /// observing and then independently mutating the same in-memory token.
    lease_operations: Arc<Mutex<()>>,
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
        let instance_id = uuid::Uuid::new_v4().simple().to_string();
        let open_threads: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));
        let chat_snapshot_cache = Arc::new(Mutex::new(HashMap::new()));
        let lease_activity = Arc::new(Mutex::new(HashMap::new()));
        let lease_generation = Arc::new(AtomicU64::new(0));
        let lease_confirmed = Arc::new(Mutex::new(HashMap::new()));
        let lease_reapers = Arc::new(Mutex::new(HashMap::new()));
        let lease_operations = Arc::new(Mutex::new(()));
        let lease_heartbeat_stop = Arc::new(AtomicBool::new(false));
        let heartbeat_threads = Arc::clone(&open_threads);
        let heartbeat_confirmed = Arc::clone(&lease_confirmed);
        let heartbeat_operations = Arc::clone(&lease_operations);
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
                        // The SQLite write runs outside the operation lock so a
                        // slow disk never stalls a UI-side lease transition.
                        // Confirmation is published under the lock only if the
                        // fencing token is still current, so a close/reopen
                        // transition still retires this snapshot.
                        if !heartbeat_storage
                            .touch_thread_open_lease(&session, &heartbeat_instance, token, 30.0)
                            .unwrap_or(false)
                        {
                            continue;
                        }
                        let Ok(_operation) = heartbeat_operations.lock() else {
                            continue;
                        };
                        let current = heartbeat_threads
                            .lock()
                            .map(|set| set.get(&session).copied())
                            .unwrap_or(None);
                        if current != Some(token) {
                            continue;
                        }
                        if let Ok(mut confirmed) = heartbeat_confirmed.lock() {
                            confirmed.insert(session, Instant::now());
                        }
                    }
                }
            })
            .ok();
        Ok(Self {
            runtime,
            desktop,
            settings_lock: std::sync::Mutex::new(()),
            instance_id,
            open_threads,
            chat_snapshot_cache,
            lease_activity,
            lease_generation,
            lease_confirmed,
            lease_reapers,
            lease_operations,
            lease_heartbeat_stop,
        })
    }

    fn ensure_thread_open(&self, session_id: &str) -> Result<()> {
        let session_id = session_id.trim();
        anyhow::ensure!(!session_id.is_empty(), "线程不存在");
        // Keep every local transition of one fencing token ordered. Without
        // this an old idle reaper can race a second opener after both have
        // inspected the same token, then drop the lease the opener returned.
        let _operation = self
            .lease_operations
            .lock()
            .map_err(|_| anyhow!("thread lease operation unavailable"))?;
        let existing = self
            .open_threads
            .lock()
            .map_err(|_| anyhow!("thread lease state unavailable"))?
            .get(session_id)
            .copied();
        if let Some(fencing_token) = existing {
            let recently_confirmed = self
                .lease_confirmed
                .lock()
                .ok()
                .and_then(|confirmed| confirmed.get(session_id).copied())
                .is_some_and(|confirmed| confirmed.elapsed() < Duration::from_secs(20));
            if recently_confirmed {
                // The heartbeat refreshes durable TTLs. Returning during its
                // healthy window avoids SQLite work on the interaction path.
                self.mark_thread_lease_used(session_id);
                return Ok(());
            }
            if self.state().storage.touch_thread_open_lease(
                session_id,
                &self.instance_id,
                fencing_token,
                30.0,
            )? {
                if let Ok(mut confirmed) = self.lease_confirmed.lock() {
                    confirmed.insert(session_id.to_string(), Instant::now());
                }
                self.mark_thread_lease_used(session_id);
                return Ok(());
            }
            let mut leases = self
                .open_threads
                .lock()
                .map_err(|_| anyhow!("thread lease state unavailable"))?;
            if leases.get(session_id).copied() == Some(fencing_token) {
                leases.remove(session_id);
            }
            if let Ok(mut confirmed) = self.lease_confirmed.lock() {
                confirmed.remove(session_id);
            }
            if let Ok(mut activity) = self.lease_activity.lock() {
                activity.remove(session_id);
            }
        }
        let mut leases = self
            .open_threads
            .lock()
            .map_err(|_| anyhow!("thread lease state unavailable"))?;
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
                drop(leases);
                if let Ok(mut confirmed) = self.lease_confirmed.lock() {
                    confirmed.insert(session_id.to_string(), Instant::now());
                }
                self.mark_thread_lease_used(session_id);
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
        let _operation = self
            .lease_operations
            .lock()
            .map_err(|_| anyhow!("thread lease operation unavailable"))?;
        let fencing_token = self
            .open_threads
            .lock()
            .map_err(|_| anyhow!("thread lease state unavailable"))?
            .get(session_id)
            .copied();
        if let Some(fencing_token) = fencing_token {
            self.state().storage.release_thread_open_lease(
                session_id,
                &self.instance_id,
                fencing_token,
            )?;
            if let Ok(mut leases) = self.open_threads.lock() {
                if leases.get(session_id).copied() == Some(fencing_token) {
                    leases.remove(session_id);
                }
            }
            if let Ok(mut activity) = self.lease_activity.lock() {
                activity.remove(session_id);
            }
            if let Ok(mut confirmed) = self.lease_confirmed.lock() {
                confirmed.remove(session_id);
            }
        }
        Ok(())
    }

    /// Delay a navigation-originated release briefly. If the user switches
    /// A -> B -> A in that interval, A's activity generation changes and the
    /// stale B cleanup becomes a no-op instead of closing the newly selected
    /// thread underneath its observer.
    pub fn defer_close_session(self: &Arc<Self>, session_id: &str, grace: Duration) {
        let session_id = session_id.trim().to_string();
        if session_id.is_empty() {
            return;
        }
        let activity = self
            .lease_activity
            .lock()
            .ok()
            .and_then(|activity| activity.get(&session_id).copied());
        let Some(activity) = activity else {
            return;
        };
        let desktop = Arc::clone(self);
        thread::spawn(move || {
            thread::sleep(grace);
            let unchanged = desktop
                .lease_activity
                .lock()
                .map(|current| current.get(&session_id).copied() == Some(activity))
                .unwrap_or(false);
            if unchanged {
                desktop.close_session_when_idle(session_id, activity);
            }
        });
    }

    fn mark_thread_lease_used(&self, session_id: &str) {
        if let Ok(mut activity) = self.lease_activity.lock() {
            let next = self
                .lease_generation
                .fetch_add(1, Ordering::Relaxed)
                .wrapping_add(1);
            activity.insert(session_id.to_string(), next);
        }
    }

    fn close_session_when_idle(&self, session_id: String, activity: u64) {
        let should_start = self
            .lease_reapers
            .lock()
            .map(
                |mut reapers| match reapers.insert(session_id.clone(), activity) {
                    Some(previous) => previous != activity,
                    None => true,
                },
            )
            .unwrap_or(false);
        if !should_start {
            return;
        }
        let storage = self.state().storage.clone();
        let open_threads = self.open_threads.clone();
        let lease_activity = self.lease_activity.clone();
        let lease_confirmed = self.lease_confirmed.clone();
        let lease_reapers = self.lease_reapers.clone();
        let lease_operations = self.lease_operations.clone();
        let instance_id = self.instance_id.clone();
        let user_id = self.user_id().to_string();
        thread::spawn(move || {
            loop {
                let current_activity = lease_activity
                    .lock()
                    .ok()
                    .and_then(|current| current.get(&session_id).copied());
                let current_reaper = lease_reapers
                    .lock()
                    .ok()
                    .and_then(|current| current.get(&session_id).copied());
                if current_activity != Some(activity) || current_reaper != Some(activity) {
                    break;
                }
                let still_owned = open_threads
                    .lock()
                    .map(|leases| leases.get(&session_id).copied())
                    .unwrap_or(None);
                let Some(fencing_token) = still_owned else {
                    break;
                };
                let running = storage
                    .list_session_locks_by_user(&user_id)
                    .map(|locks| locks.iter().any(|lock| lock.session_id == session_id))
                    .unwrap_or(true);
                if running {
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
                // Re-check activity under the same process-local transition
                // lock used by open/close. An opener either advances the
                // generation before we enter, or waits until the durable
                // release finishes and acquires a fresh fencing token.
                let Ok(_operation) = lease_operations.lock() else {
                    break;
                };
                let Ok(activity_guard) = lease_activity.lock() else {
                    break;
                };
                if activity_guard.get(&session_id).copied() != Some(activity) {
                    break;
                }
                if let Ok(mut leases) = open_threads.lock() {
                    if leases.get(&session_id).copied() == Some(fencing_token) {
                        leases.remove(&session_id);
                        if let Ok(mut confirmed) = lease_confirmed.lock() {
                            confirmed.remove(&session_id);
                        }
                        let _ = storage.release_thread_open_lease(
                            &session_id,
                            &instance_id,
                            fencing_token,
                        );
                    }
                }
                drop(activity_guard);
                break;
            }
            if let Ok(mut active) = lease_reapers.lock() {
                if active.get(&session_id).copied() == Some(activity) {
                    active.remove(&session_id);
                }
            }
        });
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
        let turns = self.load_chat_turns(cleaned)?;
        let workspaces = self.workspace_lookup();
        Ok((self.session_with_stats(record, &workspaces), turns))
    }

    /// Bounded, detail-free history for thread selection. Full rounds and
    /// tool output stay behind `chat_process_page`, which is invoked only
    /// after the work-process disclosure is opened.
    pub fn load_chat_headers(&self, session_id: &str) -> Result<Vec<NativeChatTurn>> {
        chat_turns::load_turn_headers(self.state(), self.user_id(), session_id)
    }

    /// Acquire the local open lease, then take the compact initial history and
    /// its replay cursor in one worker-owned sequence. The cursor deliberately
    /// precedes the history page: writes which race a mechanical-disk read are
    /// replayed through the passive watcher instead of being lost between two
    /// otherwise independent façade calls.
    pub fn open_chat_snapshot(&self, session_id: &str) -> Result<NativeChatSnapshot> {
        let session_id = session_id.trim();
        self.ensure_thread_open(session_id)?;
        if let Some(snapshot) = self.cached_chat_snapshot(session_id) {
            return Ok(snapshot);
        }
        let record = self
            .state()
            .user_store
            .get_chat_session(self.user_id(), session_id)?
            .ok_or_else(|| anyhow!("chat session not found"))?;
        let workspaces = self.workspace_lookup();
        let session = self.session_with_stats(record, &workspaces);
        let change_cursor = self
            .state()
            .storage
            .latest_thread_change_seq_by_session(session_id)?;
        let turns = self.load_chat_headers(session_id)?;
        let snapshot = NativeChatSnapshot {
            session,
            turns,
            change_cursor,
        };
        self.store_chat_snapshot(session_id, snapshot.clone());
        Ok(snapshot)
    }

    fn cached_chat_snapshot(&self, session_id: &str) -> Option<NativeChatSnapshot> {
        let mut cache = self.chat_snapshot_cache.lock().ok()?;
        cache.retain(|_, entry| entry.cached_at.elapsed() <= CHAT_SNAPSHOT_CACHE_TTL);
        cache.get(session_id).map(|entry| entry.snapshot.clone())
    }

    fn store_chat_snapshot(&self, session_id: &str, snapshot: NativeChatSnapshot) {
        let Ok(mut cache) = self.chat_snapshot_cache.lock() else {
            return;
        };
        cache.retain(|_, entry| entry.cached_at.elapsed() <= CHAT_SNAPSHOT_CACHE_TTL);
        if !cache.contains_key(session_id) && cache.len() >= CHAT_SNAPSHOT_CACHE_LIMIT {
            if let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, entry)| entry.cached_at)
                .map(|(id, _)| id.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            session_id.to_string(),
            CachedNativeChatSnapshot {
                snapshot,
                cached_at: Instant::now(),
            },
        );
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
        let session_id = session_id.trim();
        anyhow::ensure!(!session_id.is_empty(), "线程不存在");
        let mut record = self
            .desktop
            .state
            .user_store
            .get_chat_session(&self.desktop.user_id, session_id)?
            .ok_or_else(|| anyhow!("线程不存在"))?;
        // Archiving is a terminal user action. Settle model turns, queued
        // tasks, goals and monitor state first so an archived row can never be
        // resurrected as running after the next catalog refresh.
        self.ensure_thread_open(session_id)?;
        self.runtime.block_on(
            self.desktop
                .state
                .kernel
                .thread_runtime
                .cancel_session_activity(&self.desktop.user_id, session_id, "archive"),
        )?;
        record.status = "archived".into();
        record.updated_at = now_ts();
        self.desktop.state.user_store.upsert_chat_session(&record)?;
        self.close_session(session_id)
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

    /// Open the supplement's Git Bash console (`opt/git/git-bash.exe`). The
    /// window is owned by mintty, so this only spawns the launcher and returns.
    /// The active workspace is the shell's starting directory, and the
    /// supplement's tool directories are prepended to the child PATH so `git`,
    /// `python` and friends resolve to the bundled runtimes.
    pub fn launch_git_bash(&self, workspace_root: &str) -> Result<(), String> {
        let roots = wunder_server::local_bootstrap::supplement::roots(
            &self.desktop.wunder_home,
            &self.desktop.app_dir,
            None,
        );
        let git_bash = roots
            .iter()
            .map(|root| root.join("opt/git/git-bash.exe"))
            .find(|path| path.is_file())
            .ok_or_else(|| "补充包未安装 Git：opt/git/git-bash.exe 不存在".to_string())?;
        let workdir = {
            let requested = std::path::PathBuf::from(workspace_root.trim());
            if workspace_root.trim().is_empty() || !requested.is_dir() {
                self.desktop.workspace_root.clone()
            } else {
                requested
            }
        };
        let supplement_path = wunder_server::local_bootstrap::supplement::path_entries(&roots);
        let child_path = if supplement_path.is_empty() {
            std::env::var_os("PATH").unwrap_or_default()
        } else {
            let mut value = std::env::join_paths(supplement_path.iter().map(|p| p.as_path()))
                .map_err(|e| format!("构造 PATH 失败: {e}"))?
                .into_string()
                .map_err(|_| "PATH 编码错误".to_string())?;
            if let Some(existing) = std::env::var_os("PATH") {
                value.push(';');
                value.push_str(&existing.to_string_lossy());
            }
            std::ffi::OsString::from(value)
        };
        std::process::Command::new(&git_bash)
            .current_dir(&workdir)
            .env("PATH", child_path)
            .spawn()
            .map_err(|e| format!("启动 Git Bash 失败: {e}"))?;
        Ok(())
    }

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

#[cfg(test)]
mod thread_switch_tests {
    use super::*;

    fn start_isolated() -> NativeDesktop {
        let directory = tempfile::tempdir().expect("temporary desktop root");
        let config = directory.path().join("runtime/config");
        std::fs::create_dir_all(&config).expect("config directory");
        std::fs::write(config.join("wunder.yaml"), "{}\n").expect("runtime config");
        let settings = serde_json::json!({
            "workspace_root": "", "desktop_token": "", "updated_at": 0,
            "lan_mesh": {"enabled": false},
            "llm": {"default": "test-model", "models": {
                "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                               "base_url": "http://127.0.0.1:9/v1", "timeout_s": 5,
                               "support_vision": false}
            }}
        });
        std::fs::write(
            config.join("desktop.settings.json"),
            serde_json::to_vec(&settings).expect("settings json"),
        )
        .expect("desktop settings");
        let mut args = DesktopArgs::native_defaults();
        args.temp_root = Some(
            directory
                .path()
                .join("runtime")
                .canonicalize()
                .expect("runtime root"),
        );
        args.workspace = Some(directory.path().join("workspace"));
        // The runtime keeps SQLite alive after this helper returns.
        std::mem::forget(directory);
        NativeDesktop::start_with_args(args).expect("isolated desktop")
    }

    #[test]
    fn reopened_local_thread_reuses_lease_and_header_snapshot() {
        let desktop = Arc::new(start_isolated());
        let session = desktop.create_session(None).expect("session");
        let first = desktop.open_chat_snapshot(&session.id).expect("first open");
        desktop.defer_close_session(&session.id, Duration::from_millis(10));
        let second = desktop.open_chat_snapshot(&session.id).expect("reopen");
        assert_eq!(first.session.id, second.session.id);
        assert_eq!(first.change_cursor, second.change_cursor);
        std::thread::sleep(Duration::from_millis(30));
        assert!(
            desktop
                .open_threads
                .lock()
                .expect("lease map")
                .contains_key(&session.id),
            "a stale delayed close must not release a thread selected again"
        );
        desktop.defer_close_session(&session.id, Duration::from_millis(10));
        std::thread::sleep(Duration::from_millis(30));
        assert!(
            !desktop
                .open_threads
                .lock()
                .expect("lease map")
                .contains_key(&session.id),
            "the current delayed close releases an idle thread after the grace window"
        );
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
        text: wunder_server::stable_core::thread_view::message_content_text(
            value.get("content").unwrap_or(&Value::Null),
        )?
        .to_string(),
        attachments: value
            .get("attachments")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .take(8)
                    .filter_map(attachment_from_value)
                    .collect()
            })
            .unwrap_or_default(),
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

/// One attachment card from either the durable source shape (`content_type`)
/// or the bounded view projection (`mime_type`).
pub(crate) fn attachment_from_value(item: &Value) -> Option<NativeMessageAttachment> {
    let attachment = NativeMessageAttachment {
        name: item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        content_type: item
            .get("content_type")
            .or_else(|| item.get("mime_type"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        public_path: item
            .get("public_path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    };
    (attachment != NativeMessageAttachment::default()).then_some(attachment)
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs_f64())
        .unwrap_or_default()
}
