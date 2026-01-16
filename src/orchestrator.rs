// 调度引擎：负责会话锁、LLM 调用、工具执行、历史压缩与 SSE 事件流。
use crate::a2a_store::A2aStore;
use crate::config::{Config, LlmModelConfig};
use crate::config_store::ConfigStore;
use crate::history::HistoryManager;
use crate::i18n;
use crate::llm::{build_llm_client, is_llm_configured, ChatMessage};
use crate::memory::MemoryStore;
use crate::monitor::MonitorState;
use crate::orchestrator_constants::{
    COMPACTION_HISTORY_RATIO, COMPACTION_META_TYPE, COMPACTION_MIN_OBSERVATION_TOKENS,
    COMPACTION_SUMMARY_MAX_OUTPUT, COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS, DEFAULT_LLM_TIMEOUT_S,
    DEFAULT_TOOL_TIMEOUT_S, MIN_TOOL_TIMEOUT_S, OBSERVATION_PREFIX, SESSION_LOCK_HEARTBEAT_S,
    SESSION_LOCK_POLL_INTERVAL_S, SESSION_LOCK_TTL_S, STREAM_EVENT_CLEANUP_INTERVAL_S,
    STREAM_EVENT_FETCH_LIMIT, STREAM_EVENT_POLL_INTERVAL_S, STREAM_EVENT_QUEUE_SIZE,
    STREAM_EVENT_TTL_S,
};
use crate::path_utils::{normalize_path_for_compare, normalize_target_path};
use crate::prompting::{read_prompt_template, PromptComposer};
use crate::sandbox;
use crate::schemas::{AttachmentPayload, StreamEvent, TokenUsage, WunderRequest, WunderResponse};
use crate::skills::{load_skills, SkillRegistry};
use crate::storage::{SessionLockStatus, StorageBackend};
use crate::token_utils::{
    approx_token_count, estimate_message_tokens, estimate_messages_tokens, trim_messages_to_budget,
    trim_text_to_tokens,
};
use crate::tools::{
    collect_available_tool_names, resolve_tool_name, ToolContext, ToolEventEmitter,
};
use crate::user_tools::{UserToolBindings, UserToolManager};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use chrono::{DateTime, TimeZone, Utc};
use futures::{Stream, StreamExt};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, Notify, RwLock};
use tokio::task::JoinHandle;
use tracing::{error, warn};
use uuid::Uuid;

const MEMORY_SUMMARY_PROMPT_PATH: &str = "app/prompts/memory_summary.txt";
const TOOL_TIMEOUT_ERROR: &str = "tool_timeout";

#[derive(Debug, Clone)]
struct PreparedRequest {
    user_id: String,
    question: String,
    session_id: String,
    tool_names: Option<Vec<String>>,
    skip_tool_calls: bool,
    model_name: Option<String>,
    config_overrides: Option<Value>,
    stream: bool,
    attachments: Option<Vec<AttachmentPayload>>,
    language: String,
}

#[derive(Debug, Clone)]
struct MemorySummaryTask {
    task_id: String,
    user_id: String,
    session_id: String,
    queued_time: f64,
    config_overrides: Option<Value>,
    model_name: Option<String>,
    attachments: Option<Vec<AttachmentPayload>>,
    request_messages: Option<Vec<Value>>,
    language: String,
    status: String,
    start_time: f64,
    end_time: f64,
    request_payload: Option<Value>,
    final_answer: String,
    summary_result: String,
    error: String,
}

#[derive(Debug, Default)]
struct OutputTiming {
    first_output_at: Option<Instant>,
    last_output_at: Option<Instant>,
}

impl OutputTiming {
    fn mark_output(&mut self, now: Instant) {
        if self.first_output_at.is_none() {
            self.first_output_at = Some(now);
        }
        self.last_output_at = Some(now);
    }

    fn durations(
        &self,
        request_start: Instant,
        response_end: Instant,
    ) -> (Option<f64>, Option<f64>) {
        let Some(first_output_at) = self.first_output_at else {
            return (None, None);
        };
        let last_output_at = self.last_output_at.unwrap_or(response_end);
        let prefill = first_output_at
            .saturating_duration_since(request_start)
            .as_secs_f64();
        let decode = last_output_at
            .saturating_duration_since(first_output_at)
            .as_secs_f64();
        (Some(prefill), Some(decode))
    }
}

#[derive(Debug)]
pub(crate) struct OrchestratorError {
    code: &'static str,
    message: String,
    detail: Option<Value>,
}

impl OrchestratorError {
    fn new(code: &'static str, message: String, detail: Option<Value>) -> Self {
        Self {
            code,
            message,
            detail,
        }
    }

    fn invalid_request(message: String) -> Self {
        Self::new("INVALID_REQUEST", message, None)
    }

    fn user_busy(message: String) -> Self {
        Self::new("USER_BUSY", message, None)
    }

    fn cancelled(message: String) -> Self {
        Self::new("CANCELLED", message, None)
    }

    fn llm_unavailable(message: String) -> Self {
        Self::new("LLM_UNAVAILABLE", message, None)
    }

    fn internal(message: String) -> Self {
        Self::new("INTERNAL_ERROR", message, None)
    }

    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    pub(crate) fn to_payload(&self) -> Value {
        let mut payload = json!({
            "code": self.code,
            "message": self.message,
        });
        if let Some(detail) = &self.detail {
            if let Value::Object(ref mut map) = payload {
                map.insert("detail".to_string(), detail.clone());
            }
        }
        payload
    }
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for OrchestratorError {}

#[derive(Debug, Clone)]
struct ToolCall {
    name: String,
    arguments: Value,
}

#[derive(Debug, Clone)]
struct ToolResultPayload {
    ok: bool,
    data: Value,
    error: String,
    sandbox: bool,
    timestamp: DateTime<Utc>,
}

impl ToolResultPayload {
    fn from_value(value: Value) -> Self {
        let timestamp = Utc::now();
        if let Value::Object(map) = &value {
            if map.get("ok").and_then(Value::as_bool).is_some() && map.contains_key("data") {
                let ok = map.get("ok").and_then(Value::as_bool).unwrap_or(true);
                let data = map.get("data").cloned().unwrap_or_else(|| json!({}));
                let error = map
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let sandbox = map.get("sandbox").and_then(Value::as_bool).unwrap_or(false);
                return Self {
                    ok,
                    data,
                    error,
                    sandbox,
                    timestamp,
                };
            }
        }

        let data = if value.is_object() {
            value
        } else {
            json!({ "result": value })
        };
        Self {
            ok: true,
            data,
            error: String::new(),
            sandbox: false,
            timestamp,
        }
    }

    fn error(message: String, data: Value) -> Self {
        Self {
            ok: false,
            data: if data.is_object() {
                data
            } else {
                json!({ "detail": data })
            },
            error: message,
            sandbox: false,
            timestamp: Utc::now(),
        }
    }

    fn to_observation_payload(&self, tool_name: &str) -> Value {
        let mut payload = json!({
            "tool": tool_name,
            "ok": self.ok,
            "data": self.data,
            "timestamp": self.timestamp.to_rfc3339(),
        });
        if !self.error.trim().is_empty() {
            if let Value::Object(ref mut map) = payload {
                map.insert("error".to_string(), Value::String(self.error.clone()));
            }
        }
        if self.sandbox {
            if let Value::Object(ref mut map) = payload {
                map.insert("sandbox".to_string(), Value::Bool(true));
            }
        }
        payload
    }

    fn to_event_payload(&self, tool_name: &str) -> Value {
        self.to_observation_payload(tool_name)
    }
}

enum StreamSignal {
    Event(StreamEvent),
    Done,
}

#[derive(Clone)]
pub struct Orchestrator {
    config_store: ConfigStore,
    workspace: Arc<WorkspaceManager>,
    monitor: Arc<MonitorState>,
    a2a_store: Arc<A2aStore>,
    skills: Arc<RwLock<SkillRegistry>>,
    user_tool_manager: Arc<UserToolManager>,
    prompt_composer: Arc<PromptComposer>,
    storage: Arc<dyn StorageBackend>,
    memory_store: Arc<MemoryStore>,
    memory_queue: Arc<MemoryQueue>,
    http: reqwest::Client,
}

struct MemoryQueue {
    state: Mutex<MemoryQueueState>,
    notify: Notify,
}

struct MemoryQueueState {
    queue: std::collections::BinaryHeap<MemoryQueueItem>,
    seq: u64,
    active: Option<MemorySummaryTask>,
    history: VecDeque<MemorySummaryTask>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct MemoryQueueItem {
    queued_time: f64,
    seq: u64,
    task: MemorySummaryTask,
}

impl Ord for MemoryQueueItem {
    fn cmp(&self, other: &Self) -> Ordering {
        let time_cmp = other
            .queued_time
            .partial_cmp(&self.queued_time)
            .unwrap_or(Ordering::Equal);
        if time_cmp != Ordering::Equal {
            return time_cmp;
        }
        other.seq.cmp(&self.seq)
    }
}

impl PartialOrd for MemoryQueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for MemoryQueueItem {
    fn eq(&self, other: &Self) -> bool {
        self.queued_time == other.queued_time && self.seq == other.seq
    }
}

impl Eq for MemoryQueueItem {}

impl MemoryQueue {
    fn new() -> Self {
        Self {
            state: Mutex::new(MemoryQueueState {
                queue: std::collections::BinaryHeap::new(),
                seq: 0,
                active: None,
                history: VecDeque::with_capacity(100),
                worker: None,
            }),
            notify: Notify::new(),
        }
    }
}

impl Orchestrator {
    pub fn new(
        config_store: ConfigStore,
        _config: Config,
        workspace: Arc<WorkspaceManager>,
        monitor: Arc<MonitorState>,
        a2a_store: Arc<A2aStore>,
        skills: Arc<RwLock<SkillRegistry>>,
        user_tool_manager: Arc<UserToolManager>,
        storage: Arc<dyn StorageBackend>,
    ) -> Self {
        let memory_store = Arc::new(MemoryStore::new(storage.clone()));
        Self {
            config_store,
            workspace,
            monitor,
            a2a_store,
            skills,
            user_tool_manager,
            prompt_composer: Arc::new(PromptComposer::new(60.0, 256)),
            storage,
            memory_store,
            memory_queue: Arc::new(MemoryQueue::new()),
            http: reqwest::Client::new(),
        }
    }
}

#[derive(Clone)]
struct EventEmitter {
    session_id: String,
    user_id: String,
    queue: Option<mpsc::Sender<StreamSignal>>,
    storage: Option<Arc<dyn StorageBackend>>,
    monitor: Arc<MonitorState>,
    closed: Arc<AtomicBool>,
    next_event_id: Arc<AtomicI64>,
    last_cleanup_ts: Arc<AtomicU64>,
}

impl EventEmitter {
    fn new(
        session_id: String,
        user_id: String,
        queue: Option<mpsc::Sender<StreamSignal>>,
        storage: Option<Arc<dyn StorageBackend>>,
        monitor: Arc<MonitorState>,
    ) -> Self {
        Self {
            session_id,
            user_id,
            queue,
            storage,
            monitor,
            closed: Arc::new(AtomicBool::new(false)),
            next_event_id: Arc::new(AtomicI64::new(1)),
            last_cleanup_ts: Arc::new(AtomicU64::new(0)),
        }
    }

    fn close(&self) {
        self.closed.store(true, AtomicOrdering::SeqCst);
    }

    async fn finish(&self) {
        let Some(queue) = &self.queue else {
            return;
        };
        if self.closed.load(AtomicOrdering::SeqCst) {
            return;
        }
        let _ = queue.try_send(StreamSignal::Done);
    }

    async fn emit(&self, event_type: &str, data: Value) -> StreamEvent {
        let timestamp = Utc::now();
        let event_id = self.next_event_id.fetch_add(1, AtomicOrdering::SeqCst);
        self.monitor
            .record_event(&self.session_id, event_type, &data);
        let payload = enrich_event_payload(data, Some(&self.session_id), timestamp);
        let event = StreamEvent {
            event: event_type.to_string(),
            data: payload,
            id: Some(event_id.to_string()),
            timestamp: Some(timestamp),
        };
        self.enqueue_event(&event).await;
        event
    }

    async fn enqueue_event(&self, event: &StreamEvent) {
        if self.closed.load(AtomicOrdering::SeqCst) {
            return;
        }
        if let Some(queue) = &self.queue {
            match queue.try_send(StreamSignal::Event(event.clone())) {
                Ok(_) => return,
                Err(mpsc::error::TrySendError::Closed(_)) => return,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    self.record_overflow(event).await;
                    return;
                }
            }
        }
    }

    async fn record_overflow(&self, event: &StreamEvent) {
        let Some(storage) = &self.storage else {
            return;
        };
        let Some(event_id) = event.id.as_ref().and_then(|text| text.parse::<i64>().ok()) else {
            return;
        };
        let payload = json!({
            "event": event.event,
            "data": event.data,
            "timestamp": event.timestamp.map(|ts| ts.to_rfc3339()),
        });
        let session_id = self.session_id.clone();
        let user_id = self.user_id.clone();
        let storage = storage.clone();
        let cleanup_cutoff = self.cleanup_cutoff();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || {
                let _ = storage.append_stream_event(&session_id, &user_id, event_id, &payload);
                if let Some(cutoff) = cleanup_cutoff {
                    let _ = storage.delete_stream_events_before(cutoff);
                }
            });
        } else {
            let _ = storage.append_stream_event(&session_id, &user_id, event_id, &payload);
            if let Some(cutoff) = cleanup_cutoff {
                let _ = storage.delete_stream_events_before(cutoff);
            }
        }
    }

    fn cleanup_cutoff(&self) -> Option<f64> {
        let now = Utc::now().timestamp_millis() as u64;
        let last = self.last_cleanup_ts.load(AtomicOrdering::SeqCst);
        let interval_ms = (STREAM_EVENT_CLEANUP_INTERVAL_S * 1000.0) as u64;
        if last > 0 && now.saturating_sub(last) < interval_ms {
            return None;
        }
        self.last_cleanup_ts.store(now, AtomicOrdering::SeqCst);
        let cutoff = Utc::now().timestamp_millis() as f64 / 1000.0 - STREAM_EVENT_TTL_S;
        Some(cutoff)
    }
}

