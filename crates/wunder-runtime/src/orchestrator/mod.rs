// 调度引擎：负责会话锁、LLM 调用、工具执行、历史压缩与 SSE 事件流。
use crate::a2a_store::A2aStore;
use crate::config::{is_debug_log_level, Config, LlmModelConfig};
use crate::config_store::ConfigStore;
use crate::core::approval_registry::PendingApprovalRegistry;
use crate::cron::CronWakeSignal;
use crate::gateway::GatewayHub;
use crate::history::HistoryManager;
use crate::i18n;
use crate::llm::{build_llm_client, is_llm_configured, is_llm_model, ChatMessage, ToolCallMode};
use crate::lsp::LspManager;
use crate::monitor::MonitorState;
use crate::orchestrator_constants::{
    COMPACTION_FORCE_FALLBACK_LIMIT, COMPACTION_HISTORY_RATIO, COMPACTION_META_TYPE,
    COMPACTION_MIN_OBSERVATION_TOKENS, COMPACTION_REPLACEMENT_HISTORY_META_KEY,
    COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS, DEFAULT_LLM_TIMEOUT_S, DEFAULT_TOOL_PARALLELISM,
    DEFAULT_TOOL_TIMEOUT_S, MIN_TOOL_TIMEOUT_S, OBSERVATION_PREFIX, SESSION_LOCK_BUSY_RETRY_S,
    SESSION_LOCK_HEARTBEAT_S, SESSION_LOCK_POLL_INTERVAL_S, SESSION_LOCK_TTL_S,
    STREAM_EVENT_CLEANUP_INTERVAL_S, STREAM_EVENT_FETCH_LIMIT, STREAM_EVENT_PERSIST_CHARS,
    STREAM_EVENT_PERSIST_INTERVAL_MS, STREAM_EVENT_QUEUE_SIZE,
    STREAM_EVENT_RESUME_POLL_BACKOFF_AFTER, STREAM_EVENT_RESUME_POLL_BACKOFF_FACTOR,
    STREAM_EVENT_RESUME_POLL_INTERVAL_S, STREAM_EVENT_RESUME_POLL_MAX_INTERVAL_S,
    STREAM_EVENT_TTL_S, TOOL_RESULT_ARRAY_HEAD_ITEMS, TOOL_RESULT_ARRAY_TAIL_ITEMS,
    TOOL_RESULT_HEAD_CHARS, TOOL_RESULT_MAX_ARRAY_ITEMS, TOOL_RESULT_MAX_CHARS,
    TOOL_RESULT_PAGINATED_ARRAY_HEAD_ITEMS, TOOL_RESULT_PAGINATED_ARRAY_TAIL_ITEMS,
    TOOL_RESULT_PAGINATED_MAX_ARRAY_ITEMS, TOOL_RESULT_TAIL_CHARS, TOOL_RESULT_TRUNCATION_MARKER,
};
use crate::path_utils::{normalize_path_for_compare, normalize_target_path};
use crate::prompting::PromptComposer;
use crate::sandbox;
use crate::schemas::{AttachmentPayload, StreamEvent, TokenUsage, WunderRequest, WunderResponse};
use crate::services::beeroom_realtime::BeeroomRealtimeService;
use crate::services::inner_visible::InnerVisibleService;
use crate::services::tools::command_sessions::CommandSessionBroker;
use crate::skills::{load_skills, SkillRegistry};
use crate::storage::{SessionLockStatus, StorageBackend, UserTokenBalanceStatus};
use crate::token_utils::{
    approx_token_count, estimate_message_tokens, estimate_messages_tokens, trim_messages_to_budget,
    trim_text_to_chars, trim_text_to_tokens,
};
use crate::tools::{
    build_desktop_followup_user_message, build_read_image_followup_user_message, builtin_aliases,
    collect_available_tool_names, collect_prompt_tool_specs_with_language,
    filter_tool_names_by_model_capability, is_desktop_control_tool_name, is_read_image_tool_name,
    resolve_tool_name, ToolContext, ToolEventEmitter,
};
use crate::user_store::UserStore;
use crate::user_tools::{UserToolBindings, UserToolManager};
use crate::user_world::UserWorldService;
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Local, Utc};
use futures::{Stream, StreamExt};
use parking_lot::Mutex as ParkingMutex;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tracing::{error, warn};
use uuid::Uuid;

