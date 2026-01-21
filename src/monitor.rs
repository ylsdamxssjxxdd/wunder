// 运行监控：记录会话状态、事件与系统资源指标，支持持久化恢复与取消控制。
use crate::config::{ObservabilityConfig, SandboxConfig};
use crate::i18n;
use crate::storage::StorageBackend;
use chrono::{DateTime, Local, Utc};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use std::any::Any;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{self, SyncSender, TrySendError},
    Arc,
};
use std::thread;
use sysinfo::{Disks, ProcessRefreshKind, System};
use tracing::{error, warn};
use walkdir::WalkDir;

const DEFAULT_EVENT_LIMIT: usize = 500;
const DEFAULT_PAYLOAD_LIMIT: usize = 4000;
const MIN_PAYLOAD_LIMIT: usize = 256;
const DEFAULT_PERSIST_INTERVAL_S: f64 = 15.0;
const DEFAULT_SYSTEM_SNAPSHOT_TTL_S: f64 = 1.0;
const DEFAULT_LOG_USAGE_TTL_S: f64 = 15.0;
const DEFAULT_WORKSPACE_USAGE_TTL_S: f64 = 10.0;
const MIN_PREFILL_DURATION_S: f64 = 0.05;
const MONITOR_WRITE_QUEUE_SIZE: usize = 1024;
const MONITOR_WRITE_BATCH_SIZE: usize = 64;

#[derive(Debug, Clone)]
struct MonitorEvent {
    timestamp: f64,
    event_type: String,
    data: Value,
}

impl MonitorEvent {
    fn to_storage(&self) -> Value {
        json!({
            "timestamp": self.timestamp,
            "type": self.event_type,
            "data": self.data,
        })
    }

    fn from_storage(payload: &Value) -> Option<Self> {
        let timestamp = payload
            .get("timestamp")
            .and_then(Value::as_f64)
            .unwrap_or_else(now_ts);
        let event_type = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let data = payload
            .get("data")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));
        Some(Self {
            timestamp,
            event_type,
            data,
        })
    }

    fn to_dict(&self) -> Value {
        let mut data = self.data.clone();
        if let Value::Object(ref mut map) = data {
            if let Some(Value::String(summary)) = map.get("summary") {
                map.insert(
                    "summary".to_string(),
                    Value::String(localize_summary(summary)),
                );
            }
        }
        json!({
            "timestamp": format_ts(self.timestamp),
            "type": self.event_type,
            "data": data,
        })
    }
}

enum MonitorWriteTask {
    Upsert(Value),
}

struct MonitorWriteQueue {
    sender: SyncSender<MonitorWriteTask>,
    dropped: AtomicU64,
}

impl MonitorWriteQueue {
    fn new(storage: Arc<dyn StorageBackend>) -> Self {
        let (sender, receiver) = mpsc::sync_channel(MONITOR_WRITE_QUEUE_SIZE);
        thread::Builder::new()
            .name("wunder-monitor-writer".to_string())
            .spawn(move || {
                while let Ok(task) = receiver.recv() {
                    let mut batch = Vec::with_capacity(MONITOR_WRITE_BATCH_SIZE);
                    batch.push(task);
                    while batch.len() < MONITOR_WRITE_BATCH_SIZE {
                        match receiver.try_recv() {
                            Ok(task) => batch.push(task),
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => break,
                        }
                    }
                    for task in batch {
                        if let Err(err) = Self::apply_write(&storage, task) {
                            error!("monitor storage write failed: {err}");
                        }
                    }
                }
            })
            .ok();
        Self {
            sender,
            dropped: AtomicU64::new(0),
        }
    }

    fn enqueue(&self, task: MonitorWriteTask) {
        match self.sender.try_send(task) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                let dropped = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                if dropped == 1 || dropped % 1000 == 0 {
                    warn!("monitor write queue full, dropped {dropped} records");
                }
            }
        }
    }

    fn apply_write(
        storage: &Arc<dyn StorageBackend>,
        task: MonitorWriteTask,
    ) -> anyhow::Result<()> {
        match task {
            MonitorWriteTask::Upsert(payload) => storage.upsert_monitor_record(&payload),
        }
    }
}

#[derive(Debug, Default, Clone)]
struct LlmRoundMetrics {
    start_ts: Option<f64>,
    first_output_ts: Option<f64>,
    last_output_ts: Option<f64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    prefill_duration_s: Option<f64>,
    decode_duration_s: Option<f64>,
}

#[derive(Debug, Clone)]
struct SessionRecord {
    session_id: String,
    user_id: String,
    question: String,
    status: String,
    stage: String,
    summary: String,
    start_time: f64,
    updated_time: f64,
    cancel_requested: bool,
    ended_time: Option<f64>,
    rounds: i64,
    token_usage: i64,
    events: VecDeque<MonitorEvent>,
    dirty: bool,
    last_persisted: f64,
}

impl SessionRecord {
    fn new(session_id: String, user_id: String, question: String, now: f64) -> Self {
        Self {
            session_id,
            user_id,
            question,
            status: MonitorState::STATUS_RUNNING.to_string(),
            stage: "received".to_string(),
            summary: i18n::t("monitor.summary.received"),
            start_time: now,
            updated_time: now,
            cancel_requested: false,
            ended_time: None,
            rounds: 1,
            token_usage: 0,
            events: VecDeque::new(),
            dirty: true,
            last_persisted: 0.0,
        }
    }

    fn elapsed_s(&self) -> f64 {
        let end = self.ended_time.unwrap_or_else(now_ts);
        (end - self.start_time).max(0.0)
    }

    fn to_summary(&self) -> Value {
        json!({
            "session_id": self.session_id,
            "user_id": self.user_id,
            "question": self.question,
            "status": self.status,
            "stage": self.stage,
            "summary": localize_summary(&self.summary),
            "start_time": format_ts(self.start_time),
            "updated_time": format_ts(self.updated_time),
            "elapsed_s": round2(self.elapsed_s()),
            "cancel_requested": self.cancel_requested,
            "token_usage": self.token_usage,
        })
    }

    fn to_storage(&self) -> Value {
        json!({
            "session_id": self.session_id,
            "user_id": self.user_id,
            "question": self.question,
            "status": self.status,
            "stage": self.stage,
            "summary": self.summary,
            "start_time": self.start_time,
            "updated_time": self.updated_time,
            "ended_time": self.ended_time,
            "cancel_requested": self.cancel_requested,
            "rounds": self.rounds,
            "token_usage": self.token_usage,
            "events": self
                .events
                .iter()
                .map(|event| event.to_storage())
                .collect::<Vec<_>>(),
        })
    }