#[derive(Clone)]
struct RequestLimiter {
    storage: Arc<dyn StorageBackend>,
    max_active: i64,
    poll_interval_s: f64,
    lock_ttl_s: f64,
}

impl RequestLimiter {
    fn new(storage: Arc<dyn StorageBackend>, max_active: usize) -> Self {
        Self {
            storage,
            max_active: max_active.max(1) as i64,
            poll_interval_s: SESSION_LOCK_POLL_INTERVAL_S,
            lock_ttl_s: SESSION_LOCK_TTL_S,
        }
    }

    async fn acquire(&self, session_id: &str, user_id: &str) -> Result<bool> {
        if session_id.trim().is_empty() || user_id.trim().is_empty() {
            return Ok(false);
        }
        loop {
            let storage = self.storage.clone();
            let session_id = session_id.to_string();
            let user_id = user_id.to_string();
            let ttl = self.lock_ttl_s;
            let max_active = self.max_active;
            let status = tokio::task::spawn_blocking(move || {
                storage.try_acquire_session_lock(&session_id, &user_id, ttl, max_active)
            })
            .await
            .map_err(|err| anyhow!("session lock join error: {err}"))??;
            match status {
                SessionLockStatus::Acquired => return Ok(true),
                SessionLockStatus::UserBusy => return Ok(false),
                SessionLockStatus::SystemBusy => {
                    tokio::time::sleep(std::time::Duration::from_secs_f64(self.poll_interval_s))
                        .await;
                }
            }
        }
    }

    async fn touch(&self, session_id: &str) {
        let storage = self.storage.clone();
        let session_id = session_id.to_string();
        let ttl = self.lock_ttl_s;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || {
                let _ = storage.touch_session_lock(&session_id, ttl);
            });
        } else {
            let _ = storage.touch_session_lock(&session_id, ttl);
        }
    }

    async fn release(&self, session_id: &str) {
        let storage = self.storage.clone();
        let session_id = session_id.to_string();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || {
                let _ = storage.release_session_lock(&session_id);
            });
        } else {
            let _ = storage.release_session_lock(&session_id);
        }
    }
}
impl Orchestrator {
    pub async fn run(&self, request: WunderRequest) -> Result<WunderResponse> {
        let prepared = self.prepare_request(request)?;
        let language = prepared.language.clone();
        let emitter = EventEmitter::new(
            prepared.session_id.clone(),
            prepared.user_id.clone(),
            None,
            None,
            self.monitor.clone(),
        );
        let response = i18n::with_language(language, async {
            self.execute_request(prepared, emitter).await
        })
        .await?;
        Ok(response)
    }

    pub async fn stream(
        &self,
        request: WunderRequest,
    ) -> Result<impl Stream<Item = Result<StreamEvent, std::convert::Infallible>>> {
        let prepared = self.prepare_request(request)?;
        let language = prepared.language.clone();
        let (queue_tx, queue_rx) = mpsc::channel::<StreamSignal>(STREAM_EVENT_QUEUE_SIZE);
        let (event_tx, event_rx) = mpsc::channel::<StreamEvent>(STREAM_EVENT_QUEUE_SIZE);
        let emitter = EventEmitter::new(
            prepared.session_id.clone(),
            prepared.user_id.clone(),
            Some(queue_tx),
            Some(self.storage.clone()),
            self.monitor.clone(),
        );
        let runner = {
            let orchestrator = self.clone();
            let emitter = emitter.clone();
            let prepared = prepared.clone();
            let language = language.clone();
            tokio::spawn(async move {
                let result = i18n::with_language(language, async {
                    orchestrator.execute_request(prepared, emitter).await
                })
                .await;
                if let Err(err) = result {
                    warn!("流式请求执行失败: {}", err);
                }
            })
        };
        self.spawn_stream_pump(
            prepared.session_id.clone(),
            queue_rx,
            event_tx,
            emitter,
            runner,
        );
        let stream = tokio_stream::wrappers::ReceiverStream::new(event_rx)
            .map(|event| Ok::<_, std::convert::Infallible>(event));
        Ok(stream)
    }

    pub async fn build_system_prompt(
        &self,
        config: &Config,
        tool_names: &[String],
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
        user_id: &str,
        config_overrides: Option<&Value>,
    ) -> String {
        let allowed_tool_names =
            self.resolve_allowed_tool_names(config, tool_names, skills, user_tool_bindings);
        let prompt = self
            .build_system_prompt_with_allowed(
                config,
                config_overrides,
                &allowed_tool_names,
                skills,
                user_tool_bindings,
                user_id,
            )
            .await;
        self.append_memory_prompt(user_id, prompt).await
    }

    pub async fn get_memory_queue_status(&self) -> Value {
        let now = now_ts();
        let (active, queued, history_fallback) = {
            let state = self.memory_queue.state.lock().await;
            let active = state.active.clone();
            let queued = state
                .queue
                .iter()
                .map(|item| item.task.clone())
                .collect::<Vec<_>>();
            let history = state.history.iter().cloned().collect::<Vec<_>>();
            (active, queued, history)
        };

        let mut active_items = Vec::new();
        if let Some(task) = active {
            active_items.push(self.format_memory_task(&task, now));
        }
        let mut queued_sorted = queued;
        queued_sorted.sort_by(|a, b| {
            let time_cmp = a
                .queued_time
                .partial_cmp(&b.queued_time)
                .unwrap_or(Ordering::Equal);
            if time_cmp != Ordering::Equal {
                return time_cmp;
            }
            a.task_id.cmp(&b.task_id)
        });
        for task in queued_sorted {
            active_items.push(self.format_memory_task(&task, now));
        }

        let storage_history = self
            .memory_store
            .list_task_logs_async(None)
            .await
            .into_iter()
            .map(|payload| Value::Object(payload.into_iter().collect::<Map<String, Value>>()))
            .collect::<Vec<_>>();
        let history = if storage_history.is_empty() {
            history_fallback
                .into_iter()
                .map(|task| self.format_memory_task(&task, now))
                .collect::<Vec<_>>()
        } else {
            storage_history
        };

        json!({
            "active": active_items,
            "history": history,
        })
    }

    pub async fn get_memory_queue_detail(&self, task_id: &str) -> Option<Value> {
        let cleaned = task_id.trim();
        if cleaned.is_empty() {
            return None;
        }
        if let Some(task) = self.find_memory_task(cleaned).await {
            let mut detail = self.format_memory_task(&task, now_ts());
            let mut request_payload = task.request_payload.clone();
            if request_payload.is_none() {
                if let Ok(payload) = self.build_memory_summary_request_payload(&task).await {
                    request_payload = Some(payload);
                }
            }
            if let Value::Object(ref mut map) = detail {
                if let Some(payload) = request_payload {
                    map.insert("request".to_string(), payload);
                }
                map.insert("result".to_string(), json!(task.summary_result));
                if !task.error.is_empty() {
                    map.insert("error".to_string(), json!(task.error));
                }
            }
            return Some(detail);
        }
        self.memory_store
            .get_task_log_async(cleaned)
            .await
            .map(|payload| Value::Object(payload.into_iter().collect::<Map<String, Value>>()))
    }
}

impl Orchestrator {
    fn prepare_request(
        &self,
        request: WunderRequest,
    ) -> Result<PreparedRequest, OrchestratorError> {
        let user_id = request.user_id.trim().to_string();
        if user_id.is_empty() {
            return Err(OrchestratorError::invalid_request(i18n::t(
                "error.user_id_required",
            )));
        }
        if let Err(err) = self.workspace.ensure_user_root(&user_id) {
            return Err(OrchestratorError::internal(format!(
                "failed to prepare workspace: {err}"
            )));
        }
        self.workspace.touch_user_session(&user_id);
        let question = request.question.trim().to_string();
        if question.is_empty() {
            return Err(OrchestratorError::invalid_request(i18n::t(
                "error.question_required",
            )));
        }
        let session_id = request
            .session_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let tool_names = if request.tool_names.is_empty() {
            None
        } else {
            Some(request.tool_names.clone())
        };
        let language = request
            .language
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(i18n::get_default_language);
        let attachments = request
            .attachments
            .clone()
            .filter(|items| !items.is_empty());
        Ok(PreparedRequest {
            user_id,
            question,
            session_id,
            tool_names,
            skip_tool_calls: request.skip_tool_calls,
            model_name: request.model_name.clone(),
            config_overrides: request.config_overrides.clone(),
            stream: request.stream,
            attachments,
            language,
        })
    }

    fn spawn_stream_pump(
        &self,
        session_id: String,
        mut queue_rx: mpsc::Receiver<StreamSignal>,
        event_tx: mpsc::Sender<StreamEvent>,
        emitter: EventEmitter,
        runner: JoinHandle<()>,
    ) {
        let storage = self.storage.clone();
        tokio::spawn(async move {
            let mut last_event_id: i64 = 0;
            let mut closed = false;
            let poll_interval = std::time::Duration::from_secs_f64(STREAM_EVENT_POLL_INTERVAL_S);

            async fn drain_until(
                storage: Arc<dyn StorageBackend>,
                session_id: &str,
                last_event_id: &mut i64,
                target_event_id: i64,
                event_tx: &mpsc::Sender<StreamEvent>,
                emitter: &EventEmitter,
            ) -> bool {
                if target_event_id <= *last_event_id {
                    return true;
                }
                let mut current = *last_event_id;
                while current < target_event_id {
                    let events = load_overflow_events(
                        storage.clone(),
                        session_id.to_string(),
                        current,
                        STREAM_EVENT_FETCH_LIMIT,
                    )
                    .await;
                    if events.is_empty() {
                        break;
                    }
                    let mut progressed = false;
                    for event in events {
                        let Some(event_id) = parse_stream_event_id(&event) else {
                            continue;
                        };
                        if event_id <= current {
                            continue;
                        }
                        if event_tx.send(event).await.is_err() {
                            emitter.close();
                            return false;
                        }
                        current = event_id;
                        progressed = true;
                        if current >= target_event_id {
                            break;
                        }
                    }
                    if !progressed {
                        break;
                    }
                }
                *last_event_id = current;
                true
            }

            loop {
                let mut signal: Option<StreamSignal> = None;
                if !closed {
                    match tokio::time::timeout(poll_interval, queue_rx.recv()).await {
                        Ok(value) => signal = value,
                        Err(_) => signal = None,
                    }
                }

                match signal {
                    Some(StreamSignal::Done) => {
                        closed = true;
                        continue;
                    }
                    Some(StreamSignal::Event(event)) => {
                        let event_id = parse_stream_event_id(&event);
                        if let Some(event_id) = event_id {
                            if event_id > last_event_id + 1 {
                                if !drain_until(
                                    storage.clone(),
                                    &session_id,
                                    &mut last_event_id,
                                    event_id - 1,
                                    &event_tx,
                                    &emitter,
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            if event_id <= last_event_id {
                                continue;
                            }
                        }
                        if event_tx.send(event).await.is_err() {
                            emitter.close();
                            return;
                        }
                        if let Some(event_id) = event_id {
                            last_event_id = event_id;
                        }
                        continue;
                    }
                    None => {
                        let overflow = load_overflow_events(
                            storage.clone(),
                            session_id.clone(),
                            last_event_id,
                            STREAM_EVENT_FETCH_LIMIT,
                        )
                        .await;
                        if !overflow.is_empty() {
                            for event in overflow {
                                let event_id = parse_stream_event_id(&event);
                                if event_tx.send(event).await.is_err() {
                                    emitter.close();
                                    return;
                                }
                                if let Some(event_id) = event_id {
                                    last_event_id = event_id;
                                }
                            }
                            continue;
                        }
                    }
                }

                if closed && runner.is_finished() {
                    break;
                }
                if closed && queue_rx.is_closed() {
                    break;
                }
                if runner.is_finished() && queue_rx.is_empty() {
                    break;
                }
            }
            emitter.close();
        });
    }
}

fn parse_stream_event_id(event: &StreamEvent) -> Option<i64> {
    event.id.as_ref().and_then(|text| text.parse::<i64>().ok())
}

async fn load_overflow_events(
    storage: Arc<dyn StorageBackend>,
    session_id: String,
    after_event_id: i64,
    limit: i64,
) -> Vec<StreamEvent> {
    tokio::task::spawn_blocking(move || {
        load_overflow_events_inner(storage.as_ref(), &session_id, after_event_id, limit)
    })
    .await
    .unwrap_or_default()
}

fn load_overflow_events_inner(
    storage: &dyn StorageBackend,
    session_id: &str,
    after_event_id: i64,
    limit: i64,
) -> Vec<StreamEvent> {
    let records = storage
        .load_stream_events(session_id, after_event_id, limit)
        .unwrap_or_default();
    let mut events = Vec::new();
    for record in records {
        let event_id = record.get("event_id").and_then(Value::as_i64);
        let event_type = record.get("event").and_then(Value::as_str).unwrap_or("");
        if event_type.is_empty() {
            continue;
        }
        let data = record.get("data").cloned().unwrap_or(Value::Null);
        let timestamp = record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let event = StreamEvent {
            event: event_type.to_string(),
            data,
            id: event_id.map(|value| value.to_string()),
            timestamp,
        };
        events.push(event);
    }
    events
}

fn enrich_event_payload(data: Value, session_id: Option<&str>, timestamp: DateTime<Utc>) -> Value {
    let mut map = serde_json::Map::new();
    if let Some(session_id) = session_id {
        let cleaned = session_id.trim();
        if !cleaned.is_empty() {
            map.insert("session_id".to_string(), Value::String(cleaned.to_string()));
        }
    }
    map.insert(
        "timestamp".to_string(),
        Value::String(timestamp.to_rfc3339()),
    );
    map.insert("data".to_string(), data);
    Value::Object(map)
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}
impl Orchestrator {
    async fn execute_request(
        &self,
        prepared: PreparedRequest,
        emitter: EventEmitter,
    ) -> Result<WunderResponse, OrchestratorError> {
        let mut heartbeat_task: Option<JoinHandle<()>> = None;
        let mut acquired = false;
        let limiter = RequestLimiter::new(
            self.storage.clone(),
            self.config_store.get().await.server.max_active_sessions,
        );
        let session_id = prepared.session_id.clone();
        let user_id = prepared.user_id.clone();
        let question = prepared.question.clone();

        let result = async {
            let ok = limiter
                .acquire(&session_id, &user_id)
                .await
                .map_err(|err| OrchestratorError::internal(err.to_string()))?;
            if !ok {
                return Err(OrchestratorError::user_busy(i18n::t("error.user_session_busy")));
            }
            acquired = true;

            // 心跳续租会话锁，避免长任务被误判超时。
            let heartbeat_limiter = limiter.clone();
            let heartbeat_session = session_id.clone();
            heartbeat_task = Some(tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs_f64(SESSION_LOCK_HEARTBEAT_S)).await;
                    heartbeat_limiter.touch(&heartbeat_session).await;
                }
            }));