mod compaction_policy;
mod config;
pub mod constants;
mod context;
mod error;
mod event_stream;
mod execute;
mod execute_support;
mod execute_tools;
mod limiter;
mod llm;
mod memory;
mod memory_auto_extract;
mod memory_compaction_window;
mod memory_messages;
mod memory_support;
mod prompt;
mod request;
mod result_normalizer;
mod retry_governor;
mod runtime_snapshot;
mod stream_persist;
mod thread_runtime;
mod tool_calls;
mod tool_exec;
mod tool_parallel;
mod tool_result_payload;
mod turn_state;
mod types;

use context::ContextManager;
pub(crate) use error::OrchestratorError;
use event_stream::EventEmitter;
use event_stream::StreamSignal;
use limiter::RequestLimiter;
pub(crate) use stream_persist::flush_stream_event_persist_queue;
use thread_runtime::ThreadRuntimeRegistry;
use tool_calls::apply_tool_name_map;
use tool_calls::collect_tool_calls_from_output;
use tool_calls::compile_regex;
use tool_calls::strip_tool_calls;
use tool_result_payload::ToolResultPayload;
use turn_state::ActiveTurnRegistry;
use types::{PreparedRequest, RoundInfo};

#[derive(Clone)]
pub struct Orchestrator {
    config_store: ConfigStore,
    workspace: Arc<WorkspaceManager>,
    monitor: Arc<MonitorState>,
    a2a_store: Arc<A2aStore>,
    gateway: Arc<GatewayHub>,
    skills: Arc<RwLock<SkillRegistry>>,
    inner_visible: Arc<InnerVisibleService>,
    user_tool_manager: Arc<UserToolManager>,
    lsp_manager: Arc<LspManager>,
    prompt_composer: Arc<PromptComposer>,
    storage: Arc<dyn StorageBackend>,
    approval_registry: Arc<PendingApprovalRegistry>,
    command_sessions: Arc<CommandSessionBroker>,
    active_turns: Arc<ActiveTurnRegistry>,
    thread_runtime: Arc<ThreadRuntimeRegistry>,
    user_world: Arc<UserWorldService>,
    beeroom_realtime: Arc<BeeroomRealtimeService>,
    cron_wake_signal: Option<CronWakeSignal>,
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
        inner_visible: Arc<InnerVisibleService>,
        user_tool_manager: Arc<UserToolManager>,
        lsp_manager: Arc<LspManager>,
        storage: Arc<dyn StorageBackend>,
        approval_registry: Arc<PendingApprovalRegistry>,
        command_sessions: Arc<CommandSessionBroker>,
        gateway: Arc<GatewayHub>,
        user_world: Arc<UserWorldService>,
        beeroom_realtime: Arc<BeeroomRealtimeService>,
        cron_wake_signal: Option<CronWakeSignal>,
    ) -> Self {
        Self {
            config_store,
            workspace,
            monitor,
            a2a_store,
            gateway,
            skills,
            inner_visible,
            user_tool_manager,
            lsp_manager,
            prompt_composer: Arc::new(PromptComposer::new(60.0, 256)),
            storage,
            approval_registry,
            command_sessions,
            active_turns: Arc::new(ActiveTurnRegistry::new()),
            thread_runtime: Arc::new(ThreadRuntimeRegistry::new()),
            user_world,
            beeroom_realtime,
            cron_wake_signal,
            http: reqwest::Client::new(),
        }
    }

    pub async fn resolve_session_effective_tool_names(
        &self,
        user: &crate::storage::UserAccountRecord,
        session: &crate::storage::ChatSessionRecord,
    ) -> Vec<String> {
        const TOOL_OVERRIDE_NONE: &str = "__no_tools__";
        let config = self.config_store.get().await;
        let skills = self.skills.read().await.clone();
        let bindings = self
            .user_tool_manager
            .build_bindings(&config, &skills, &user.user_id);
        let user_context = crate::user_access::UserToolContext {
            config: config.clone(),
            skills,
            bindings,
            tool_access: self
                .storage
                .get_user_tool_access(&user.user_id)
                .ok()
                .flatten(),
            org_units: self.storage.list_org_units().unwrap_or_default(),
        };
        let mut allowed = crate::user_access::compute_allowed_tool_names(user, &user_context);
        let agent_record = if let Some(agent_id) = session
            .agent_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if agent_id.eq_ignore_ascii_case("__default__")
                || agent_id.eq_ignore_ascii_case("default")
            {
                None
            } else {
                self.storage.get_user_agent_by_id(agent_id).ok().flatten()
            }
        } else {
            None
        };
        let frozen_tool_overrides = self
            .workspace
            .load_session_frozen_tool_overrides(&user.user_id, &session.session_id);
        let overrides = if !session.tool_overrides.is_empty() {
            normalize_tool_overrides_for_session(session.tool_overrides.clone(), TOOL_OVERRIDE_NONE)
        } else if let Some(snapshot) = frozen_tool_overrides {
            normalize_tool_overrides_for_session(snapshot, TOOL_OVERRIDE_NONE)
        } else {
            crate::services::agent_abilities::resolve_agent_runtime_tool_names(
                &agent_record
                    .as_ref()
                    .map(|record| record.tool_names.clone())
                    .unwrap_or_default(),
                &agent_record
                    .as_ref()
                    .map(|record| record.declared_tool_names.clone())
                    .unwrap_or_default(),
                &agent_record
                    .as_ref()
                    .map(|record| record.declared_skill_names.clone())
                    .unwrap_or_default(),
            )
        };
        let agent_defaults = crate::services::agent_abilities::resolve_agent_runtime_tool_names(
            &agent_record
                .as_ref()
                .map(|record| record.tool_names.clone())
                .unwrap_or_default(),
            &agent_record
                .as_ref()
                .map(|record| record.declared_tool_names.clone())
                .unwrap_or_default(),
            &agent_record
                .as_ref()
                .map(|record| record.declared_skill_names.clone())
                .unwrap_or_default(),
        );
        allowed = apply_session_tool_overrides_for_allowed(
            allowed,
            &overrides,
            &agent_defaults,
            TOOL_OVERRIDE_NONE,
        );
        let mut output = allowed.into_iter().collect::<Vec<_>>();
        output.sort();
        output
    }
}