    fn from_storage(payload: &Value) -> Option<Self> {
        let session_id = payload
            .get("session_id")
            .and_then(Value::as_str)?
            .to_string();
        let user_id = payload
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let question = payload
            .get("question")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let status = payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("finished")
            .to_string();
        let stage = payload
            .get("stage")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let summary = payload
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let start_time = payload
            .get("start_time")
            .and_then(Value::as_f64)
            .unwrap_or_else(now_ts);
        let updated_time = payload
            .get("updated_time")
            .and_then(Value::as_f64)
            .unwrap_or_else(now_ts);
        let ended_time = payload.get("ended_time").and_then(Value::as_f64);
        let cancel_requested = payload
            .get("cancel_requested")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let rounds = payload.get("rounds").and_then(Value::as_i64).unwrap_or(1);
        let token_usage = payload
            .get("token_usage")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let mut events = VecDeque::new();
        if let Some(Value::Array(items)) = payload.get("events") {
            for item in items {
                if let Some(event) = MonitorEvent::from_storage(item) {
                    events.push_back(event);
                }
            }
        }
        Some(Self {
            session_id,
            user_id,
            question,
            status,
            stage,
            summary,
            start_time,
            updated_time,
            cancel_requested,
            ended_time,
            rounds,
            token_usage,
            events,
            dirty: false,
            last_persisted: updated_time,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemSnapshot {
    pub cpu_percent: f32,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_available: u64,
    pub process_rss: u64,
    pub process_cpu_percent: f32,
    pub load_avg_1: f64,
    pub load_avg_5: f64,
    pub load_avg_15: f64,
    pub disk_total: u64,
    pub disk_used: u64,
    pub disk_free: u64,
    pub disk_percent: f32,
    pub log_used: u64,
    pub workspace_used: u64,
    pub uptime_s: u64,
}

impl Default for SystemSnapshot {
    fn default() -> Self {
        Self {
            cpu_percent: 0.0,
            memory_total: 0,
            memory_used: 0,
            memory_available: 0,
            process_rss: 0,
            process_cpu_percent: 0.0,
            load_avg_1: 0.0,
            load_avg_5: 0.0,
            load_avg_15: 0.0,
            disk_total: 0,
            disk_used: 0,
            disk_free: 0,
            disk_percent: 0.0,
            log_used: 0,
            workspace_used: 0,
            uptime_s: 0,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct UsageCache {
    value: u64,
    updated_ts: f64,
}

pub struct MonitorState {
    sessions: Mutex<HashMap<String, SessionRecord>>,
    forced_cancelled: Mutex<HashSet<String>>,
    storage: Arc<dyn StorageBackend>,
    write_queue: MonitorWriteQueue,
    system: Mutex<System>,
    disks: Mutex<Disks>,
    system_snapshot_cache: Mutex<Option<(SystemSnapshot, f64)>>,
    system_snapshot_ttl_s: f64,
    workspace_root: PathBuf,
    log_usage_cache: Mutex<UsageCache>,
    workspace_usage_cache: Mutex<UsageCache>,
    log_usage_ttl_s: f64,
    workspace_usage_ttl_s: f64,
    event_limit: Option<usize>,
    payload_limit: Option<usize>,
    persist_interval_s: f64,
    drop_event_types: HashSet<String>,
    history_dir: PathBuf,
    history_ready: AtomicBool,
    history_loading: AtomicBool,
    history_lock: Mutex<()>,
    app_start_ts: Mutex<f64>,
    sandbox_config: SandboxConfig,
}

impl MonitorState {
    pub const STATUS_RUNNING: &'static str = "running";
    pub const STATUS_FINISHED: &'static str = "finished";
    pub const STATUS_ERROR: &'static str = "error";
    pub const STATUS_CANCELLED: &'static str = "cancelled";
    pub const STATUS_CANCELLING: &'static str = "cancelling";

    pub fn new(
        storage: Arc<dyn StorageBackend>,
        observability: ObservabilityConfig,
        sandbox: SandboxConfig,
        workspace_root: String,
    ) -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        let mut disks = Disks::new_with_refreshed_list();
        disks.refresh();
        let event_limit = resolve_event_limit(observability.monitor_event_limit);
        let payload_limit = resolve_payload_limit(observability.monitor_payload_max_chars);
        let persist_interval_s = DEFAULT_PERSIST_INTERVAL_S;
        let drop_event_types = observability
            .monitor_drop_event_types
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect::<HashSet<_>>();
        let history_dir = PathBuf::from("data/historys/monitor");
        let workspace_root = PathBuf::from(workspace_root);
        let _ = storage.ensure_initialized();
        let write_queue = MonitorWriteQueue::new(storage.clone());
        Self {
            sessions: Mutex::new(HashMap::new()),
            forced_cancelled: Mutex::new(HashSet::new()),
            storage,
            write_queue,
            system: Mutex::new(system),
            disks: Mutex::new(disks),
            system_snapshot_cache: Mutex::new(None),
            system_snapshot_ttl_s: DEFAULT_SYSTEM_SNAPSHOT_TTL_S,
            workspace_root,
            log_usage_cache: Mutex::new(UsageCache::default()),
            workspace_usage_cache: Mutex::new(UsageCache::default()),
            log_usage_ttl_s: DEFAULT_LOG_USAGE_TTL_S,
            workspace_usage_ttl_s: DEFAULT_WORKSPACE_USAGE_TTL_S,
            event_limit,
            payload_limit,
            persist_interval_s,
            drop_event_types,
            history_dir,
            history_ready: AtomicBool::new(false),
            history_loading: AtomicBool::new(false),
            history_lock: Mutex::new(()),
            app_start_ts: Mutex::new(now_ts()),
            sandbox_config: sandbox,
        }
    }

    pub fn warm_history(self: &Arc<Self>, background: bool) -> bool {
        self.run_guarded(
            "monitor.warm_history",
            || false,
            || {
                if self.history_ready.load(Ordering::SeqCst) {
                    return true;
                }
                let _guard = self.history_lock.lock();
                if self.history_ready.load(Ordering::SeqCst) {
                    return true;
                }
                if self.history_loading.swap(true, Ordering::SeqCst) {
                    return false;
                }
                let this = Arc::clone(self);
                if background {
                    thread::spawn(move || {
                        this.run_guarded(
                            "monitor.load_history_background",
                            || (),
                            || {
                                this.load_history();
                            },
                        );
                        this.history_loading.store(false, Ordering::SeqCst);
                        this.history_ready.store(true, Ordering::SeqCst);
                    });
                    return false;
                }
                this.run_guarded(
                    "monitor.load_history",
                    || (),
                    || {
                        this.load_history();
                    },
                );
                this.history_loading.store(false, Ordering::SeqCst);
                this.history_ready.store(true, Ordering::SeqCst);
                true
            },
        )
    }

    pub fn register(&self, session_id: &str, user_id: &str, question: &str) {
        self.run_guarded(
            "monitor.register",
            || (),
            || {
                let now = now_ts();
                let mut sessions = self.sessions.lock();
                let to_persist =
                    self.register_locked(&mut sessions, session_id, user_id, question, now, false);
                drop(sessions);
                if let Some(record) = to_persist {
                    self.save_record(&record);
                }
            },
        );
    }

    pub fn record_event(&self, session_id: &str, event_type: &str, data: &Value) {
        self.run_guarded(
            "monitor.record_event",
            || (),
            || {
                let now = now_ts();
                let to_persist = {
                    let mut sessions = self.sessions.lock();
                    let Some(record) = sessions.get_mut(session_id) else {
                        return;
                    };
                    record.updated_time = now;
                    if event_type == "token_usage" {
                        if let Some(total) = data.get("total_tokens").and_then(Value::as_i64) {
                            record.token_usage = total;
                        }
                    }
                    if event_type == "progress" {
                        if let Some(stage) = data.get("stage").and_then(Value::as_str) {
                            record.stage = stage.to_string();
                        }
                        if let Some(summary) = data.get("summary").and_then(Value::as_str) {
                            record.summary = summary.to_string();
                        }
                    } else if event_type == "tool_call" {
                        record.stage = "tool_call".to_string();
                        let tool = data.get("tool").and_then(Value::as_str).unwrap_or("");
                        record.summary = i18n::t_with_params(
                            "monitor.summary.tool_call",
                            &HashMap::from([("tool".to_string(), tool.to_string())]),
                        );
                    } else if event_type == "plan_update" {
                        record.stage = "plan_update".to_string();
                        record.summary = i18n::t("monitor.summary.plan_update");
                    } else if event_type == "llm_request" {
                        record.stage = "llm_request".to_string();
                        record.summary = i18n::t("monitor.summary.model_call");
                    } else if event_type == "final" {
                        record.stage = "final".to_string();
                        record.summary = i18n::t("monitor.summary.finished");
                    } else if event_type == "error" {
                        record.stage = "error".to_string();
                        record.summary = data
                            .get("message")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| i18n::t("monitor.summary.exception"));
                    }
                    self.append_event(record, event_type, data, now);
                    record.dirty = true;
                    self.maybe_persist_record(record, now, false)
                };
                if let Some(record) = to_persist {
                    self.save_record(&record);
                }
            },
        );
    }

    pub fn mark_finished(&self, session_id: &str) {
        self.mark_status(session_id, Self::STATUS_FINISHED, None);
    }

    pub fn mark_error(&self, session_id: &str, message: &str) {
        self.mark_status(session_id, Self::STATUS_ERROR, Some(message));
    }

    pub fn mark_cancelled(&self, session_id: &str) {
        self.mark_status(
            session_id,
            Self::STATUS_CANCELLED,
            Some(&i18n::t("monitor.summary.cancelled")),
        );
    }
    pub fn cancel(&self, session_id: &str) -> bool {
        self.run_guarded(
            "monitor.cancel",
            || false,
            || {
                let to_persist = {
                    let mut sessions = self.sessions.lock();
                    let Some(record) = sessions.get_mut(session_id) else {
                        return false;
                    };
                    if record.status != Self::STATUS_RUNNING
                        && record.status != Self::STATUS_CANCELLING
                    {
                        return false;
                    }
                    record.cancel_requested = true;
                    record.status = Self::STATUS_CANCELLING.to_string();
                    record.updated_time = now_ts();
                    let updated_time = record.updated_time;
                    self.append_event(
                        record,
                        "cancel",
                        &json!({ "summary": i18n::t("monitor.summary.cancel_requested") }),
                        updated_time,
                    );
                    record.dirty = true;
                    self.maybe_persist_record(record, updated_time, true)
                };
                if let Some(record) = to_persist {
                    self.save_record(&record);
                }
                true
            },
        )
    }

    pub fn delete_session(&self, session_id: &str) -> bool {
        self.run_guarded(
            "monitor.delete_session",
            || false,
            || {
                let mut sessions = self.sessions.lock();
                if let Some(record) = sessions.get(session_id) {
                    if record.status == Self::STATUS_RUNNING
                        || record.status == Self::STATUS_CANCELLING
                    {
                        return false;
                    }
                } else {
                    return false;
                }
                sessions.remove(session_id);
                let _ = self.storage.delete_monitor_record(session_id);
                true
            },
        )
    }

    pub fn purge_user_sessions(&self, user_id: &str) -> HashMap<String, i64> {
        self.run_guarded(
            "monitor.purge_user_sessions",
            || {
                HashMap::from([
                    ("cancelled".to_string(), 0),
                    ("deleted".to_string(), 0),
                    ("deleted_storage".to_string(), 0),
                ])
            },
            || {
                let cleaned = user_id.trim();
                if cleaned.is_empty() {
                    return HashMap::from([
                        ("cancelled".to_string(), 0),
                        ("deleted".to_string(), 0),
                        ("deleted_storage".to_string(), 0),
                    ]);
                }
                let mut cancelled = 0;
                let mut session_ids = Vec::new();
                let mut forced = Vec::new();
                let mut sessions = self.sessions.lock();
                for (session_id, record) in sessions.iter_mut() {
                    if record.user_id != cleaned {
                        continue;
                    }
                    session_ids.push(session_id.clone());
                    if record.status == Self::STATUS_RUNNING
                        || record.status == Self::STATUS_CANCELLING
                    {
                        record.cancel_requested = true;
                        record.status = Self::STATUS_CANCELLING.to_string();
                        record.updated_time = now_ts();
                        let updated_time = record.updated_time;
                        self.append_event(
                            record,
                            "cancel",
                            &json!({ "summary": i18n::t("monitor.summary.user_deleted_cancel") }),
                            updated_time,
                        );
                        cancelled += 1;
                        forced.push(session_id.clone());
                    }
                }
                for session_id in &session_ids {
                    sessions.remove(session_id);
                }
                drop(sessions);
                if !forced.is_empty() {
                    let mut forced_guard = self.forced_cancelled.lock();
                    for session_id in forced {
                        forced_guard.insert(session_id);
                    }
                }
                let deleted_storage = self
                    .storage
                    .delete_monitor_records_by_user(cleaned)
                    .unwrap_or(0);
                HashMap::from([
                    ("cancelled".to_string(), cancelled),
                    ("deleted".to_string(), session_ids.len() as i64),
                    ("deleted_storage".to_string(), deleted_storage),
                ])
            },
        )
    }

    pub fn is_cancelled(&self, session_id: &str) -> bool {
        self.run_guarded(
            "monitor.is_cancelled",
            || false,
            || {
                {
                    let forced = self.forced_cancelled.lock();
                    if forced.contains(session_id) {
                        return true;
                    }
                }
                let sessions = self.sessions.lock();
                sessions
                    .get(session_id)
                    .map(|record| record.cancel_requested)
                    .unwrap_or(false)
            },
        )
    }

    pub fn list_sessions(&self, active_only: bool) -> Vec<Value> {
        self.run_guarded("monitor.list_sessions", Vec::new, || {
            let sessions = self.sessions.lock();
            sessions
                .values()
                .filter(|record| {
                    if !active_only {
                        return true;
                    }
                    record.status == Self::STATUS_RUNNING
                        || record.status == Self::STATUS_CANCELLING
                })
                .map(|record| {
                    let mut summary = record.to_summary();
                    if let Value::Object(ref mut map) = summary {
                        for (key, value) in build_llm_speed_summary(&record.events) {
                            map.insert(key, value);
                        }
                    }
                    summary
                })
                .collect()
        })
    }

    pub fn get_detail(&self, session_id: &str) -> Option<Value> {
        self.run_guarded(
            "monitor.get_detail",
            || None,
            || {
                let sessions = self.sessions.lock();
                let record = sessions.get(session_id)?;
                let events = record
                    .events
                    .iter()
                    .map(|event| event.to_dict())
                    .collect::<Vec<_>>();
                let mut session = record.to_summary();
                if let Value::Object(ref mut map) = session {
                    for (key, value) in build_llm_speed_summary(&record.events) {
                        map.insert(key, value);
                    }
                }
                Some(json!({
                    "session": session,
                    "events": events,
                }))
            },
        )
    }

    pub fn get_record(&self, session_id: &str) -> Option<Value> {
        self.run_guarded(
            "monitor.get_record",
            || None,
            || {
                let cleaned = session_id.trim();
                if cleaned.is_empty() {
                    return None;
                }
                if let Some(record) = self.sessions.lock().get(cleaned) {
                    return Some(record.to_storage());
                }
                self.storage.get_monitor_record(cleaned).ok().flatten()
            },
        )
    }

    pub fn list_records(&self) -> Vec<Value> {
        self.run_guarded("monitor.list_records", Vec::new, || {
            let mut map = HashMap::new();
            if let Ok(records) = self.storage.load_monitor_records() {
                for record in records {
                    if let Some(session_id) = record.get("session_id").and_then(Value::as_str) {
                        map.insert(session_id.to_string(), record);
                    }
                }
            }
            let sessions = self.sessions.lock();
            for (session_id, record) in sessions.iter() {
                map.insert(session_id.clone(), record.to_storage());
            }
            map.into_values().collect()
        })
    }

    pub fn get_system_metrics(&self) -> SystemSnapshot {
        self.run_guarded(
            "monitor.get_system_metrics",
            || self.fallback_system_snapshot(),
            || {
                let now = now_ts();
                {
                    let cache = self.system_snapshot_cache.lock();
                    if let Some((snapshot, ts)) = cache.as_ref() {
                        if now - *ts < self.system_snapshot_ttl_s {
                            return snapshot.clone();
                        }
                    }
                }

                let snapshot = self.collect_system_snapshot();
                let mut cache = self.system_snapshot_cache.lock();
                *cache = Some((snapshot.clone(), now));
                snapshot
            },
        )
    }

    fn resolve_log_usage(&self, now: f64) -> u64 {
        {
            let cache = self.log_usage_cache.lock();
            if cache.updated_ts > 0.0 && now - cache.updated_ts < self.log_usage_ttl_s {
                return cache.value;
            }
        }
        let value = self.calc_log_usage();
        let mut cache = self.log_usage_cache.lock();
        cache.value = value;
        cache.updated_ts = now;
        value
    }

    fn resolve_workspace_usage(&self, now: f64) -> u64 {
        {
            let cache = self.workspace_usage_cache.lock();
            if cache.updated_ts > 0.0 && now - cache.updated_ts < self.workspace_usage_ttl_s {
                return cache.value;
            }
        }
        let value = self.calc_workspace_usage();
        let mut cache = self.workspace_usage_cache.lock();
        cache.value = value;
        cache.updated_ts = now;
        value
    }

    fn calc_log_usage(&self) -> u64 {
        match self.storage.get_log_usage() {
            Ok(value) => value,
            Err(error) => {
                warn!("monitor log usage query failed: {error}");
                0
            }
        }
    }

    fn calc_workspace_usage(&self) -> u64 {
        let entries = match fs::read_dir(&self.workspace_root) {
            Ok(entries) => entries,
            Err(_) => return 0,
        };
        let mut total: u64 = 0;
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            total = total.saturating_add(calc_dir_size(entry.path().as_path(), None));
        }
        total
    }

    fn collect_system_snapshot(&self) -> SystemSnapshot {
        let mut system = self.system.lock();
        system.refresh_cpu_usage();
        system.refresh_memory();
        let pid = sysinfo::get_current_pid().ok();
        let refresh_kind = ProcessRefreshKind::new().with_cpu().with_memory();
        if let Some(pid) = pid {
            system.refresh_process_specifics(pid, refresh_kind);
        } else {
            system.refresh_processes_specifics(refresh_kind);
        }
        let cpu_percent = system.global_cpu_info().cpu_usage();
        let total_memory = system.total_memory();
        let used_memory = system.used_memory();
        let available = total_memory.saturating_sub(used_memory);
        let mut process_rss = 0;
        let mut process_cpu = 0.0;
        if let Some(pid) = pid {
            if let Some(process) = system.process(pid) {
                process_rss = process.memory();
                process_cpu = process.cpu_usage();
            }
        }
        let load_avg = System::load_average();
        drop(system);

        let mut disk_total: u64 = 0;
        let mut disk_used: u64 = 0;
        let mut disk_free: u64 = 0;
        let mut disk_percent = 0.0;
        let mut disks = self.disks.lock();
        if disks.list().is_empty() {
            disks.refresh_list();
        }
        disks.refresh();
        for disk in disks.list() {
            disk_total = disk_total.saturating_add(disk.total_space());
            disk_free = disk_free.saturating_add(disk.available_space());
        }
        if disk_total > 0 {
            disk_used = disk_total.saturating_sub(disk_free);
            disk_percent = (disk_used as f64 / disk_total as f64 * 100.0) as f32;
        }

        let now = now_ts();
        let log_used = self.resolve_log_usage(now);
        let workspace_used = self.resolve_workspace_usage(now);
        let uptime_s = {
            let start = *self.app_start_ts.lock();
            (now - start).max(0.0) as u64
        };
        SystemSnapshot {
            cpu_percent,
            memory_total: total_memory,
            memory_used: used_memory,
            memory_available: available,
            process_rss,
            process_cpu_percent: process_cpu,
            load_avg_1: load_avg.one as f64,
            load_avg_5: load_avg.five as f64,
            load_avg_15: load_avg.fifteen as f64,
            disk_total,
            disk_used,
            disk_free,
            disk_percent,
            log_used,
            workspace_used,
            uptime_s,
        }
    }

    pub fn get_service_metrics(
        &self,
        recent_window_s: Option<f64>,
        current_ts: Option<f64>,
    ) -> Value {
        self.run_guarded(
            "monitor.get_service_metrics",
            || self.fallback_service_metrics(),
            || {
                let now = current_ts.unwrap_or_else(now_ts);
                let window = recent_window_s.unwrap_or(3600.0).max(1.0);
                let window_start = now - window;
                let sessions = self.sessions.lock();
                let mut total_sessions = 0;
                let mut active_sessions = 0;
                let mut finished_sessions = 0;
                let mut error_sessions = 0;
                let mut cancelled_sessions = 0;
                let mut token_usage_total: i64 = 0;
                let mut elapsed_total = 0.0;
                let mut elapsed_count = 0.0;
                let mut prefill_tokens_total = 0.0;
                let mut prefill_duration_total = 0.0;
                let mut decode_tokens_total = 0.0;
                let mut decode_duration_total = 0.0;
                for record in sessions.values() {
                    let mut record_ts = record.updated_time;
                    if record_ts <= 0.0 {
                        record_ts = record.start_time;
                    }
                    if record_ts < window_start || record_ts > now {
                        continue;
                    }
                    total_sessions += 1;
                    token_usage_total += record.token_usage;
                    if record.status == Self::STATUS_RUNNING
                        || record.status == Self::STATUS_CANCELLING
                    {
                        active_sessions += 1;
                        continue;
                    }
                    if record.status == Self::STATUS_FINISHED {
                        finished_sessions += 1;
                    } else if record.status == Self::STATUS_ERROR {
                        error_sessions += 1;
                    } else if record.status == Self::STATUS_CANCELLED {
                        cancelled_sessions += 1;
                    }
                    let end_ts = record.ended_time.unwrap_or(record.updated_time);
                    let summary = build_llm_speed_summary(&record.events);
                    let prefill_tokens = parse_i64_value(summary.get("prefill_tokens"));
                    let prefill_duration = parse_f64_value(summary.get("prefill_duration_s"));
                    if let (Some(tokens), Some(duration)) = (prefill_tokens, prefill_duration) {
                        if tokens > 0 && duration > 0.0 {
                            prefill_tokens_total += tokens as f64;
                            prefill_duration_total += duration;
                        }
                    }
                    let decode_tokens = parse_i64_value(summary.get("decode_tokens"));
                    let decode_duration = parse_f64_value(summary.get("decode_duration_s"));
                    if let (Some(tokens), Some(duration)) = (decode_tokens, decode_duration) {
                        if tokens > 0 && duration > 0.0 {
                            decode_tokens_total += tokens as f64;
                            decode_duration_total += duration;
                        }
                    }
                    elapsed_total += (end_ts - record.start_time).max(0.0);
                    elapsed_count += 1.0;
                }
                let history_sessions = total_sessions - active_sessions;
                let avg_elapsed = if elapsed_count > 0.0 {
                    round2(elapsed_total / elapsed_count)
                } else {
                    0.0
                };
                let avg_prefill_speed =
                    if prefill_tokens_total > 0.0 && prefill_duration_total > 0.0 {
                        Some(prefill_tokens_total / prefill_duration_total)
                    } else {
                        None
                    };
                let avg_decode_speed = if decode_tokens_total > 0.0 && decode_duration_total > 0.0 {
                    Some(decode_tokens_total / decode_duration_total)
                } else {
                    None
                };
                let avg_token_usage = if history_sessions > 0 {
                    Some(round2(token_usage_total as f64 / history_sessions as f64))
                } else {
                    None
                };
                json!({
                    "active_sessions": active_sessions,
                    "history_sessions": history_sessions,
                    "finished_sessions": finished_sessions,
                    "error_sessions": error_sessions,
                    "cancelled_sessions": cancelled_sessions,
                    "total_sessions": total_sessions,
                    "avg_token_usage": avg_token_usage,
                    "avg_elapsed_s": avg_elapsed,
                    "avg_prefill_speed_tps": avg_prefill_speed,
                    "avg_decode_speed_tps": avg_decode_speed,
                })
            },
        )
    }

    pub fn get_sandbox_metrics(&self, since_time: Option<f64>, until_time: Option<f64>) -> Value {
        self.run_guarded(
            "monitor.get_sandbox_metrics",
            || self.fallback_sandbox_metrics(),
            || {
                let mut call_count = 0;
                let mut session_ids = HashSet::new();
                let sessions = self.sessions.lock();
                for record in sessions.values() {
                    for event in &record.events {
                        if event.event_type != "tool_result" {
                            continue;
                        }
                        if !event
                            .data
                            .get("sandbox")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        {
                            continue;
                        }
                        if let Some(since) = since_time {
                            if event.timestamp < since {
                                continue;
                            }
                        }
                        if let Some(until) = until_time {
                            if event.timestamp > until {
                                continue;
                            }
                        }
                        call_count += 1;
                        session_ids.insert(record.session_id.clone());
                    }
                }
                json!({
                    "mode": self.sandbox_config.mode,
                    "network": self.sandbox_config.network,
                    "readonly_rootfs": self.sandbox_config.readonly_rootfs,
                    "idle_ttl_s": self.sandbox_config.idle_ttl_s,
                    "timeout_s": self.sandbox_config.timeout_s,
                    "endpoint": self.sandbox_config.endpoint,
                    "image": self.sandbox_config.image,
                    "resources": {
                        "cpu": self.sandbox_config.resources.cpu,
                        "memory_mb": self.sandbox_config.resources.memory_mb,
                        "pids": self.sandbox_config.resources.pids,
                    },
                    "recent_calls": call_count,
                    "recent_sessions": session_ids.len(),
                })
            },
        )
    }

    fn run_guarded<T, F, G>(&self, label: &'static str, fallback: G, f: F) -> T
    where
        F: FnOnce() -> T,
        G: FnOnce() -> T,
    {
        match panic::catch_unwind(AssertUnwindSafe(f)) {
            Ok(value) => value,
            Err(payload) => {
                let message = format_panic_payload(payload.as_ref());
                error!("monitor panic in {label}: {message}");
                fallback()
            }
        }
    }

    fn fallback_system_snapshot(&self) -> SystemSnapshot {
        if let Some((snapshot, _)) = self.system_snapshot_cache.lock().as_ref() {
            return snapshot.clone();
        }
        SystemSnapshot::default()
    }

    fn fallback_service_metrics(&self) -> Value {
        json!({
            "active_sessions": 0,
            "history_sessions": 0,
            "finished_sessions": 0,
            "error_sessions": 0,
            "cancelled_sessions": 0,
            "total_sessions": 0,
            "avg_token_usage": Value::Null,
            "avg_elapsed_s": 0.0,
            "avg_prefill_speed_tps": Value::Null,
            "avg_decode_speed_tps": Value::Null,
        })
    }

    fn fallback_sandbox_metrics(&self) -> Value {
        json!({
            "mode": self.sandbox_config.mode,
            "network": self.sandbox_config.network,
            "readonly_rootfs": self.sandbox_config.readonly_rootfs,
            "idle_ttl_s": self.sandbox_config.idle_ttl_s,
            "timeout_s": self.sandbox_config.timeout_s,
            "endpoint": self.sandbox_config.endpoint,
            "image": self.sandbox_config.image,
            "resources": {
                "cpu": self.sandbox_config.resources.cpu,
                "memory_mb": self.sandbox_config.resources.memory_mb,
                "pids": self.sandbox_config.resources.pids,
            },
            "recent_calls": 0,
            "recent_sessions": 0,
        })
    }

    fn load_history(&self) {
        self.migrate_legacy_history();
        let records = self.storage.load_monitor_records().unwrap_or_default();
        let mut rebuilt = HashMap::new();
        for payload in records {
            let Some(mut record) = SessionRecord::from_storage(&payload) else {
                continue;
            };
            if record.status == Self::STATUS_RUNNING || record.status == Self::STATUS_CANCELLING {
                record.status = Self::STATUS_ERROR.to_string();
                record.summary = i18n::t("monitor.summary.restarted");
                record.ended_time = Some(record.updated_time);
                let summary = record.summary.clone();
                let updated_time = record.updated_time;
                self.append_event(
                    &mut record,
                    "restart",
                    &json!({ "summary": summary }),
                    updated_time,
                );
            }
            if record.status == Self::STATUS_FINISHED {
                record.stage = "final".to_string();
                record.summary = i18n::t("monitor.summary.finished");
            } else if record.status == Self::STATUS_ERROR {
                record.stage = "error".to_string();
            } else if record.status == Self::STATUS_CANCELLED {
                record.stage = "cancelled".to_string();
            } else if record.status == Self::STATUS_CANCELLING {
                record.stage = "cancelling".to_string();
            }
            if let Some(limit) = self.event_limit {
                while record.events.len() > limit {
                    record.events.pop_front();
                }
            }
            rebuilt.insert(record.session_id.clone(), record);
        }
        if rebuilt.is_empty() {
            return;
        }
        let mut sessions = self.sessions.lock();
        for (session_id, record) in rebuilt {
            if let Some(current) = sessions.get(&session_id) {
                if current.status == Self::STATUS_RUNNING
                    || current.status == Self::STATUS_CANCELLING
                {
                    continue;
                }
                if current.updated_time >= record.updated_time {
                    continue;
                }
            }
            sessions.insert(session_id, record);
        }
    }

    fn migrate_legacy_history(&self) {
        let migration_key = "monitor_migrated";
        if self
            .storage
            .get_meta(migration_key)
            .ok()
            .flatten()
            .as_deref()
            == Some("1")
        {
            return;
        }
        if !self.history_dir.exists() {
            let _ = self.storage.set_meta(migration_key, "1");
            return;
        }
        if let Ok(entries) = std::fs::read_dir(&self.history_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    continue;
                }
                let content = match std::fs::read_to_string(&path) {
                    Ok(content) => content,
                    Err(_) => continue,
                };
                let Ok(payload) = serde_json::from_str::<Value>(&content) else {
                    continue;
                };
                if payload.is_object() {
                    let _ = self.storage.upsert_monitor_record(&payload);
                }
            }
        }
        let _ = self.storage.set_meta(migration_key, "1");
    }

    fn register_locked(
        &self,
        sessions: &mut HashMap<String, SessionRecord>,
        session_id: &str,
        user_id: &str,
        question: &str,
        now: f64,
        append_received: bool,
    ) -> Option<SessionRecord> {
        if session_id.trim().is_empty() {
            return None;
        }
        {
            let mut forced = self.forced_cancelled.lock();
            forced.remove(session_id);
        }
        if let Some(record) = sessions.get_mut(session_id) {
            record.rounds += 1;
            record.question = question.to_string();
            record.status = Self::STATUS_RUNNING.to_string();
            record.stage = "received".to_string();
            record.summary = i18n::t("monitor.summary.received");
            record.start_time = now;
            record.updated_time = now;
            record.ended_time = None;
            record.cancel_requested = false;
            record.token_usage = 0;
            let summary = record.summary.clone();
            let rounds = record.rounds;
            self.append_event(
                record,
                "round_start",
                &json!({
                    "summary": summary,
                    "round": rounds,
                    "question": question
                }),
                now,
            );
            record.dirty = true;
            return self.maybe_persist_record(record, now, false);
        }
        let mut record = SessionRecord::new(
            session_id.to_string(),
            user_id.to_string(),
            question.to_string(),
            now,
        );
        let event_type = if append_received {
            "received"
        } else {
            "round_start"
        };
        let summary = record.summary.clone();
        let rounds = record.rounds;
        self.append_event(
            &mut record,
            event_type,
            &json!({
                "summary": summary,
                "round": rounds,
                "question": question
            }),
            now,
        );
        record.dirty = true;
        let to_persist = self.maybe_persist_record(&mut record, now, false);
        sessions.insert(session_id.to_string(), record);
        to_persist
    }

    fn mark_status(&self, session_id: &str, status: &str, summary: Option<&str>) {
        self.run_guarded(
            "monitor.mark_status",
            || (),
            || {
                let now = now_ts();
                let to_persist = {
                    let mut sessions = self.sessions.lock();
                    let Some(record) = sessions.get_mut(session_id) else {
                        return;
                    };
                    record.status = status.to_string();
                    record.updated_time = now;
                    record.ended_time = Some(now);
                    match status {
                        Self::STATUS_FINISHED => {
                            record.stage = "final".to_string();
                            record.summary = summary
                                .map(str::to_string)
                                .unwrap_or_else(|| i18n::t("monitor.summary.finished"));
                        }
                        Self::STATUS_ERROR => {
                            record.stage = "error".to_string();
                            if let Some(summary) = summary {
                                record.summary = summary.to_string();
                            }
                        }
                        Self::STATUS_CANCELLED => {
                            record.stage = "cancelled".to_string();
                            if let Some(summary) = summary {
                                record.summary = summary.to_string();
                            }
                        }
                        Self::STATUS_CANCELLING => {
                            record.stage = "cancelling".to_string();
                            if let Some(summary) = summary {
                                record.summary = summary.to_string();
                            }
                        }
                        _ => {
                            if let Some(summary) = summary {
                                record.summary = summary.to_string();
                            }
                        }
                    }
                    let summary = record.summary.clone();
                    self.append_event(record, status, &json!({ "summary": summary }), now);
                    record.dirty = true;
                    self.maybe_persist_record(record, now, true)
                };
                if let Some(record) = to_persist {
                    self.save_record(&record);
                }
            },
        );
    }

    fn append_event(
        &self,
        record: &mut SessionRecord,
        event_type: &str,
        data: &Value,
        timestamp: f64,
    ) {
        if self.drop_event_types.contains(event_type) {
            return;
        }
        let sanitized = self.sanitize_event_data(event_type, data);
        record.events.push_back(MonitorEvent {
            timestamp,
            event_type: event_type.to_string(),
            data: sanitized,
        });
        if let Some(limit) = self.event_limit {
            while record.events.len() > limit {
                record.events.pop_front();
            }
        }
    }

    fn sanitize_event_data(&self, event_type: &str, data: &Value) -> Value {
        if !data.is_object() {
            return data.clone();
        }
        if event_type == "llm_request" {
            return trim_string_fields(data, self.payload_limit);
        }
        if event_type == "llm_output" {
            let mut trimmed = trim_string_fields(data, self.payload_limit);
            if let Value::Object(ref mut map) = trimmed {
                if let Some(Value::String(text)) = map.get("content") {
                    map.insert(
                        "content".to_string(),
                        Value::String(trim_text(text, self.payload_limit)),
                    );
                }
                if let Some(Value::String(text)) = map.get("reasoning") {
                    map.insert(
                        "reasoning".to_string(),
                        Value::String(trim_text(text, self.payload_limit)),
                    );
                }
            }
            return trimmed;
        }
        trim_string_fields(data, self.payload_limit)
    }

    fn maybe_persist_record(
        &self,
        record: &mut SessionRecord,
        now: f64,
        force: bool,
    ) -> Option<SessionRecord> {
        if !record.dirty && !force {
            return None;
        }
        let should_persist = force
            || record.last_persisted <= 0.0
            || now - record.last_persisted >= self.persist_interval_s;
        if !should_persist {
            return None;
        }
        record.dirty = false;
        record.last_persisted = now;
        Some(record.clone())
    }

    fn save_record(&self, record: &SessionRecord) {
        let payload = record.to_storage();
        self.write_queue.enqueue(MonitorWriteTask::Upsert(payload));
    }
}

fn build_llm_speed_summary(events: &VecDeque<MonitorEvent>) -> serde_json::Map<String, Value> {
    let mut rounds: HashMap<i64, LlmRoundMetrics> = HashMap::new();
    let mut first_round: Option<i64> = None;
    let mut latest_round: Option<i64> = None;
    let mut last_round_seen: Option<i64> = None;
    let mut implicit_round: i64 = 0;

    for event in events {
        match event.event_type.as_str() {
            "llm_request" => {
                let round = parse_round(&event.data).unwrap_or_else(|| {
                    implicit_round += 1;
                    implicit_round
                });
                last_round_seen = Some(round);
                if first_round.is_none() {
                    first_round = Some(round);
                }
                let entry = rounds.entry(round).or_default();
                if entry.start_ts.is_none() {
                    entry.start_ts = Some(event.timestamp);
                }
            }
            "llm_output_delta" | "llm_output" => {
                let round = parse_round(&event.data).or(last_round_seen);
                if let Some(round) = round {
                    last_round_seen = Some(round);
                    let entry = rounds.entry(round).or_default();
                    if entry.first_output_ts.is_none() {
                        entry.first_output_ts = Some(event.timestamp);
                    }
                    entry.last_output_ts = Some(event.timestamp);
                    if event.event_type == "llm_output" {
                        let (input_tokens, output_tokens) = parse_usage_tokens(&event.data);
                        if entry.input_tokens.is_none() {
                            entry.input_tokens = input_tokens;
                        }
                        if entry.output_tokens.is_none() {
                            entry.output_tokens = output_tokens;
                        }
                        if entry.prefill_duration_s.is_none() {
                            entry.prefill_duration_s =
                                parse_f64_value(event.data.get("prefill_duration_s"));
                        }
                        if entry.decode_duration_s.is_none() {
                            entry.decode_duration_s =
                                parse_f64_value(event.data.get("decode_duration_s"));
                        }
                        if entry.output_tokens.is_some() {
                            latest_round = Some(round);
                        }
                    }
                }
            }
            "token_usage" => {
                let round = parse_round(&event.data).or(last_round_seen);
                if let Some(round) = round {
                    last_round_seen = Some(round);
                    let entry = rounds.entry(round).or_default();
                    if entry.input_tokens.is_none() {
                        entry.input_tokens = parse_i64_value(event.data.get("input_tokens"));
                    }
                    if entry.output_tokens.is_none() {
                        entry.output_tokens = parse_i64_value(event.data.get("output_tokens"));
                    }
                    if entry.prefill_duration_s.is_none() {
                        entry.prefill_duration_s =
                            parse_f64_value(event.data.get("prefill_duration_s"));
                    }
                    if entry.decode_duration_s.is_none() {
                        entry.decode_duration_s =
                            parse_f64_value(event.data.get("decode_duration_s"));
                    }
                    if entry.output_tokens.is_some() {
                        latest_round = Some(round);
                    }
                }
            }
            _ => {}
        }
    }

    let prefill_metrics = first_round.and_then(|round| rounds.get(&round));
    let prefill_tokens = prefill_metrics.and_then(|metrics| metrics.input_tokens);
    let prefill_duration_s = prefill_metrics
        .and_then(|metrics| metrics.prefill_duration_s)
        .or_else(|| {
            prefill_metrics.and_then(|metrics| {
                let Some(start_ts) = metrics.start_ts else {
                    return None;
                };
                let Some(first_output_ts) = metrics.first_output_ts else {
                    return None;
                };
                Some((first_output_ts - start_ts).max(0.0))
            })
        })
        .map(|value| {
            let duration = value.max(0.0);
            if duration < MIN_PREFILL_DURATION_S {
                MIN_PREFILL_DURATION_S
            } else {
                duration
            }
        });
    let prefill_speed_tps = match (prefill_tokens, prefill_duration_s) {
        (Some(tokens), Some(duration)) if tokens > 0 && duration > 0.0 => {
            Some(tokens as f64 / duration)
        }
        _ => None,
    };

    let decode_round = latest_round.or(first_round);
    let decode_metrics = decode_round.and_then(|round| rounds.get(&round));
    let decode_tokens = decode_metrics.and_then(|metrics| metrics.output_tokens);
    let decode_duration_s = decode_metrics
        .and_then(|metrics| metrics.decode_duration_s)
        .map(|value| value.max(0.0))
        .or_else(|| {
            decode_metrics.and_then(|metrics| {
                let Some(first_output_ts) = metrics.first_output_ts else {
                    return None;
                };
                let Some(last_output_ts) = metrics.last_output_ts else {
                    return None;
                };
                Some((last_output_ts - first_output_ts).max(0.0))
            })
        });
    let decode_speed_tps = match (decode_tokens, decode_duration_s) {
        (Some(tokens), Some(duration)) if tokens > 0 && duration > 0.0 => {
            Some(tokens as f64 / duration)
        }
        _ => None,
    };

    let mut result = serde_json::Map::new();
    result.insert("prefill_tokens".to_string(), json!(prefill_tokens));
    result.insert("prefill_duration_s".to_string(), json!(prefill_duration_s));
    result.insert("prefill_speed_tps".to_string(), json!(prefill_speed_tps));
    result.insert("prefill_speed_lower_bound".to_string(), json!(false));
    result.insert("decode_tokens".to_string(), json!(decode_tokens));
    result.insert("decode_duration_s".to_string(), json!(decode_duration_s));
    result.insert("decode_speed_tps".to_string(), json!(decode_speed_tps));
    result
}

fn parse_i64_value(value: Option<&Value>) -> Option<i64> {
    value
        .and_then(Value::as_i64)
        .or_else(|| value.and_then(Value::as_u64).map(|value| value as i64))
}

fn parse_f64_value(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .or_else(|| value.and_then(Value::as_i64).map(|value| value as f64))
        .or_else(|| value.and_then(Value::as_u64).map(|value| value as f64))
}

fn parse_round(data: &Value) -> Option<i64> {
    parse_i64_value(data.get("round"))
}

fn parse_usage_tokens(data: &Value) -> (Option<i64>, Option<i64>) {
    let Some(usage) = data.get("usage").and_then(Value::as_object) else {
        return (None, None);
    };
    (
        parse_i64_value(usage.get("input_tokens")),
        parse_i64_value(usage.get("output_tokens")),
    )
}

fn format_panic_payload(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic payload".to_string()
}

fn resolve_event_limit(raw: i64) -> Option<usize> {
    if raw == 0 {
        return Some(DEFAULT_EVENT_LIMIT);
    }
    if raw < 0 {
        return None;
    }
    let value = raw as usize;
    if value == 0 {
        None
    } else {
        Some(value)
    }
}

fn resolve_payload_limit(raw: i64) -> Option<usize> {
    if raw == 0 {
        return Some(DEFAULT_PAYLOAD_LIMIT);
    }
    if raw < 0 {
        return None;
    }
    let value = raw as usize;
    if value == 0 {
        None
    } else {
        Some(value.max(MIN_PAYLOAD_LIMIT))
    }
}

fn trim_text(text: &str, limit: Option<usize>) -> String {
    let Some(limit) = limit else {
        return text.to_string();
    };
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...(truncated)", &text[..end])
}

fn trim_string_fields(data: &Value, limit: Option<usize>) -> Value {
    let Value::Object(map) = data else {
        return data.clone();
    };
    let mut output = serde_json::Map::new();
    for (key, value) in map {
        if let Value::String(text) = value {
            output.insert(key.clone(), Value::String(trim_text(text, limit)));
        } else {
            output.insert(key.clone(), value.clone());
        }
    }
    Value::Object(output)
}

fn calc_dir_size(path: &Path, extensions: Option<&[&str]>) -> u64 {
    if !path.exists() {
        return 0;
    }
    let mut total: u64 = 0;
    for entry in WalkDir::new(path).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(extensions) = extensions {
            let ext = entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if !extensions
                .iter()
                .any(|allowed| ext.eq_ignore_ascii_case(allowed))
            {
                continue;
            }
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        total = total.saturating_add(metadata.len());
    }
    total
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

fn format_ts(ts: f64) -> String {
    if ts <= 0.0 {
        return String::new();
    }
    let Some(dt) = DateTime::<Utc>::from_timestamp(ts as i64, 0) else {
        return String::new();
    };
    dt.with_timezone(&Local).to_rfc3339()
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn split_tool_summary_template(template: &str) -> (String, String) {
    if let Some((prefix, suffix)) = template.split_once("{tool}") {
        return (prefix.to_string(), suffix.to_string());
    }
    (template.to_string(), String::new())
}

fn localize_summary(summary: &str) -> String {
    let cleaned = summary.trim();
    if cleaned.is_empty() {
        return String::new();
    }
    let tool_templates = i18n::get_known_prefixes("monitor.summary.tool_call")
        .into_iter()
        .map(|item| split_tool_summary_template(&item))
        .collect::<Vec<_>>();
    for (prefix, suffix) in tool_templates {
        if prefix.is_empty() {
            continue;
        }
        if !cleaned.starts_with(&prefix) {
            continue;
        }
        if !suffix.is_empty() && !cleaned.ends_with(&suffix) {
            continue;
        }
        let mut tool_name = cleaned[prefix.len()..].to_string();
        if !suffix.is_empty() && cleaned.len() >= suffix.len() {
            tool_name = cleaned[prefix.len()..cleaned.len() - suffix.len()].to_string();
        }
        return i18n::t_with_params(
            "monitor.summary.tool_call",
            &HashMap::from([("tool".to_string(), tool_name.trim().to_string())]),
        );
    }
    let summary_keys = [
        "monitor.summary.restarted",
        "monitor.summary.finished",
        "monitor.summary.received",
        "monitor.summary.model_call",
        "monitor.summary.exception",
        "monitor.summary.cancelled",
        "monitor.summary.cancel_requested",
        "monitor.summary.user_deleted_cancel",
    ];
    for key in summary_keys {
        if i18n::get_known_prefixes(key)
            .iter()
            .any(|item| item == cleaned)
        {
            return i18n::t(key);
        }
    }
    cleaned.to_string()
}