            self.monitor.register(&session_id, &user_id, &question);
            emitter
                .emit(
                    "progress",
                    json!({
                        "stage": "start",
                        "summary": i18n::t("monitor.summary.received")
                    }),
                )
                .await;

            let config = self.resolve_config(prepared.config_overrides.as_ref()).await;
            let (_llm_name, llm_config) =
                self.resolve_llm_config(&config, prepared.model_name.as_deref())?;
            let skills = if prepared.config_overrides.is_some() {
                Arc::new(RwLock::new(load_skills(&config, true, true)))
            } else {
                self.skills.clone()
            };
            let skills_snapshot = skills.read().await.clone();
            let user_tool_bindings = self
                .user_tool_manager
                .build_bindings(&config, &skills_snapshot, &user_id);
            let allowed_tool_names = self.resolve_allowed_tool_names(
                &config,
                prepared.tool_names.as_deref().unwrap_or(&[]),
                &skills_snapshot,
                Some(&user_tool_bindings),
            );

            let mut system_prompt = self
                .build_system_prompt_with_allowed(
                    &config,
                    prepared.config_overrides.as_ref(),
                    &allowed_tool_names,
                    &skills_snapshot,
                    Some(&user_tool_bindings),
                    &user_id,
                )
                .await;
            system_prompt = self
                .resolve_session_prompt(
                    &user_id,
                    &session_id,
                    system_prompt,
                    prepared.tool_names.as_ref(),
                    prepared.config_overrides.as_ref(),
                    Some(&prepared.language),
                )
                .await;
            system_prompt = self.append_memory_prompt(&user_id, system_prompt).await;

            let history_manager = HistoryManager;
            let mut messages = vec![json!({ "role": "system", "content": system_prompt })];
            let history_messages = history_manager
                .load_history_messages_async(
                    self.workspace.clone(),
                    user_id.clone(),
                    session_id.clone(),
                    config.workspace.max_history_items,
                )
                .await;
            messages.extend(history_messages);
            let user_message = self.build_user_message(&question, prepared.attachments.as_deref());
            messages.push(user_message.clone());
            self.append_chat(&user_id, &session_id, "user", user_message.get("content"), None, None);

            let max_rounds = llm_config.max_rounds.unwrap_or(1).max(1) as i64;
            let mut last_usage: Option<TokenUsage> = None;
            let mut answer = String::new();
            let mut stop_reason: Option<String> = None;
            let mut a2ui_uid: Option<String> = None;
            let mut a2ui_messages: Option<Value> = None;
            let mut last_response: Option<(String, String)> = None;
            let mut last_request_messages: Option<Vec<Value>> = None;

            for round in 1..=max_rounds {
                self.ensure_not_cancelled(&session_id)?;
                messages = self
                    .maybe_compact_messages(
                        &config,
                        &llm_config,
                        &user_id,
                        &session_id,
                        messages,
                        &emitter,
                        &question,
                    )
                    .await?;
                self.ensure_not_cancelled(&session_id)?;

                last_request_messages = Some(self.sanitize_messages_for_log(
                    messages.clone(),
                    prepared.attachments.as_deref(),
                ));

                emitter
                    .emit(
                        "progress",
                        json!({
                            "stage": "llm_call",
                            "summary": i18n::t("monitor.summary.model_call"),
                            "round": round
                        }),
                    )
                    .await;

                let (content, reasoning, usage) = self
                    .call_llm(
                        &llm_config,
                        &messages,
                        &emitter,
                        &session_id,
                        prepared.stream,
                        round,
                        true,
                        None,
                    )
                    .await?;
                last_response = Some((content.clone(), reasoning.clone()));
                last_usage = Some(usage.clone());
                self.workspace
                    .save_session_token_usage_async(&user_id, &session_id, usage.total as i64)
                    .await;

                let tool_calls = if prepared.skip_tool_calls {
                    Vec::new()
                } else {
                    collect_tool_calls_from_output(&content, &reasoning)
                };
                if tool_calls.is_empty() {
                    if prepared.skip_tool_calls {
                        answer = content.trim().to_string();
                    } else {
                        answer = self.resolve_final_answer(&content);
                    }
                    stop_reason = Some("model_response".to_string());
                    let assistant_content = if answer.is_empty() { content.clone() } else { answer.clone() };
                    if !assistant_content.trim().is_empty() {
                        self.append_chat(
                            &user_id,
                            &session_id,
                            "assistant",
                            Some(&json!(assistant_content)),
                            None,
                            Some(&reasoning),
                        );
                    }
                    if answer.is_empty() {
                        answer = content.trim().to_string();
                    }
                    break;
                }

                let assistant_content = content.clone();
                let assistant_reasoning = reasoning.clone();
                if !assistant_content.trim().is_empty() || !assistant_reasoning.trim().is_empty() {
                    let mut assistant_message = json!({
                        "role": "assistant",
                        "content": assistant_content.clone(),
                    });
                    if !assistant_reasoning.trim().is_empty() {
                        assistant_message["reasoning_content"] = json!(assistant_reasoning.clone());
                    }
                    messages.push(assistant_message);
                    let meta = json!({ "type": "tool_call" });
                    self.append_chat(
                        &user_id,
                        &session_id,
                        "assistant",
                        Some(&json!(assistant_content)),
                        Some(&meta),
                        Some(&assistant_reasoning),
                    );
                }

                let tool_event_emitter = ToolEventEmitter::new({
                    let emitter = emitter.clone();
                    move |event_type, data| {
                        let emitter = emitter.clone();
                        let event_name = event_type.to_string();
                        tokio::spawn(async move {
                            emitter.emit(&event_name, data).await;
                        });
                    }
                });

                for call in tool_calls {
                    let mut name = call.name.clone();
                    let args = call.arguments.clone();
                    if name.trim().is_empty() {
                        continue;
                    }
                    name = resolve_tool_name(&name);

                    self.ensure_not_cancelled(&session_id)?;
                    if name == "a2ui" {
                        let (uid, messages_payload, content) =
                            self.resolve_a2ui_tool_payload(&args, &user_id, &session_id);
                        if let Some(messages_payload) = messages_payload.as_ref() {
                            emitter
                                .emit(
                                    "a2ui",
                                    json!({
                                        "uid": uid,
                                        "messages": messages_payload,
                                        "content": content
                                    }),
                                )
                                .await;
                        }
                        a2ui_uid = if uid.trim().is_empty() { None } else { Some(uid.clone()) };
                        a2ui_messages = messages_payload;
                        answer = if content.trim().is_empty() {
                            i18n::t("response.a2ui_fallback")
                        } else {
                            content
                        };
                        stop_reason = Some("a2ui".to_string());
                        self.log_a2ui_tool_call(&user_id, &session_id, &name, &args, &uid, &a2ui_messages, &answer);
                        if !answer.trim().is_empty() {
                            self.append_chat(
                                &user_id,
                                &session_id,
                                "assistant",
                                Some(&json!(answer.clone())),
                                None,
                                None,
                            );
                        }
                        break;
                    }
                    if name == "最终回复" {
                        answer = self.resolve_final_answer_from_tool(&args);
                        stop_reason = Some("final_tool".to_string());
                        self.log_final_tool_call(&user_id, &session_id, &name, &args);
                        if !answer.trim().is_empty() {
                            self.append_chat(
                                &user_id,
                                &session_id,
                                "assistant",
                                Some(&json!(answer.clone())),
                                None,
                                None,
                            );
                        }
                        break;
                    }

                    let tool_context = ToolContext {
                        user_id: &user_id,
                        session_id: &session_id,
                        workspace: self.workspace.clone(),
                        config: &config,
                        a2a_store: &self.a2a_store,
                        skills: &skills_snapshot,
                        user_tool_manager: Some(self.user_tool_manager.as_ref()),
                        user_tool_bindings: Some(&user_tool_bindings),
                        user_tool_store: Some(self.user_tool_manager.store()),
                        event_emitter: Some(tool_event_emitter.clone()),
                        http: &self.http,
                    };

                    let tool_timeout = self.resolve_tool_timeout(&config, &name, &args);
                    let tool_result = if !allowed_tool_names.contains(&name) {
                        let safe_args = if args.is_object() { args.clone() } else { json!({ "raw": args }) };
                        emitter
                            .emit("tool_call", json!({ "tool": name, "args": safe_args }))
                            .await;
                        ToolResultPayload::error(
                            i18n::t("error.tool_disabled_or_unavailable"),
                            json!({ "tool": name }),
                        )
                    } else {
                        emitter
                            .emit("tool_call", json!({ "tool": name, "args": args }))
                            .await;
                        let result = tokio::select! {
                            res = self.execute_tool_with_timeout(&tool_context, &name, &args, tool_timeout) => res,
                            err = self.wait_for_cancelled(&session_id) => {
                                return Err(err);
                            }
                        };
                        match result {
                            Ok(value) => ToolResultPayload::from_value(value),
                            Err(err) => {
                                let message = if err.to_string() == TOOL_TIMEOUT_ERROR {
                                    i18n::t_with_params(
                                        "error.tool_execution_failed",
                                        &HashMap::from([(
                                            "name".to_string(),
                                            format!("{name} timeout"),
                                        )]),
                                    )
                                } else {
                                    err.to_string()
                                };
                                ToolResultPayload::error(message, json!({ "tool": name }))
                            }
                        }
                    };

                    let observation = self.build_tool_observation(&name, &tool_result);
                    messages.push(json!({
                        "role": "user",
                        "content": format!("{OBSERVATION_PREFIX}{observation}"),
                    }));
                    self.append_chat(
                        &user_id,
                        &session_id,
                        "tool",
                        Some(&json!(observation)),
                        None,
                        None,
                    );

                    self.append_tool_log(&user_id, &session_id, &name, &args, &tool_result);
                    self.append_artifact_logs(
                        &user_id,
                        &session_id,
                        &name,
                        &args,
                        &tool_result,
                    );
                    if name == "读取文件" {
                        self.append_skill_usage_logs(
                            &user_id,
                            &session_id,
                            &args,
                            &skills_snapshot,
                            Some(&user_tool_bindings),
                        );
                    }

                    emitter
                        .emit(
                            "tool_result",
                            tool_result.to_event_payload(&name),
                        )
                        .await;

                    self.ensure_not_cancelled(&session_id)?;
                    if !answer.is_empty() {
                        break;
                    }
                }
                if !answer.is_empty() {
                    break;
                }
            }

            if answer.is_empty() {
                if let Some((content, _)) = last_response.as_ref() {
                    answer = self.resolve_final_answer(content);
                    if stop_reason.is_none() {
                        stop_reason = Some("max_rounds".to_string());
                    }
                }
            }
            if answer.is_empty() {
                answer = i18n::t("error.max_rounds_no_final_answer");
                if stop_reason.is_none() {
                    stop_reason = Some("max_rounds".to_string());
                }
            }

            self.enqueue_memory_summary(&prepared, last_request_messages, &answer)
                .await;

            let stop_reason = stop_reason.unwrap_or_else(|| "unknown".to_string());
            let response = WunderResponse {
                session_id: session_id.clone(),
                answer: answer.clone(),
                usage: last_usage.clone(),
                stop_reason: Some(stop_reason.clone()),
                uid: a2ui_uid.clone(),
                a2ui: a2ui_messages.clone(),
            };
            emitter
                .emit(
                    "final",
                    json!({
                        "answer": answer,
                        "usage": last_usage.clone().unwrap_or(TokenUsage { input: 0, output: 0, total: 0 }),
                        "stop_reason": stop_reason
                    }),
                )
                .await;
            self.monitor.mark_finished(&session_id);
            Ok(response)
        }
        .await;

        match result {
            Ok(value) => {
                emitter.finish().await;
                if acquired {
                    limiter.release(&session_id).await;
                }
                if let Some(handle) = heartbeat_task.take() {
                    handle.abort();
                }
                Ok(value)
            }
            Err(err) => {
                emitter.emit("error", err.to_payload()).await;
                if err.code == "CANCELLED" {
                    self.monitor.mark_cancelled(&session_id);
                } else if err.code != "USER_BUSY" {
                    self.monitor.mark_error(&session_id, &err.message);
                }
                emitter.finish().await;
                if acquired {
                    limiter.release(&session_id).await;
                }
                if let Some(handle) = heartbeat_task.take() {
                    handle.abort();
                }
                Err(err)
            }
        }
    }
}
impl Orchestrator {
    async fn resolve_config(&self, overrides: Option<&Value>) -> Config {
        let base = self.config_store.get().await;
        let Some(overrides) = overrides else {
            return base;
        };
        let mut base_value = serde_json::to_value(&base).unwrap_or(Value::Null);
        merge_json(&mut base_value, overrides);
        serde_json::from_value::<Config>(base_value).unwrap_or(base)
    }

