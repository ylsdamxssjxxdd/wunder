// 调度引擎：负责会话锁、LLM 调用、工具执行、历史压缩与 SSE 事件流。
use crate::a2a_store::A2aStore;
use crate::config::{is_debug_log_level, Config, LlmModelConfig};
use crate::config_store::ConfigStore;
use crate::gateway::GatewayHub;
use crate::history::HistoryManager;
use crate::i18n;
use crate::llm::{
    build_llm_client, is_llm_configured, is_llm_model, normalize_tool_call_mode, ChatMessage,
    ToolCallMode,
};
use crate::lsp::LspManager;
use crate::memory::MemoryStore;
use crate::monitor::MonitorState;
use crate::orchestrator_constants::{
    COMPACTION_HISTORY_RATIO, COMPACTION_META_TYPE, COMPACTION_MIN_OBSERVATION_TOKENS,
    COMPACTION_SUMMARY_MAX_OUTPUT, COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS, DEFAULT_LLM_TIMEOUT_S,
    DEFAULT_TOOL_PARALLELISM, DEFAULT_TOOL_TIMEOUT_S, MIN_TOOL_TIMEOUT_S, OBSERVATION_PREFIX,
    SESSION_LOCK_HEARTBEAT_S, SESSION_LOCK_POLL_INTERVAL_S, SESSION_LOCK_TTL_S,
    STREAM_EVENT_CLEANUP_INTERVAL_S, STREAM_EVENT_FETCH_LIMIT, STREAM_EVENT_PERSIST_CHARS,
    STREAM_EVENT_PERSIST_INTERVAL_MS, STREAM_EVENT_POLL_INTERVAL_S, STREAM_EVENT_QUEUE_SIZE,
    STREAM_EVENT_TTL_S, TOOL_RESULT_HEAD_CHARS, TOOL_RESULT_MAX_CHARS, TOOL_RESULT_TAIL_CHARS,
    TOOL_RESULT_TRUNCATION_MARKER,
};
use crate::path_utils::{normalize_path_for_compare, normalize_target_path};
use crate::prompting::{read_prompt_template, PromptComposer};
use crate::sandbox;
use crate::schemas::{AttachmentPayload, StreamEvent, TokenUsage, WunderRequest, WunderResponse};
use crate::skills::{load_skills, SkillRegistry};
use crate::storage::{SessionLockStatus, StorageBackend, UserQuotaStatus};
use crate::token_utils::{
    approx_token_count, estimate_message_tokens, estimate_messages_tokens, trim_messages_to_budget,
    trim_text_to_tokens,
};
use crate::tools::{
    builtin_aliases, collect_available_tool_names, collect_prompt_tool_specs_with_language,
    resolve_tool_name, ToolContext, ToolEventEmitter,
};
use crate::user_store::UserStore;
use crate::user_tools::{UserToolBindings, UserToolManager};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Local, TimeZone, Utc};
use futures::{Stream, StreamExt};
use parking_lot::Mutex as ParkingMutex;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, Notify, RwLock};
use tokio::task::JoinHandle;
use tracing::{error, warn};
use uuid::Uuid;

mod config;
pub mod constants;
mod context;
mod error;
mod event_stream;
mod execute;
mod limiter;
mod llm;
mod memory;
mod prompt;
mod request;
mod tool_calls;
mod tool_exec;
mod types;

use context::ContextManager;
pub(crate) use error::OrchestratorError;
use event_stream::now_ts;
use event_stream::EventEmitter;
use event_stream::StreamSignal;
use limiter::RequestLimiter;
use memory::MemoryQueue;
use tool_calls::apply_tool_name_map;
use tool_calls::collect_tool_calls_from_output;
use tool_calls::compile_regex;
use tool_calls::strip_tool_calls;
use tool_exec::ToolResultPayload;
use types::{PreparedRequest, RoundInfo};

#[derive(Clone)]
pub struct Orchestrator {
    config_store: ConfigStore,
    workspace: Arc<WorkspaceManager>,
    monitor: Arc<MonitorState>,
    a2a_store: Arc<A2aStore>,
    gateway: Arc<GatewayHub>,
    skills: Arc<RwLock<SkillRegistry>>,
    user_tool_manager: Arc<UserToolManager>,
    lsp_manager: Arc<LspManager>,
    prompt_composer: Arc<PromptComposer>,
    storage: Arc<dyn StorageBackend>,
    memory_store: Arc<MemoryStore>,
    memory_queue: Arc<MemoryQueue>,
    http: reqwest::Client,
}

impl Orchestrator {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config_store: ConfigStore,
        _config: Config,
        workspace: Arc<WorkspaceManager>,
        monitor: Arc<MonitorState>,
        a2a_store: Arc<A2aStore>,
        skills: Arc<RwLock<SkillRegistry>>,
        user_tool_manager: Arc<UserToolManager>,
        lsp_manager: Arc<LspManager>,
        storage: Arc<dyn StorageBackend>,
        gateway: Arc<GatewayHub>,
    ) -> Self {
        let memory_store = Arc::new(MemoryStore::new(storage.clone()));
        Self {
            config_store,
            workspace,
            monitor,
            a2a_store,
            gateway,
            skills,
            user_tool_manager,
            lsp_manager,
            prompt_composer: Arc::new(PromptComposer::new(60.0, 256)),
            storage,
            memory_store,
            memory_queue: Arc::new(MemoryQueue::new()),
            http: reqwest::Client::new(),
        }
    }
}