fn normalize_tool_overrides_for_session(values: Vec<String>, none_token: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    let mut has_none = false;
    for raw in values {
        let name = raw.trim().to_string();
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        if name == none_token {
            has_none = true;
        }
        seen.insert(name.clone());
        output.push(name);
    }
    if has_none {
        vec![none_token.to_string()]
    } else {
        output
    }
}

fn resolve_override_name_with_allowed_for_session(
    raw: &str,
    allowed: &HashSet<String>,
) -> Option<String> {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return None;
    }
    if allowed.contains(cleaned) {
        return Some(cleaned.to_string());
    }
    for (index, _) in cleaned.match_indices('@') {
        let suffix = cleaned[index + 1..].trim();
        if !suffix.is_empty() && allowed.contains(suffix) {
            return Some(suffix.to_string());
        }
    }
    None
}

fn apply_session_tool_overrides_for_allowed(
    allowed: HashSet<String>,
    overrides: &[String],
    agent_defaults: &[String],
    none_token: &str,
) -> HashSet<String> {
    if overrides.is_empty() {
        return allowed;
    }
    if overrides.iter().any(|name| name == none_token) {
        return HashSet::new();
    }
    let scoped_defaults: HashSet<String> = agent_defaults
        .iter()
        .map(String::as_str)
        .filter_map(|name| resolve_override_name_with_allowed_for_session(name, &allowed))
        .collect();
    let mut filtered = HashSet::new();
    for raw in overrides {
        if let Some(mapped) = resolve_override_name_with_allowed_for_session(raw, &allowed) {
            if !scoped_defaults.is_empty() && !scoped_defaults.contains(&mapped) {
                continue;
            }
            filtered.insert(mapped);
        }
    }
    filtered
}