    fn resolve_llm_config(
        &self,
        config: &Config,
        model_name: Option<&str>,
    ) -> Result<(String, LlmModelConfig), OrchestratorError> {
        let name = model_name
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| config.llm.default.as_str());
        if let Some(configured) = config.llm.models.get(name) {
            return Ok((name.to_string(), configured.clone()));
        }
        if let Some((fallback_name, fallback)) = config.llm.models.iter().next() {
            return Ok((fallback_name.clone(), fallback.clone()));
        }
        Err(OrchestratorError::llm_unavailable(i18n::t(
            "error.llm_unavailable",
        )))
    }

    fn ensure_not_cancelled(&self, session_id: &str) -> Result<(), OrchestratorError> {
        if self.monitor.is_cancelled(session_id) {
            return Err(OrchestratorError::cancelled(i18n::t(
                "error.session_cancelled",
            )));
        }
        Ok(())
    }

    async fn wait_for_cancelled(&self, session_id: &str) -> OrchestratorError {
        let mut interval = tokio::time::interval(Duration::from_millis(200));
        loop {
            interval.tick().await;
            if self.monitor.is_cancelled(session_id) {
                return OrchestratorError::cancelled(i18n::t("error.session_cancelled"));
            }
        }
    }

    async fn sleep_or_cancel(
        &self,
        session_id: &str,
        duration: Duration,
    ) -> Result<(), OrchestratorError> {
        let cancel = self.wait_for_cancelled(session_id);
        tokio::select! {
            _ = tokio::time::sleep(duration) => Ok(()),
            err = cancel => Err(err),
        }
    }

    async fn await_with_cancel<F, T>(
        &self,
        session_id: &str,
        timeout_s: u64,
        fut: F,
    ) -> Result<Result<T, anyhow::Error>, OrchestratorError>
    where
        F: std::future::Future<Output = Result<T, anyhow::Error>>,
    {
        let cancel = self.wait_for_cancelled(session_id);
        if timeout_s > 0 {
            tokio::select! {
                res = tokio::time::timeout(Duration::from_secs(timeout_s), fut) => {
                    Ok(res.map_err(|_| anyhow::anyhow!("timeout")).and_then(|inner| inner))
                }
                err = cancel => Err(err),
            }
        } else {
            tokio::select! {
                res = fut => Ok(res),
                err = cancel => Err(err),
            }
        }
    }

    fn append_chat(
        &self,
        user_id: &str,
        session_id: &str,
        role: &str,
        content: Option<&Value>,
        meta: Option<&Value>,
        reasoning: Option<&str>,
    ) {
        let timestamp = Utc::now().to_rfc3339();
        let content_value = content
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()));
        let content_value = match content_value {
            Value::String(_) | Value::Array(_) | Value::Object(_) => content_value,
            Value::Null => Value::String(String::new()),
            other => Value::String(other.to_string()),
        };
        let mut payload = json!({
            "role": role,
            "content": content_value,
            "session_id": session_id,
            "timestamp": timestamp,
        });
        if let Some(reasoning) = reasoning {
            let cleaned = reasoning.trim();
            if !cleaned.is_empty() {
                payload["reasoning_content"] = Value::String(cleaned.to_string());
            }
        }
        if let Some(meta) = meta {
            if !meta.is_null() {
                payload["meta"] = meta.clone();
            }
        }
        let _ = self.workspace.append_chat(user_id, &payload);
    }

    fn build_tool_observation(&self, tool_name: &str, result: &ToolResultPayload) -> String {
        serde_json::to_string(&result.to_observation_payload(tool_name))
            .unwrap_or_else(|_| "{}".to_string())
    }

    fn append_tool_log(
        &self,
        user_id: &str,
        session_id: &str,
        tool_name: &str,
        args: &Value,
        result: &ToolResultPayload,
    ) {
        let timestamp = Utc::now().to_rfc3339();
        let safe_args = if args.is_object() {
            args.clone()
        } else {
            json!({ "raw": args })
        };
        let mut payload = json!({
            "tool": tool_name,
            "session_id": session_id,
            "ok": result.ok,
            "error": result.error,
            "args": safe_args,
            "data": result.data,
            "timestamp": timestamp,
        });
        if result.sandbox {
            payload["sandbox"] = Value::Bool(true);
        }
        let _ = self.workspace.append_tool_log(user_id, &payload);
    }

    fn append_artifact_logs(
        &self,
        user_id: &str,
        session_id: &str,
        tool_name: &str,
        args: &Value,
        result: &ToolResultPayload,
    ) {
        let entries = self.build_artifact_entries(tool_name, args, result);
        if entries.is_empty() {
            return;
        }
        let timestamp = Utc::now().to_rfc3339();
        for mut entry in entries {
            if let Value::Object(ref mut map) = entry {
                map.entry("tool".to_string())
                    .or_insert_with(|| Value::String(tool_name.to_string()));
                map.entry("ok".to_string())
                    .or_insert_with(|| Value::Bool(result.ok));
                if !result.error.trim().is_empty() {
                    map.entry("error".to_string())
                        .or_insert_with(|| Value::String(result.error.clone()));
                }
                map.insert(
                    "session_id".to_string(),
                    Value::String(session_id.to_string()),
                );
                map.insert("timestamp".to_string(), Value::String(timestamp.clone()));
            }
            let _ = self.workspace.append_artifact_log(user_id, &entry);
        }
    }

    fn build_artifact_entries(
        &self,
        tool_name: &str,
        args: &Value,
        result: &ToolResultPayload,
    ) -> Vec<Value> {
        let mut entries = Vec::new();
        let file_actions = HashMap::from([
            ("读取文件", "read"),
            ("写入文件", "write"),
            ("替换文本", "replace"),
            ("编辑文件", "edit"),
        ]);
        if let Some(action) = file_actions.get(tool_name) {
            let paths = extract_file_paths(args);
            for path in paths {
                let mut meta = serde_json::Map::new();
                if let Value::Object(data) = &result.data {
                    if *action == "replace" {
                        if let Some(value) = data.get("replaced") {
                            meta.insert("replaced".to_string(), value.clone());
                        }
                    } else if *action == "write" {
                        if let Some(value) = data.get("bytes") {
                            meta.insert("bytes".to_string(), value.clone());
                        }
                    } else if *action == "edit" {
                        if let Some(value) = data.get("lines") {
                            meta.insert("lines".to_string(), value.clone());
                        }
                    }
                }
                entries.push(json!({
                    "kind": "file",
                    "action": action,
                    "name": path,
                    "meta": Value::Object(meta),
                }));
            }
            return entries;
        }

        if tool_name == "执行命令" {
            let commands = extract_command_lines(args);
            let mut returncode_map = HashMap::new();
            let mut fallback_rc: Option<Value> = None;
            if let Value::Object(data) = &result.data {
                if let Some(Value::Array(items)) = data.get("results") {
                    for item in items {
                        let Some(obj) = item.as_object() else {
                            continue;
                        };
                        let command = obj
                            .get("command")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if command.is_empty() {
                            continue;
                        }
                        returncode_map.insert(
                            command,
                            obj.get("returncode").cloned().unwrap_or(Value::Null),
                        );
                    }
                }
                if data.contains_key("returncode") {
                    fallback_rc = data.get("returncode").cloned();
                }
            }
            for command in commands {
                let returncode = returncode_map
                    .get(&command)
                    .cloned()
                    .or_else(|| fallback_rc.clone());
                let ok = match returncode.as_ref().and_then(Value::as_i64) {
                    Some(code) => code == 0,
                    None => result.ok,
                };
                entries.push(json!({
                    "kind": "command",
                    "action": "execute",
                    "name": command,
                    "ok": ok,
                    "meta": { "returncode": returncode.unwrap_or(Value::Null) },
                }));
            }
            return entries;
        }

        if tool_name == "ptc" {
            let mut script_path = String::new();
            if let Value::Object(data) = &result.data {
                script_path = data
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
            if script_path.is_empty() {
                script_path = args
                    .get("filename")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
            if script_path.is_empty() {
                return entries;
            }
            let returncode = match &result.data {
                Value::Object(data) => data.get("returncode").cloned(),
                _ => None,
            };
            let ok = match returncode.as_ref().and_then(Value::as_i64) {
                Some(code) => code == 0,
                None => result.ok,
            };
            entries.push(json!({
                "kind": "script",
                "action": "run",
                "name": script_path,
                "ok": ok,
                "meta": { "returncode": returncode.unwrap_or(Value::Null) }
            }));
            return entries;
        }

        entries
    }

    fn append_skill_usage_logs(
        &self,
        user_id: &str,
        session_id: &str,
        args: &Value,
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
    ) {
        let paths = extract_file_paths(args);
        if paths.is_empty() {
            return;
        }
        let mut specs = skills.list_specs();
        if let Some(bindings) = user_tool_bindings {
            if !bindings.skill_specs.is_empty() {
                specs.extend(bindings.skill_specs.iter().cloned());
            }
        }
        if specs.is_empty() {
            return;
        }

        let mut seen_names = HashSet::new();
        let mut path_map: HashMap<PathBuf, String> = HashMap::new();
        for spec in specs {
            let name = spec.name.trim();
            if name.is_empty() {
                continue;
            }
            if !seen_names.insert(name.to_string()) {
                continue;
            }
            let Some(spec_path) = resolve_absolute_path(&spec.path) else {
                continue;
            };
            let key = normalize_compare_path(&spec_path);
            path_map.insert(key, name.to_string());
        }
        if path_map.is_empty() {
            return;
        }

        let mut matched = HashSet::new();
        for raw in paths {
            let Some(candidate) = resolve_absolute_path(&raw) else {
                continue;
            };
            let key = normalize_compare_path(&candidate);
            if let Some(name) = path_map.get(&key) {
                matched.insert(name.clone());
            }
        }
        if matched.is_empty() {
            return;
        }
        let result = ToolResultPayload::from_value(json!({ "source": "skill_read" }));
        for name in matched {
            self.append_tool_log(user_id, session_id, &name, args, &result);
        }
    }

    fn resolve_final_answer(&self, content: &str) -> String {
        strip_tool_calls(content).trim().to_string()
    }

    fn resolve_final_answer_from_tool(&self, args: &Value) -> String {
        if let Some(obj) = args.as_object() {
            let value = obj
                .get("content")
                .or_else(|| obj.get("answer"))
                .cloned()
                .unwrap_or(Value::Null);
            match value {
                Value::String(text) => text.trim().to_string(),
                Value::Null => String::new(),
                other => serde_json::to_string(&other).unwrap_or_else(|_| other.to_string()),
            }
        } else if let Some(text) = args.as_str() {
            text.trim().to_string()
        } else {
            String::new()
        }
    }

    fn resolve_a2ui_tool_payload(
        &self,
        args: &Value,
        user_id: &str,
        session_id: &str,
    ) -> (String, Option<Value>, String) {
        let (mut uid, content, mut raw_messages) = if let Some(obj) = args.as_object() {
            let uid = obj
                .get("uid")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let content = obj
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let raw_messages = obj
                .get("a2ui")
                .cloned()
                .or_else(|| obj.get("messages").cloned())
                .unwrap_or(Value::Null);
            (uid, content, raw_messages)
        } else {
            (String::new(), String::new(), args.clone())
        };
        if uid.trim().is_empty() {
            uid = session_id.trim().to_string();
            if uid.is_empty() {
                uid = user_id.trim().to_string();
            }
        }
        if let Value::String(text) = raw_messages {
            raw_messages = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
        }
        if raw_messages.is_object() {
            raw_messages = Value::Array(vec![raw_messages]);
        }
        let Value::Array(items) = raw_messages else {
            return (uid, None, content);
        };
        let mut normalized = Vec::new();
        for item in items {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let mut message = obj.clone();
            for key in [
                "beginRendering",
                "surfaceUpdate",
                "dataModelUpdate",
                "deleteSurface",
            ] {
                if let Some(payload) = message.get(key).and_then(Value::as_object) {
                    if !uid.is_empty() && !payload.contains_key("surfaceId") {
                        let mut payload = payload.clone();
                        payload.insert("surfaceId".to_string(), Value::String(uid.clone()));
                        message.insert(key.to_string(), Value::Object(payload));
                    }
                    break;
                }
            }
            normalized.push(Value::Object(message));
        }
        let messages_payload = if normalized.is_empty() {
            None
        } else {
            Some(Value::Array(normalized))
        };
        (uid, messages_payload, content)
    }

    fn log_final_tool_call(&self, user_id: &str, session_id: &str, name: &str, args: &Value) {
        let content = self.resolve_final_answer_from_tool(args);
        let data = if content.trim().is_empty() {
            json!({})
        } else {
            json!({ "content": content })
        };
        let result = ToolResultPayload::from_value(data);
        self.append_tool_log(user_id, session_id, name, args, &result);
    }

    fn log_a2ui_tool_call(
        &self,
        user_id: &str,
        session_id: &str,
        name: &str,
        args: &Value,
        uid: &str,
        messages: &Option<Value>,
        content: &str,
    ) {
        let message_count = messages
            .as_ref()
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or(0);
        let mut data = json!({
            "uid": uid,
            "message_count": message_count,
        });
        if !content.trim().is_empty() {
            if let Value::Object(ref mut map) = data {
                map.insert(
                    "content".to_string(),
                    Value::String(content.trim().to_string()),
                );
            }
        }
        let result = ToolResultPayload::from_value(data);
        self.append_tool_log(user_id, session_id, name, args, &result);
    }

    fn build_chat_messages(&self, messages: &[Value]) -> Vec<ChatMessage> {
        messages
            .iter()
            .filter_map(|message| {
                let role = message.get("role").and_then(Value::as_str)?.to_string();
                let content = message.get("content").cloned().unwrap_or(Value::Null);
                let reasoning_content = message
                    .get("reasoning_content")
                    .or_else(|| message.get("reasoning"))
                    .and_then(Value::as_str)
                    .and_then(|text| {
                        if text.trim().is_empty() {
                            None
                        } else {
                            Some(text.to_string())
                        }
                    });
                Some(ChatMessage {
                    role,
                    content,
                    reasoning_content,
                })
            })
            .collect()
    }

    fn estimate_token_usage(
        &self,
        messages: &[Value],
        content: &str,
        reasoning: &str,
    ) -> TokenUsage {
        let input = estimate_messages_tokens(messages).max(0) as u64;
        let output = (approx_token_count(content) + approx_token_count(reasoning)).max(0) as u64;
        TokenUsage {
            input,
            output,
            total: input + output,
        }
    }

    fn resolve_llm_timeout_s(&self, config: &LlmModelConfig) -> u64 {
        let timeout_s = config.timeout_s.unwrap_or(DEFAULT_LLM_TIMEOUT_S);
        if timeout_s == 0 {
            DEFAULT_LLM_TIMEOUT_S
        } else {
            timeout_s
        }
    }

    async fn call_llm(
        &self,
        llm_config: &LlmModelConfig,
        messages: &[Value],
        emitter: &EventEmitter,
        session_id: &str,
        stream: bool,
        round_index: i64,
        emit_events: bool,
        llm_config_override: Option<LlmModelConfig>,
    ) -> Result<(String, String, TokenUsage), OrchestratorError> {
        self.ensure_not_cancelled(session_id)?;
        let effective_config = llm_config_override.unwrap_or_else(|| llm_config.clone());
        if !is_llm_configured(&effective_config) {
            if effective_config.mock_if_unconfigured.unwrap_or(false) {
                let content = i18n::t("error.llm_not_configured");
                let usage = self.estimate_token_usage(messages, &content, "");
                if emit_events {
                    emitter
                        .emit(
                            "llm_output",
                            json!({ "content": content, "reasoning": "", "round": round_index, "usage": usage }),
                        )
                        .await;
                    emitter
                        .emit(
                            "token_usage",
                            json!({
                                "round": round_index,
                                "input_tokens": usage.input,
                                "output_tokens": usage.output,
                                "total_tokens": usage.total,
                            }),
                        )
                        .await;
                }
                return Ok((content, String::new(), usage));
            }
            let detail = i18n::t("error.llm_config_missing");
            return Err(OrchestratorError::llm_unavailable(i18n::t_with_params(
                "error.llm_unavailable",
                &HashMap::from([("detail".to_string(), detail)]),
            )));
        }

        let client = build_llm_client(&effective_config, self.http.clone());
        let chat_messages = self.build_chat_messages(messages);
        let will_stream = stream;

        if emit_events {
            let payload_messages = self.sanitize_messages_for_log(messages.to_vec(), None);
            let payload_chat = self.build_chat_messages(&payload_messages);
            let payload = client.build_request_payload(&payload_chat, will_stream);
            let request_payload = json!({
                "provider": effective_config.provider,
                "model": effective_config.model,
                "base_url": effective_config.base_url,
                "round": round_index,
                "stream": will_stream,
                "payload": payload,
            });
            emitter.emit("llm_request", request_payload).await;
        }

        let timeout_s = self.resolve_llm_timeout_s(&effective_config);
        let max_attempts = effective_config.retry.unwrap_or(0).saturating_add(1).max(1);
        let mut attempt = 0u32;
        let mut last_err: anyhow::Error;
        loop {
            attempt += 1;
            let request_started_at = Instant::now();
            let output_timing = Arc::new(parking_lot::Mutex::new(OutputTiming::default()));
            let result = if will_stream {
                let emitter_snapshot = emitter.clone();
                let timing_snapshot = Arc::clone(&output_timing);
                let on_delta = move |delta: String, reasoning_delta: String| {
                    let emitter = emitter_snapshot.clone();
                    let timing = Arc::clone(&timing_snapshot);
                    async move {
                        if !delta.is_empty() || !reasoning_delta.is_empty() {
                            timing.lock().mark_output(Instant::now());
                        }
                        if emit_events {
                            let mut payload = serde_json::Map::new();
                            if !delta.is_empty() {
                                payload.insert("delta".to_string(), Value::String(delta));
                            }
                            if !reasoning_delta.is_empty() {
                                payload.insert(
                                    "reasoning_delta".to_string(),
                                    Value::String(reasoning_delta),
                                );
                            }
                            payload.insert("round".to_string(), json!(round_index));
                            emitter
                                .emit("llm_output_delta", Value::Object(payload))
                                .await;
                        }
                        Ok(())
                    }
                };
                let fut = client.stream_complete_with_callback(&chat_messages, on_delta);
                self.await_with_cancel(session_id, timeout_s, fut).await?
            } else {
                let fut = client.complete(&chat_messages);
                self.await_with_cancel(session_id, timeout_s, fut).await?
            };

            match result {
                Ok(response) => {
                    let response_finished_at = Instant::now();
                    let content = response.content;
                    let reasoning = response.reasoning;
                    let mut usage = response.usage;
                    if let Some(item) = usage.as_mut() {
                        if item.total == 0 {
                            let total = item.input.saturating_add(item.output);
                            if total > 0 {
                                item.total = total;
                            }
                        }
                    }
                    let mut usage = usage.filter(|item| item.total > 0).unwrap_or_else(|| {
                        self.estimate_token_usage(messages, &content, &reasoning)
                    });
                    if (usage.input == 0 || usage.output == 0) && usage.total > 0 {
                        let estimated = self.estimate_token_usage(messages, &content, &reasoning);
                        if estimated.total > 0 {
                            let ratio = usage.total as f64 / estimated.total as f64;
                            let mut input = (estimated.input as f64 * ratio).round() as u64;
                            if input > usage.total {
                                input = usage.total;
                            }
                            let output = usage.total.saturating_sub(input);
                            usage.input = input;
                            usage.output = output;
                        }
                    }
                    let (prefill_duration_s, decode_duration_s) = if will_stream {
                        output_timing
                            .lock()
                            .durations(request_started_at, response_finished_at)
                    } else {
                        (None, None)
                    };
                    if emit_events {
                        emitter
                            .emit(
                                "llm_output",
                                json!({
                                    "content": content,
                                    "reasoning": reasoning,
                                    "round": round_index,
                                    "usage": usage,
                                    "prefill_duration_s": prefill_duration_s,
                                    "decode_duration_s": decode_duration_s,
                                }),
                            )
                            .await;
                        emitter
                            .emit(
                                "token_usage",
                                json!({
                                    "round": round_index,
                                    "input_tokens": usage.input,
                                    "output_tokens": usage.output,
                                    "total_tokens": usage.total,
                                    "prefill_duration_s": prefill_duration_s,
                                    "decode_duration_s": decode_duration_s,
                                }),
                            )
                            .await;
                    }
                    return Ok((content, reasoning, usage));
                }
                Err(err) => {
                    last_err = err;
                }
            }

            if attempt >= max_attempts {
                break;
            }
            if emit_events && will_stream {
                let delay_s = (attempt as f64).min(3.0);
                emitter
                    .emit(
                        "llm_stream_retry",
                        json!({
                            "attempt": attempt,
                            "max_attempts": max_attempts,
                            "delay_s": delay_s,
                            "will_retry": true,
                            "round": round_index,
                        }),
                    )
                    .await;
                self.sleep_or_cancel(session_id, Duration::from_secs_f64(delay_s))
                    .await?;
            }
        }

        let detail = last_err.to_string();
        Err(OrchestratorError::internal(i18n::t_with_params(
            "error.llm_call_failed",
            &HashMap::from([("detail".to_string(), detail)]),
        )))
    }

    async fn execute_tool_with_timeout(
        &self,
        tool_context: &ToolContext<'_>,
        name: &str,
        args: &Value,
        timeout: Option<Duration>,
    ) -> Result<Value, anyhow::Error> {
        if let Some(timeout) = timeout {
            match tokio::time::timeout(
                timeout,
                crate::tools::execute_tool(tool_context, name, args),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(anyhow!(TOOL_TIMEOUT_ERROR)),
            }
        } else {
            crate::tools::execute_tool(tool_context, name, args).await
        }
    }

    fn resolve_tool_timeout(
        &self,
        config: &Config,
        tool_name: &str,
        args: &Value,
    ) -> Option<Duration> {
        let mut timeout_s = parse_timeout_secs(args.get("timeout_s")).unwrap_or(0.0);
        if tool_name == "a2a等待" {
            let wait_s = parse_timeout_secs(args.get("wait_s")).unwrap_or(0.0);
            if wait_s > 0.0 {
                timeout_s = timeout_s.max(wait_s);
            }
            if timeout_s <= 0.0 {
                timeout_s = config.a2a.timeout_s as f64;
            }
        } else if tool_name == "a2a观察" || tool_name.starts_with("a2a@") {
            if timeout_s <= 0.0 {
                timeout_s = config.a2a.timeout_s as f64;
            }
        } else if tool_name.contains('@') {
            if timeout_s <= 0.0 {
                let fallback = DEFAULT_TOOL_TIMEOUT_S;
                let configured = config.mcp.timeout_s as f64;
                timeout_s = if configured > 0.0 {
                    configured.max(fallback)
                } else {
                    fallback
                };
            }
        } else if timeout_s <= 0.0 {
            let fallback = DEFAULT_TOOL_TIMEOUT_S;
            let sandbox_timeout = if sandbox::sandbox_enabled(config) {
                config.sandbox.timeout_s as f64
            } else {
                0.0
            };
            timeout_s = if sandbox_timeout > 0.0 {
                sandbox_timeout.max(fallback)
            } else {
                fallback
            };
        }
        if timeout_s <= 0.0 {
            None
        } else {
            Some(Duration::from_secs_f64(timeout_s.max(MIN_TOOL_TIMEOUT_S)))
        }
    }

    fn shrink_messages_to_limit(&self, messages: Vec<Value>, limit: i64) -> Vec<Value> {
        let total_tokens = estimate_messages_tokens(&messages);
        if total_tokens <= limit {
            return messages;
        }
        let mut overflow = total_tokens - limit;
        let mut trimmed = messages;
        for index in 0..trimmed.len() {
            if overflow <= 0 {
                break;
            }
            let changed = if let Some(obj) = trimmed[index].as_object_mut() {
                let role = obj.get("role").and_then(Value::as_str).unwrap_or("");
                let content = obj.get("content").unwrap_or(&Value::Null);
                if !Self::is_observation_message(role, content) {
                    false
                } else if let Value::String(text) = content {
                    let current_tokens = approx_token_count(text);
                    if current_tokens <= COMPACTION_MIN_OBSERVATION_TOKENS {
                        false
                    } else {
                        let target_tokens =
                            (current_tokens - overflow).max(COMPACTION_MIN_OBSERVATION_TOKENS);
                        let new_content =
                            trim_text_to_tokens(text, target_tokens, "...(truncated)");
                        if new_content == *text {
                            false
                        } else {
                            obj.insert("content".to_string(), Value::String(new_content));
                            true
                        }
                    }
                } else {
                    false
                }
            } else {
                false
            };
            if changed {
                overflow = (estimate_messages_tokens(&trimmed) - limit).max(0);
            }
        }
        trimmed
    }

    fn prepare_summary_messages(&self, messages: Vec<Value>, max_tokens: i64) -> Vec<Value> {
        if messages.is_empty() {
            return messages;
        }
        let mut trimmed = Vec::with_capacity(messages.len());
        for message in messages {
            let Some(obj) = message.as_object() else {
                trimmed.push(message);
                continue;
            };
            let role = obj.get("role").and_then(Value::as_str).unwrap_or("");
            let content = obj.get("content").cloned().unwrap_or(Value::Null);
            let mut new_message = obj.clone();
            if let Value::String(text) = &content {
                let target = max_tokens.max(1);
                if approx_token_count(text) > target {
                    new_message.insert(
                        "content".to_string(),
                        Value::String(trim_text_to_tokens(text, target, "...(truncated)")),
                    );
                }
            }
            if role == "assistant" {
                new_message.remove("reasoning_content");
                new_message.remove("reasoning");
            }
            trimmed.push(Value::Object(new_message));
        }
        trimmed
    }

    fn locate_current_user_index(messages: &[Value]) -> Option<usize> {
        messages.iter().rposition(|message| {
            let role = message.get("role").and_then(Value::as_str).unwrap_or("");
            if role != "user" {
                return false;
            }
            let content = message.get("content").unwrap_or(&Value::Null);
            !Self::is_observation_message(role, content)
        })
    }

    async fn maybe_compact_messages(
        &self,
        config: &Config,
        llm_config: &LlmModelConfig,
        user_id: &str,
        session_id: &str,
        messages: Vec<Value>,
        emitter: &EventEmitter,
        current_question: &str,
    ) -> Result<Vec<Value>, OrchestratorError> {
        let Some(limit) = HistoryManager::get_auto_compact_limit(llm_config) else {
            return Ok(messages);
        };

        let history_usage = self
            .workspace
            .load_session_token_usage_async(user_id, session_id)
            .await;
        let max_context = llm_config.max_context.unwrap_or(0) as i64;
        let mut ratio = llm_config
            .history_compaction_ratio
            .unwrap_or(COMPACTION_HISTORY_RATIO as f32) as f64;
        if ratio <= 0.0 {
            ratio = COMPACTION_HISTORY_RATIO;
        } else if ratio > 1.0 {
            ratio = if ratio <= 100.0 { ratio / 100.0 } else { 1.0 };
        }
        let history_threshold = if max_context > 0 {
            Some((max_context as f64 * ratio) as i64)
        } else {
            None
        };
        let should_compact_by_history = history_threshold
            .map(|threshold| history_usage >= threshold)
            .unwrap_or(false);
        let total_tokens = estimate_messages_tokens(&messages);
        if !should_compact_by_history && total_tokens <= limit {
            return Ok(messages);
        }

        let reset_mode = if should_compact_by_history {
            let mode = llm_config
                .history_compaction_reset
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_lowercase();
            if matches!(mode.as_str(), "zero" | "current" | "keep") {
                mode
            } else {
                "zero".to_string()
            }
        } else {
            String::new()
        };

        let summary_text = if should_compact_by_history {
            i18n::t("compaction.reason.history_threshold")
        } else {
            i18n::t("compaction.reason.context_too_long")
        };
        emitter
            .emit(
                "progress",
                json!({ "stage": "compacting", "summary": summary_text }),
            )
            .await;

        let system_message = messages
            .first()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
            .cloned();
        let current_user_index = Self::locate_current_user_index(&messages);
        let current_user_message = current_user_index
            .and_then(|index| messages.get(index))
            .cloned();
        let mut source_messages: Vec<Value> = Vec::new();
        for (index, message) in messages.iter().enumerate() {
            if system_message.is_some() && index == 0 {
                continue;
            }
            if current_user_index.is_some() && Some(index) == current_user_index {
                continue;
            }
            source_messages.push(message.clone());
        }

        let mut artifact_prefixes = i18n::get_known_prefixes("history.artifact_prefix");
        if artifact_prefixes.is_empty() {
            artifact_prefixes.push(i18n::t("history.artifact_prefix"));
        }
        let has_artifact = source_messages.iter().any(|message| {
            let Some(obj) = message.as_object() else {
                return false;
            };
            if obj.get("role").and_then(Value::as_str) != Some("system") {
                return false;
            }
            let content = obj.get("content").and_then(Value::as_str).unwrap_or("");
            artifact_prefixes
                .iter()
                .any(|prefix| content.trim().starts_with(prefix))
        });
        if !has_artifact {
            let history_manager = HistoryManager;
            let artifact_content =
                history_manager.load_artifact_index_message(&self.workspace, user_id, session_id);
            if !artifact_content.is_empty() {
                source_messages.push(json!({ "role": "system", "content": artifact_content }));
            }
        }

        let user_content = self.build_compaction_user_content(&source_messages);
        if user_content.trim().is_empty() {
            if should_compact_by_history && reset_mode != "keep" {
                self.workspace
                    .save_session_token_usage_async(user_id, session_id, 0)
                    .await;
            }
            emitter
                .emit(
                    "compaction",
                    json!({
                        "reason": if should_compact_by_history { "history" } else { "overflow" },
                        "status": "skipped",
                        "skip_reason": "no_candidates",
                        "history_usage": history_usage,
                        "history_threshold": history_threshold,
                        "limit": limit,
                        "total_tokens": total_tokens,
                    }),
                )
                .await;
            return Ok(messages);
        }

        let compaction_prompt = HistoryManager::load_compaction_prompt();
        let mut summary_input = vec![
            json!({ "role": "system", "content": compaction_prompt }),
            json!({ "role": "user", "content": user_content }),
        ];
        summary_input =
            self.prepare_summary_messages(summary_input, COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
        if estimate_messages_tokens(&summary_input) > limit && summary_input.len() > 1 {
            let system_tokens = estimate_message_tokens(&summary_input[0]);
            let remaining = (limit - system_tokens).max(1);
            let tail = trim_messages_to_budget(summary_input.get(1..).unwrap_or(&[]), remaining);
            summary_input = vec![summary_input[0].clone()];
            summary_input.extend(tail);
        }

        let mut compacted_until_ts: Option<f64> = None;
        let mut compacted_until: Option<String> = None;
        let mut current_question_ts: Option<f64> = None;
        let mut skipped_question = false;
        let question_text = current_question.trim();
        let history = self
            .workspace
            .load_history_async(user_id, session_id, config.workspace.max_history_items)
            .await
            .unwrap_or_default();
        let (history_items, _) = HistoryManager::build_compaction_candidates(&history);
        let mut boundary_item: Option<Value> = None;
        for item in history_items.iter().rev() {
            if !skipped_question && !question_text.is_empty() {
                let role = item.get("role").and_then(Value::as_str).unwrap_or("");
                let content = item.get("content").and_then(Value::as_str).unwrap_or("");
                if role == "user" && content.trim() == question_text {
                    skipped_question = true;
                    current_question_ts = HistoryManager::get_item_timestamp(item);
                    continue;
                }
            }
            boundary_item = Some(item.clone());
            break;
        }
        if let Some(boundary_item) = boundary_item {
            compacted_until_ts = HistoryManager::get_item_timestamp(&boundary_item);
            compacted_until = boundary_item
                .get("timestamp")
                .and_then(Value::as_str)
                .map(|value| value.to_string());
        }

        let mut summary_config = llm_config.clone();
        let max_output = llm_config
            .max_output
            .unwrap_or(COMPACTION_SUMMARY_MAX_OUTPUT as u32)
            .min(COMPACTION_SUMMARY_MAX_OUTPUT as u32);
        summary_config.max_output = Some(max_output);
        summary_config.max_rounds = Some(1);

        let payload_messages = self.sanitize_messages_for_log(summary_input.clone(), None);
        let payload = build_llm_client(&summary_config, self.http.clone())
            .build_request_payload(&self.build_chat_messages(&payload_messages), false);
        emitter
            .emit(
                "llm_request",
                json!({
                    "provider": summary_config.provider,
                    "model": summary_config.model,
                    "base_url": summary_config.base_url,
                    "payload": payload,
                    "purpose": "compaction_summary",
                }),
            )
            .await;

        let mut summary_fallback = false;
        let summary_text = match self
            .call_llm(
                llm_config,
                &summary_input,
                emitter,
                session_id,
                false,
                0,
                false,
                Some(summary_config),
            )
            .await
        {
            Ok((content, _, _)) => self.resolve_final_answer(&content),
            Err(_) => {
                summary_fallback = true;
                i18n::t("compaction.summary_fallback")
            }
        };
        let summary_text = HistoryManager::format_compaction_summary(&summary_text);
        emitter
            .emit(
                "llm_response",
                json!({
                    "content": summary_text,
                    "reasoning": "",
                    "purpose": "compaction_summary",
                }),
            )
            .await;

        let mut meta = serde_json::Map::new();
        meta.insert(
            "type".to_string(),
            Value::String(COMPACTION_META_TYPE.to_string()),
        );
        if let Some(value) = compacted_until_ts {
            meta.insert("compacted_until_ts".to_string(), json!(value));
        }
        if let Some(value) = compacted_until.clone() {
            meta.insert("compacted_until".to_string(), Value::String(value));
        }
        let meta_value = Value::Object(meta);
        self.append_chat(
            user_id,
            session_id,
            "system",
            Some(&Value::String(summary_text.clone())),
            Some(&meta_value),
            None,
        );

        if skipped_question && !question_text.is_empty() {
            let should_reappend = compacted_until_ts.is_none()
                || current_question_ts.is_none()
                || current_question_ts <= compacted_until_ts;
            if should_reappend {
                let question_value = Value::String(question_text.to_string());
                self.append_chat(
                    user_id,
                    session_id,
                    "user",
                    Some(&question_value),
                    None,
                    None,
                );
            }
        }

        let mut rebuilt = Vec::new();
        if let Some(system_message) = system_message {
            rebuilt.push(system_message);
        }
        rebuilt.push(json!({ "role": "user", "content": summary_text }));
        if let Some(current_user_message) = current_user_message {
            rebuilt.push(current_user_message);
        } else if !question_text.is_empty() {
            rebuilt.push(json!({ "role": "user", "content": question_text }));
        }
        let rebuilt = self.shrink_messages_to_limit(rebuilt, limit);
        let rebuilt_tokens = estimate_messages_tokens(&rebuilt);
        if should_compact_by_history && reset_mode != "keep" {
            if reset_mode == "current" {
                self.workspace
                    .save_session_token_usage_async(user_id, session_id, rebuilt_tokens)
                    .await;
            } else {
                self.workspace
                    .save_session_token_usage_async(user_id, session_id, 0)
                    .await;
            }
        }

        emitter
            .emit(
                "compaction",
                json!({
                    "reason": if should_compact_by_history { "history" } else { "overflow" },
                    "status": if summary_fallback { "fallback" } else { "done" },
                    "summary_fallback": summary_fallback,
                    "summary_tokens": approx_token_count(&summary_text),
                    "total_tokens": total_tokens,
                    "total_tokens_after": rebuilt_tokens,
                    "history_usage": history_usage,
                    "history_threshold": history_threshold,
                    "limit": limit,
                }),
            )
            .await;

        Ok(rebuilt)
    }

    fn resolve_allowed_tool_names(
        &self,
        config: &Config,
        requested: &[String],
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
    ) -> HashSet<String> {
        let is_default = requested.is_empty();
        let allowed = if is_default {
            collect_available_tool_names(config, skills, user_tool_bindings)
        } else {
            self.prompt_composer.resolve_allowed_tool_names(
                config,
                skills,
                requested,
                user_tool_bindings,
            )
        };
        self.apply_a2ui_tool_policy(allowed, is_default)
    }

    fn apply_a2ui_tool_policy(
        &self,
        mut allowed_tool_names: HashSet<String>,
        default_mode: bool,
    ) -> HashSet<String> {
        if default_mode {
            allowed_tool_names.remove("a2ui");
        }
        if allowed_tool_names.contains("a2ui") {
            allowed_tool_names.remove("最终回复");
            allowed_tool_names.remove("final_response");
            allowed_tool_names.remove(&resolve_tool_name("final_response"));
        }
        allowed_tool_names
    }

    async fn build_system_prompt_with_allowed(
        &self,
        config: &Config,
        config_overrides: Option<&Value>,
        allowed_tool_names: &HashSet<String>,
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
        user_id: &str,
    ) -> String {
        let workdir = self
            .workspace
            .ensure_user_root(user_id)
            .unwrap_or_else(|_| self.workspace.root().to_path_buf());
        let config_version = self.config_store.version();
        self.prompt_composer
            .build_system_prompt_cached(
                config,
                config_version,
                &self.workspace,
                user_id,
                &workdir,
                config_overrides,
                allowed_tool_names,
                skills,
                user_tool_bindings,
            )
            .await
    }

    async fn resolve_session_prompt(
        &self,
        user_id: &str,
        session_id: &str,
        prompt: String,
        tool_names: Option<&Vec<String>>,
        overrides: Option<&Value>,
        language: Option<&str>,
    ) -> String {
        let stored = self
            .workspace
            .load_session_system_prompt_async(user_id, session_id, language)
            .await
            .unwrap_or(None);
        if stored.is_some() && tool_names.is_none() && overrides.is_none() {
            return stored.unwrap_or(prompt);
        }
        if stored.is_none() {
            let _ = self
                .workspace
                .save_session_system_prompt(user_id, session_id, &prompt, language);
        }
        prompt
    }

    async fn append_memory_prompt(&self, user_id: &str, prompt: String) -> String {
        if prompt.trim().is_empty() {
            return prompt;
        }
        if !self.memory_store.is_enabled_async(user_id).await {
            return prompt;
        }
        let records = self
            .memory_store
            .list_records_async(user_id, None, false)
            .await;
        let block = self.memory_store.build_prompt_block(&records);
        if block.is_empty() {
            return prompt;
        }
        format!("{}\n\n{}", prompt.trim_end(), block)
    }

    fn load_memory_summary_prompt(&self) -> String {
        let prompt = read_prompt_template(Path::new(MEMORY_SUMMARY_PROMPT_PATH))
            .trim()
            .to_string();
        if prompt.is_empty() {
            i18n::t("memory.summary_prompt_fallback")
        } else {
            prompt
        }
    }

    fn trim_attachments_for_memory(
        &self,
        attachments: Option<&[AttachmentPayload]>,
    ) -> Option<Vec<AttachmentPayload>> {
        let Some(attachments) = attachments else {
            return None;
        };
        if attachments.is_empty() {
            return None;
        }
        Some(
            attachments
                .iter()
                .map(|item| AttachmentPayload {
                    name: item.name.clone(),
                    content: None,
                    content_type: item.content_type.clone(),
                })
                .collect(),
        )
    }

    fn format_memory_task(&self, task: &MemorySummaryTask, now_ts: f64) -> Value {
        let queued_ts = task.queued_time.max(0.0);
        let start_ts = task.start_time.max(0.0);
        let end_ts = task.end_time.max(0.0);
        let mut status = task.status.trim().to_string();
        if status.is_empty() {
            status = if end_ts > 0.0 {
                i18n::t("memory.status.done")
            } else if start_ts > 0.0 {
                i18n::t("memory.status.running")
            } else {
                i18n::t("memory.status.queued")
            };
        } else {
            let normalized = match status.to_lowercase().as_str() {
                "queued" | "排队中" => Some("queued"),
                "running" | "processing" | "正在处理" => Some("running"),
                "done" | "completed" | "已完成" => Some("done"),
                "failed" | "失败" => Some("failed"),
                _ => None,
            };
            if let Some(normalized) = normalized {
                status = match normalized {
                    "queued" => i18n::t("memory.status.queued"),
                    "running" => i18n::t("memory.status.running"),
                    "done" => i18n::t("memory.status.done"),
                    "failed" => i18n::t("memory.status.failed"),
                    _ => status,
                };
            }
        }

        fn format_ts(ts: f64) -> String {
            if ts <= 0.0 {
                return String::new();
            }
            Utc.timestamp_opt(ts as i64, 0)
                .single()
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default()
        }

        let elapsed_s = if end_ts > 0.0 {
            let base_ts = if start_ts > 0.0 { start_ts } else { queued_ts };
            if base_ts > 0.0 {
                (end_ts - base_ts).max(0.0)
            } else {
                0.0
            }
        } else if start_ts > 0.0 {
            (now_ts - start_ts).max(0.0)
        } else if queued_ts > 0.0 {
            (now_ts - queued_ts).max(0.0)
        } else {
            0.0
        };

        json!({
            "task_id": task.task_id,
            "user_id": task.user_id,
            "session_id": task.session_id,
            "status": status,
            "queued_time": format_ts(queued_ts),
            "queued_time_ts": queued_ts,
            "started_time": format_ts(start_ts),
            "started_time_ts": start_ts,
            "finished_time": format_ts(end_ts),
            "finished_time_ts": end_ts,
            "elapsed_s": elapsed_s,
        })
    }

    async fn find_memory_task(&self, task_id: &str) -> Option<MemorySummaryTask> {
        let state = self.memory_queue.state.lock().await;
        if let Some(active) = &state.active {
            if active.task_id == task_id {
                return Some(active.clone());
            }
        }
        for item in state.queue.iter() {
            if item.task.task_id == task_id {
                return Some(item.task.clone());
            }
        }
        for task in state.history.iter() {
            if task.task_id == task_id {
                return Some(task.clone());
            }
        }
        None
    }

    async fn ensure_memory_worker(&self) {
        let mut state = self.memory_queue.state.lock().await;
        let should_spawn = state
            .worker
            .as_ref()
            .map(|handle| handle.is_finished())
            .unwrap_or(true);
        if !should_spawn {
            return;
        }
        let orchestrator = self.clone();
        state.worker = Some(tokio::spawn(async move {
            orchestrator.memory_worker_loop().await;
        }));
    }

    async fn enqueue_memory_summary(
        &self,
        prepared: &PreparedRequest,
        request_messages: Option<Vec<Value>>,
        final_answer: &str,
    ) {
        if !self.memory_store.is_enabled_async(&prepared.user_id).await {
            return;
        }
        self.ensure_memory_worker().await;

        let task = MemorySummaryTask {
            task_id: Uuid::new_v4().simple().to_string(),
            user_id: prepared.user_id.clone(),
            session_id: prepared.session_id.clone(),
            queued_time: now_ts(),
            config_overrides: prepared.config_overrides.clone(),
            model_name: prepared.model_name.clone(),
            attachments: self.trim_attachments_for_memory(prepared.attachments.as_deref()),
            request_messages,
            language: prepared.language.clone(),
            status: "queued".to_string(),
            start_time: 0.0,
            end_time: 0.0,
            request_payload: None,
            final_answer: final_answer.trim().to_string(),
            summary_result: String::new(),
            error: String::new(),
        };

        {
            let mut state = self.memory_queue.state.lock().await;
            state.seq = state.seq.saturating_add(1);
            let seq = state.seq;
            state.queue.push(MemoryQueueItem {
                queued_time: task.queued_time,
                seq,
                task,
            });
        }
        self.memory_queue.notify.notify_one();
    }

    async fn memory_worker_loop(self) {
        loop {
            let mut task = loop {
                let next = {
                    let mut state = self.memory_queue.state.lock().await;
                    state.queue.pop().map(|item| item.task)
                };
                match next {
                    Some(task) => break task,
                    None => self.memory_queue.notify.notified().await,
                }
            };

            let stored = i18n::with_language(task.language.clone(), async {
                task.start_time = now_ts();
                task.status = "running".to_string();
                {
                    let mut state = self.memory_queue.state.lock().await;
                    state.active = Some(task.clone());
                }

                match self.run_memory_summary_task(&mut task).await {
                    Ok(stored) => {
                        task.status = "done".to_string();
                        stored
                    }
                    Err(err) => {
                        task.status = "failed".to_string();
                        task.error = err.to_string();
                        warn!("记忆总结任务失败: {}", err);
                        false
                    }
                }
            })
            .await;

            task.end_time = now_ts();
            {
                let mut state = self.memory_queue.state.lock().await;
                state.active = None;
                state.history.push_front(task.clone());
                while state.history.len() > 100 {
                    state.history.pop_back();
                }
            }

            if stored {
                let base_ts = if task.start_time > 0.0 {
                    task.start_time
                } else {
                    task.queued_time
                };
                let elapsed_s = if base_ts > 0.0 && task.end_time > 0.0 {
                    (task.end_time - base_ts).max(0.0)
                } else {
                    0.0
                };
                self.memory_store
                    .upsert_task_log_async(
                        &task.user_id,
                        &task.session_id,
                        &task.task_id,
                        &task.status,
                        task.queued_time,
                        task.start_time,
                        task.end_time,
                        elapsed_s,
                        task.request_payload.as_ref(),
                        &task.summary_result,
                        &task.error,
                        Some(task.end_time),
                    )
                    .await;
            }
        }
    }

    async fn run_memory_summary_task(
        &self,
        task: &mut MemorySummaryTask,
    ) -> Result<bool, OrchestratorError> {
        if !self.memory_store.is_enabled_async(&task.user_id).await {
            return Ok(false);
        }
        let config = self.resolve_config(task.config_overrides.as_ref()).await;
        let (llm_name, llm_config) =
            self.resolve_llm_config(&config, task.model_name.as_deref())?;
        let mut summary_config = llm_config.clone();
        let max_output = summary_config.max_output.unwrap_or(0);
        if max_output == 0 || max_output as i64 > COMPACTION_SUMMARY_MAX_OUTPUT {
            summary_config.max_output = Some(COMPACTION_SUMMARY_MAX_OUTPUT as u32);
        }
        summary_config.max_rounds = Some(1);

        let messages = self
            .build_memory_summary_messages(task, &summary_config, &config)
            .await;
        let payload_messages =
            self.sanitize_messages_for_log(messages.clone(), task.attachments.as_deref());
        task.request_payload =
            Some(self.build_memory_summary_payload(task, &llm_name, payload_messages));

        let emitter = EventEmitter::new(
            task.session_id.clone(),
            task.user_id.clone(),
            None,
            None,
            self.monitor.clone(),
        );
        let (content, _, _) = self
            .call_llm(
                &llm_config,
                &messages,
                &emitter,
                &task.session_id,
                false,
                1,
                false,
                Some(summary_config),
            )
            .await?;
        let summary_text = strip_tool_calls(&content);
        let normalized = MemoryStore::normalize_summary(&summary_text);
        task.summary_result = normalized.clone();
        Ok(self
            .memory_store
            .upsert_record_async(
                &task.user_id,
                &task.session_id,
                &normalized,
                Some(task.queued_time),
            )
            .await)
    }

    async fn build_memory_summary_request_payload(
        &self,
        task: &MemorySummaryTask,
    ) -> Result<Value, OrchestratorError> {
        i18n::with_language(task.language.clone(), async {
            let config = self.resolve_config(task.config_overrides.as_ref()).await;
            let (llm_name, llm_config) =
                self.resolve_llm_config(&config, task.model_name.as_deref())?;
            let mut summary_config = llm_config.clone();
            let max_output = summary_config.max_output.unwrap_or(0);
            if max_output == 0 || max_output as i64 > COMPACTION_SUMMARY_MAX_OUTPUT {
                summary_config.max_output = Some(COMPACTION_SUMMARY_MAX_OUTPUT as u32);
            }
            summary_config.max_rounds = Some(1);
            let messages = self
                .build_memory_summary_messages(task, &summary_config, &config)
                .await;
            let payload_messages =
                self.sanitize_messages_for_log(messages, task.attachments.as_deref());
            Ok(self.build_memory_summary_payload(task, &llm_name, payload_messages))
        })
        .await
    }

    async fn build_memory_summary_messages(
        &self,
        task: &MemorySummaryTask,
        summary_llm_config: &LlmModelConfig,
        config: &Config,
    ) -> Vec<Value> {
        let summary_instruction = self.load_memory_summary_prompt();
        let source_messages = if let Some(request_messages) = &task.request_messages {
            request_messages.clone()
        } else {
            let history_manager = HistoryManager;
            history_manager.load_history_messages(
                &self.workspace,
                &task.user_id,
                &task.session_id,
                config.workspace.max_history_items,
            )
        };
        let user_content =
            self.build_memory_summary_user_content(&source_messages, &task.final_answer);
        let mut messages = vec![
            json!({ "role": "system", "content": summary_instruction }),
            json!({ "role": "user", "content": user_content }),
        ];
        messages = self.prepare_summary_messages(messages, COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
        if let Some(limit) = HistoryManager::get_auto_compact_limit(summary_llm_config) {
            if estimate_messages_tokens(&messages) > limit && messages.len() > 1 {
                let system_tokens = estimate_message_tokens(&messages[0]);
                let remaining = (limit - system_tokens).max(1);
                let tail = trim_messages_to_budget(messages.get(1..).unwrap_or(&[]), remaining);
                messages = vec![messages[0].clone()];
                messages.extend(tail);
            }
        }
        messages
    }

    fn build_memory_summary_user_content(&self, messages: &[Value], final_answer: &str) -> String {
        let separator = i18n::t("memory.summary.role.separator");
        let user_label = i18n::t("memory.summary.role.user");
        let assistant_label = i18n::t("memory.summary.role.assistant");
        let mut lines: Vec<String> = Vec::new();
        let mut last_assistant = String::new();
        for message in messages {
            let Some(obj) = message.as_object() else {
                continue;
            };
            let role = obj.get("role").and_then(Value::as_str).unwrap_or("").trim();
            if role.is_empty() || role == "system" {
                continue;
            }
            if Self::is_observation_message(role, obj.get("content").unwrap_or(&Value::Null)) {
                continue;
            }
            let content =
                self.extract_memory_summary_text(obj.get("content").unwrap_or(&Value::Null));
            if content.is_empty() {
                continue;
            }
            let label = if role == "user" {
                user_label.as_str()
            } else if role == "assistant" {
                assistant_label.as_str()
            } else {
                role
            };
            lines.push(format!("{label}{separator}{content}"));
            if role == "assistant" {
                last_assistant = content;
            }
        }
        let final_text = final_answer.trim();
        if !final_text.is_empty() && final_text != last_assistant {
            lines.push(format!("{assistant_label}{separator}{final_text}"));
        }
        lines.join("\n").trim().to_string()
    }

    fn build_compaction_user_content(&self, messages: &[Value]) -> String {
        let separator = i18n::t("memory.summary.role.separator");
        let user_label = i18n::t("memory.summary.role.user");
        let assistant_label = i18n::t("memory.summary.role.assistant");
        let mut lines: Vec<String> = Vec::new();
        for message in messages {
            let Some(obj) = message.as_object() else {
                continue;
            };
            let role = obj.get("role").and_then(Value::as_str).unwrap_or("").trim();
            if role.is_empty() {
                continue;
            }
            let content =
                self.extract_memory_summary_text(obj.get("content").unwrap_or(&Value::Null));
            if content.is_empty() {
                continue;
            }
            let label = if role == "user" {
                user_label.as_str()
            } else if role == "assistant" {
                assistant_label.as_str()
            } else {
                role
            };
            lines.push(format!("{label}{separator}{content}"));
        }
        lines.join("\n").trim().to_string()
    }

    fn extract_memory_summary_text(&self, content: &Value) -> String {
        match content {
            Value::Null => String::new(),
            Value::String(text) => strip_tool_calls(text).trim().to_string(),
            Value::Array(parts) => {
                let mut segments: Vec<String> = Vec::new();
                for part in parts {
                    let Some(obj) = part.as_object() else {
                        continue;
                    };
                    let part_type = obj
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_lowercase();
                    if part_type == "text" {
                        let text = obj.get("text").and_then(Value::as_str).unwrap_or("");
                        let cleaned = strip_tool_calls(text).trim().to_string();
                        if !cleaned.is_empty() {
                            segments.push(cleaned);
                        }
                        continue;
                    }
                    if part_type == "image_url" || obj.contains_key("image_url") {
                        segments.push(i18n::t("memory.summary.image_placeholder"));
                    }
                }
                segments.join("\n").trim().to_string()
            }
            other => strip_tool_calls(&other.to_string()).trim().to_string(),
        }
    }

    fn is_observation_message(role: &str, content: &Value) -> bool {
        if role != "user" {
            return false;
        }
        let Value::String(text) = content else {
            return false;
        };
        text.starts_with(OBSERVATION_PREFIX)
    }

    fn build_memory_summary_payload(
        &self,
        task: &MemorySummaryTask,
        llm_name: &str,
        messages: Vec<Value>,
    ) -> Value {
        let mut payload = json!({
            "user_id": task.user_id,
            "session_id": task.session_id,
            "model_name": llm_name,
            "tool_names": [],
            "messages": messages,
        });
        if let Some(overrides) = &task.config_overrides {
            if let Value::Object(ref mut map) = payload {
                map.insert("config_overrides".to_string(), overrides.clone());
            }
        }
        payload
    }

    fn build_user_message(
        &self,
        question: &str,
        attachments: Option<&[AttachmentPayload]>,
    ) -> Value {
        let Some(attachments) = attachments else {
            return json!({ "role": "user", "content": question });
        };
        if attachments.is_empty() {
            return json!({ "role": "user", "content": question });
        }
        let attachment_label = i18n::t("attachment.label");
        let attachment_separator = i18n::t("attachment.label.separator");
        let attachment_default_name = i18n::t("attachment.default_name");
        let mut attachment_parts: Vec<String> = Vec::new();
        let mut image_parts: Vec<Value> = Vec::new();
        for attachment in attachments {
            let content = attachment.content.as_deref().unwrap_or("");
            if content.trim().is_empty() {
                continue;
            }
            let name = attachment
                .name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(&attachment_default_name);
            let display_name = Self::display_attachment_name(name);
            if is_image_attachment(attachment, content) {
                image_parts.push(json!({
                    "type": "image_url",
                    "image_url": { "url": content }
                }));
                continue;
            }
            attachment_parts.push(format!(
                "[{attachment_label}{attachment_separator}{display_name}]\n{content}"
            ));
        }
        let mut text_content = String::new();
        if !attachment_parts.is_empty() {
            text_content.push_str(&attachment_parts.join("\n\n"));
        }
        if !question.is_empty() {
            if !text_content.is_empty() {
                text_content.push_str("\n\n");
            }
            text_content.push_str(question);
        }
        if !image_parts.is_empty() {
            let text_payload = if text_content.trim().is_empty() {
                i18n::t("attachment.image_prompt")
            } else {
                text_content
            };
            let mut parts = vec![json!({ "type": "text", "text": text_payload })];
            parts.extend(image_parts);
            return json!({ "role": "user", "content": parts });
        }
        json!({ "role": "user", "content": text_content })
    }

    fn sanitize_messages_for_log(
        &self,
        messages: Vec<Value>,
        attachments: Option<&[AttachmentPayload]>,
    ) -> Vec<Value> {
        if messages.is_empty() {
            return messages;
        }
        let image_names = attachments
            .unwrap_or(&[])
            .iter()
            .filter(|item| is_image_attachment(item, item.content.as_deref().unwrap_or("")))
            .map(|item| {
                item.name
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or("image")
                    .to_string()
            })
            .collect::<Vec<_>>();
        let mut image_index = 0usize;
        let pattern = data_url_regex();

        let mut replace_data_url = |text: &str| {
            if !text.contains("data:image/") {
                return text.to_string();
            }
            let Some(pattern) = pattern else {
                return text.to_string();
            };
            let mut output = String::with_capacity(text.len());
            let mut last = 0usize;
            for m in pattern.find_iter(text) {
                output.push_str(&text[last..m.start()]);
                image_index += 1;
                let name = image_names
                    .get(image_index - 1)
                    .cloned()
                    .unwrap_or_else(|| format!("image-{image_index}"));
                output.push_str("attachment://");
                output.push_str(&name);
                last = m.end();
            }
            if last == 0 {
                return text.to_string();
            }
            output.push_str(&text[last..]);
            output
        };

        let mut sanitized = Vec::new();
        for message in messages {
            let Some(obj) = message.as_object() else {
                sanitized.push(message);
                continue;
            };
            let content = obj.get("content");
            if let Some(Value::String(text)) = content {
                let replaced = replace_data_url(text);
                if replaced != *text {
                    let mut new_message = obj.clone();
                    new_message.insert("content".to_string(), Value::String(replaced));
                    sanitized.push(Value::Object(new_message));
                } else {
                    sanitized.push(message);
                }
                continue;
            }
            if let Some(Value::Array(parts)) = content {
                let mut new_parts = Vec::new();
                let mut changed = false;
                for part in parts {
                    if let Some(part_obj) = part.as_object() {
                        let mut new_part = part_obj.clone();
                        let part_type = part_obj
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_lowercase();
                        if part_type == "image_url" || part_obj.contains_key("image_url") {
                            if let Some(url_value) = part_obj.get("image_url") {
                                let url = if let Some(obj) = url_value.as_object() {
                                    obj.get("url").and_then(Value::as_str)
                                } else {
                                    url_value.as_str()
                                };
                                if let Some(url) = url {
                                    if url.contains("data:image/") {
                                        let replaced = replace_data_url(url);
                                        if replaced == url {
                                            continue;
                                        }
                                        let mut image_obj = url_value.clone();
                                        if let Some(obj) = image_obj.as_object_mut() {
                                            obj.insert(
                                                "url".to_string(),
                                                Value::String(replaced.clone()),
                                            );
                                        } else {
                                            image_obj = json!({ "url": replaced });
                                        }
                                        new_part.insert("image_url".to_string(), image_obj);
                                        changed = true;
                                    }
                                }
                            }
                        }
                        if part_type == "text" {
                            if let Some(Value::String(text)) = part_obj.get("text") {
                                let replaced = replace_data_url(text);
                                if replaced != *text {
                                    new_part.insert("text".to_string(), Value::String(replaced));
                                    changed = true;
                                }
                            }
                        }
                        new_parts.push(Value::Object(new_part));
                    } else {
                        new_parts.push(part.clone());
                    }
                }
                if changed {
                    let mut new_message = obj.clone();
                    new_message.insert("content".to_string(), Value::Array(new_parts));
                    sanitized.push(Value::Object(new_message));
                } else {
                    sanitized.push(message);
                }
                continue;
            }
            sanitized.push(message);
        }
        sanitized
    }

    fn display_attachment_name(name: &str) -> &str {
        let stem = Path::new(name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(name);
        if stem.is_empty() {
            name
        } else {
            stem
        }
    }
}

fn is_image_attachment(attachment: &AttachmentPayload, content: &str) -> bool {
    let content_type = attachment
        .content_type
        .as_deref()
        .unwrap_or("")
        .to_lowercase();
    if content_type.starts_with("image") {
        return true;
    }
    if content_type.contains("image") {
        return true;
    }
    if content.starts_with("data:image/") {
        return true;
    }
    let name = attachment.name.as_deref().unwrap_or("").to_lowercase();
    matches!(
        Path::new(&name)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or(""),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
    )
}

fn merge_json(base: &mut Value, override_value: &Value) {
    match (base, override_value) {
        (Value::Object(base_map), Value::Object(override_map)) => {
            for (key, value) in override_map {
                match base_map.get_mut(key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        base_map.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base_slot, override_value) => {
            if !override_value.is_null() {
                *base_slot = override_value.clone();
            }
        }
    }
}

fn tool_call_block_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        compile_regex(
            r"(?is)<tool_call\b[^>]*>(?P<payload>.*?)</tool_call\s*>",
            "tool_call_block",
        )
    })
    .as_ref()
}

fn tool_block_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        compile_regex(
            r"(?is)<tool\b[^>]*>(?P<payload>.*?)</tool\s*>",
            "tool_block",
        )
    })
    .as_ref()
}

fn tool_open_tag_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| compile_regex(r"(?is)<(tool_call|tool)\b[^>]*>", "tool_open_tag"))
        .as_ref()
}

fn tool_close_tag_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| compile_regex(r"(?is)</(tool_call|tool)\s*>", "tool_close_tag"))
        .as_ref()
}

fn find_json_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut stack: Vec<u8> = Vec::new();
    let mut in_string = false;
    let mut escape = false;
    for index in start..bytes.len() {
        let ch = bytes[index];
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == b'\\' {
                escape = true;
                continue;
            }
            if ch == b'"' {
                in_string = false;
            }
            continue;
        }
        if ch == b'"' {
            in_string = true;
            continue;
        }
        if ch == b'{' || ch == b'[' {
            stack.push(ch);
            continue;
        }
        if ch == b'}' || ch == b']' {
            let opening = stack.pop()?;
            if opening == b'{' && ch != b'}' {
                return None;
            }
            if opening == b'[' && ch != b']' {
                return None;
            }
            if stack.is_empty() {
                return Some(index + 1);
            }
        }
    }
    None
}

fn extract_json_values(payload: &str) -> Vec<Value> {
    let bytes = payload.as_bytes();
    let mut values = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'{' && bytes[index] != b'[' {
            index += 1;
            continue;
        }
        let Some(end) = find_json_end(payload, index) else {
            index += 1;
            continue;
        };
        let Some(candidate) = payload.get(index..end) else {
            index += 1;
            continue;
        };
        if let Ok(value) = serde_json::from_str::<Value>(candidate) {
            values.push(value);
            index = end;
            continue;
        }
        index += 1;
    }
    values
}

fn normalize_tool_call(map: &serde_json::Map<String, Value>) -> Option<ToolCall> {
    let name_value = map
        .get("name")
        .or_else(|| map.get("tool"))
        .or_else(|| map.get("tool_name"))
        .or_else(|| map.get("toolName"))
        .or_else(|| map.get("function_name"))
        .or_else(|| map.get("functionName"))?;
    let name = match name_value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let name = name.trim().to_string();
    if name.is_empty() {
        return None;
    }

    let args_value = map
        .get("arguments")
        .or_else(|| map.get("args"))
        .or_else(|| map.get("parameters"))
        .or_else(|| map.get("params"))
        .or_else(|| map.get("input"))
        .or_else(|| map.get("payload"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let arguments = match args_value {
        Value::Null => json!({}),
        Value::String(text) => {
            serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({ "raw": text }))
        }
        other => other,
    };
    Some(ToolCall { name, arguments })
}

fn collect_tool_calls_from_value(value: &Value, calls: &mut Vec<ToolCall>) {
    match value {
        Value::Object(map) => {
            if let Some(call) = normalize_tool_call(map) {
                calls.push(call);
            }
            for key in [
                "tool_calls",
                "toolCalls",
                "tool_call",
                "toolCall",
                "function_call",
                "functionCall",
                "function",
            ] {
                if let Some(inner) = map.get(key) {
                    collect_tool_calls_from_value(inner, calls);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_tool_calls_from_value(item, calls);
            }
        }
        _ => {}
    }
}

fn normalize_tool_calls(value: Value) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    collect_tool_calls_from_value(&value, &mut calls);
    calls
}

fn parse_tool_calls_payload(payload: &str) -> Vec<ToolCall> {
    let payload = payload.trim();
    if payload.is_empty() {
        return Vec::new();
    }
    if let Ok(value) = serde_json::from_str::<Value>(payload) {
        return normalize_tool_calls(value);
    }
    let mut calls = Vec::new();
    for value in extract_json_values(payload) {
        calls.extend(normalize_tool_calls(value));
    }
    calls
}

fn parse_tool_calls_from_text(content: &str) -> Vec<ToolCall> {
    if content.trim().is_empty() {
        return Vec::new();
    }

    let mut calls = Vec::new();
    let mut blocks: Vec<(usize, String)> = Vec::new();
    if let Some(regex) = tool_call_block_regex() {
        for captures in regex.captures_iter(content) {
            if let Some(mat) = captures.get(0) {
                let payload = captures.name("payload").map(|m| m.as_str()).unwrap_or("");
                blocks.push((mat.start(), payload.to_string()));
            }
        }
    }
    if let Some(regex) = tool_block_regex() {
        for captures in regex.captures_iter(content) {
            if let Some(mat) = captures.get(0) {
                let payload = captures.name("payload").map(|m| m.as_str()).unwrap_or("");
                blocks.push((mat.start(), payload.to_string()));
            }
        }
    }
    blocks.sort_by_key(|(start, _)| *start);

    if !blocks.is_empty() {
        for (_, payload) in blocks {
            calls.extend(parse_tool_calls_payload(&payload));
        }
    }

    let open_matches = tool_open_tag_regex()
        .map(|regex| regex.find_iter(content).collect::<Vec<_>>())
        .unwrap_or_default();
    if !open_matches.is_empty() {
        for (index, mat) in open_matches.iter().enumerate() {
            let start = mat.end();
            let end = if index + 1 < open_matches.len() {
                open_matches[index + 1].start()
            } else {
                content.len()
            };
            let Some(payload) = content.get(start..end) else {
                continue;
            };
            calls.extend(parse_tool_calls_payload(payload));
        }
    }

    calls.extend(parse_tool_calls_payload(content));
    dedupe_tool_calls(calls)
}

fn tool_call_signature(call: &ToolCall) -> String {
    let args = serde_json::to_string(&call.arguments).unwrap_or_default();
    format!("{}|{}", call.name.trim(), args)
}

fn dedupe_tool_calls(calls: Vec<ToolCall>) -> Vec<ToolCall> {
    let mut seen = HashSet::new();
    let mut merged = Vec::new();
    for call in calls {
        let signature = tool_call_signature(&call);
        if seen.insert(signature) {
            merged.push(call);
        }
    }
    merged
}

fn collect_tool_calls_from_output(content: &str, reasoning: &str) -> Vec<ToolCall> {
    let mut calls = parse_tool_calls_from_text(content);
    calls.extend(parse_tool_calls_from_text(reasoning));
    dedupe_tool_calls(calls)
}

fn strip_tool_calls(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }
    let mut stripped = content.to_string();
    if let Some(regex) = tool_call_block_regex() {
        stripped = regex.replace_all(&stripped, "").to_string();
    }
    if let Some(regex) = tool_block_regex() {
        stripped = regex.replace_all(&stripped, "").to_string();
    }
    if let Some(regex) = tool_open_tag_regex() {
        stripped = regex.replace_all(&stripped, "").to_string();
    }
    if let Some(regex) = tool_close_tag_regex() {
        stripped = regex.replace_all(&stripped, "").to_string();
    }
    stripped.trim().to_string()
}

fn extract_file_paths(args: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    let Some(obj) = args.as_object() else {
        return paths;
    };
    if let Some(Value::Array(files)) = obj.get("files") {
        for item in files {
            let Some(file_obj) = item.as_object() else {
                continue;
            };
            let path = file_obj
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if !path.is_empty() {
                paths.push(path);
            }
        }
    }
    if let Some(path) = obj.get("path").and_then(Value::as_str) {
        let cleaned = path.trim();
        if !cleaned.is_empty() {
            paths.push(cleaned.to_string());
        }
    }
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for path in paths {
        if !seen.insert(path.clone()) {
            continue;
        }
        ordered.push(path);
    }
    ordered
}

fn normalize_compare_path(path: &Path) -> PathBuf {
    let normalized = normalize_target_path(path);
    normalize_path_for_compare(&normalized)
}

fn resolve_absolute_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        Some(path)
    } else {
        let cwd = std::env::current_dir().ok()?;
        Some(cwd.join(path))
    }
}

fn extract_command_lines(args: &Value) -> Vec<String> {
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut commands = Vec::new();
    for line in content.lines() {
        let cleaned = line.trim();
        if !cleaned.is_empty() {
            commands.push(cleaned.to_string());
        }
    }
    commands
}

fn parse_timeout_secs(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(num)) => num.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(Value::Bool(flag)) => Some(if *flag { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn data_url_regex() -> Option<&'static Regex> {
    static REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    REGEX
        .get_or_init(|| {
            compile_regex(
                r"data:image/[a-zA-Z0-9+.-]+;base64,[A-Za-z0-9+/=\r\n]+",
                "data_url",
            )
        })
        .as_ref()
}

fn compile_regex(pattern: &str, label: &str) -> Option<Regex> {
    match Regex::new(pattern) {
        Ok(regex) => Some(regex),
        Err(err) => {
            error!("invalid orchestrator regex {label}: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tool_call_closed_tag() {
        let content = r#"<tool_call>{"name":"读取文件","arguments":{"files":[{"path":"a.txt"}]}}</tool_call>"#;
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "读取文件");
        assert_eq!(
            calls[0]
                .arguments
                .get("files")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("path"))
                .and_then(Value::as_str),
            Some("a.txt")
        );
    }

    #[test]
    fn test_parse_tool_call_tool_tag_and_string_arguments() {
        let content =
            r#"<tool>{"name":"execute_command","arguments":"{\"content\":\"echo hi\"}"}</tool>"#;
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "execute_command");
        assert_eq!(
            calls[0].arguments.get("content").and_then(Value::as_str),
            Some("echo hi")
        );
    }

    #[test]
    fn test_parse_tool_call_open_tag_without_close() {
        let content = r#"<tool_call>{"name":"最终回复","arguments":{"content":"ok"}}"#;
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "最终回复");
        assert_eq!(
            calls[0].arguments.get("content").and_then(Value::as_str),
            Some("ok")
        );
    }

    #[test]
    fn test_strip_tool_calls_supports_tool_and_tool_call() {
        let content = "prefix <tool>{\"name\":\"x\",\"arguments\":{}}</tool> mid <tool_call>{\"name\":\"y\",\"arguments\":{}}</tool_call> suffix";
        assert_eq!(strip_tool_calls(content), "prefix  mid  suffix");
    }

    #[test]
    fn test_collect_tool_calls_from_reasoning() {
        let content = "no tools here";
        let reasoning =
            r#"<tool_call>{"name":"read_file","arguments":{"path":"a.txt"}}</tool_call>"#;
        let calls = collect_tool_calls_from_output(content, reasoning);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn test_collect_tool_calls_dedup() {
        let payload = r#"<tool_call>{"name":"read_file","arguments":{"path":"a.txt"}}</tool_call>"#;
        let calls = collect_tool_calls_from_output(payload, payload);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn test_parse_tool_call_json_without_tags() {
        let content = "call: {\"tool\":\"read_file\",\"arguments\":{\"path\":\"a.txt\"}}";
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(
            calls[0].arguments.get("path").and_then(Value::as_str),
            Some("a.txt")
        );
    }

    #[test]
    fn test_parse_tool_call_function_wrapper() {
        let content = r#"{"tool_calls":[{"type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a.txt\"}"}}]}"#;
        let calls = parse_tool_calls_from_text(content);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(
            calls[0].arguments.get("path").and_then(Value::as_str),
            Some("a.txt")
        );
    }
}
