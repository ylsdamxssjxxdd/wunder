use anyhow::{anyhow, Context, Result};
use axum::{extract::State, routing::post, Json, Router};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::time::sleep;
use uuid::Uuid;
use wunder_server::config::LlmModelConfig;
use wunder_server::config_store::ConfigStore;
use wunder_server::schemas::WunderRequest;
use wunder_server::state::AppState;
use wunder_server::storage::{ChatSessionRecord, UserAgentRecord, DEFAULT_HIVE_ID};

const DEFAULT_WORKERS: usize = 4;
const DEFAULT_MAX_WAIT_S: u64 = 180;
const DEFAULT_MOTHER_WAIT_S: f64 = 30.0;
const DEFAULT_POLL_MS: u64 = 120;
const MOCK_MODEL_NAME: &str = "__swarm_flow_mock__";
const MOTHER_MARKER: &str = "MOTHER_SIM_START";
const WORKER_MARKER: &str = "WORKER_SIM_TASK";
const OBSERVATION_PREFIX: &str = "tool_response:";

#[derive(Debug, Clone)]
struct SimArgs {
    workers: usize,
    max_wait_s: u64,
    mother_wait_s: f64,
    poll_ms: u64,
    keep_artifacts: bool,
    output: Option<PathBuf>,
}

impl Default for SimArgs {
    fn default() -> Self {
        Self {
            workers: DEFAULT_WORKERS,
            max_wait_s: DEFAULT_MAX_WAIT_S,
            mother_wait_s: DEFAULT_MOTHER_WAIT_S,
            poll_ms: DEFAULT_POLL_MS,
            keep_artifacts: true,
            output: None,
        }
    }
}

impl SimArgs {
    fn parse_from_env() -> Result<Self> {
        let mut args = Self::default();
        let mut iter = env::args().skip(1);
        while let Some(flag) = iter.next() {
            match flag.as_str() {
                "--help" | "-h" => {
                    Self::print_help();
                    std::process::exit(0);
                }
                "--workers" => args.workers = parse_usize_flag("--workers", iter.next())?,
                "--max-wait-s" => args.max_wait_s = parse_u64_flag("--max-wait-s", iter.next())?,
                "--mother-wait-s" => {
                    args.mother_wait_s = parse_f64_flag("--mother-wait-s", iter.next())?
                }
                "--poll-ms" => args.poll_ms = parse_u64_flag("--poll-ms", iter.next())?,
                "--cleanup" => args.keep_artifacts = false,
                "--output" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| anyhow!("--output requires a path"))?;
                    args.output = Some(PathBuf::from(value));
                }
                unknown => {
                    return Err(anyhow!(
                        "unknown flag: {unknown}. Use --help to see supported options"
                    ));
                }
            }
        }

        args.workers = args.workers.max(1);
        args.max_wait_s = args.max_wait_s.max(10);
        args.mother_wait_s = args.mother_wait_s.max(1.0);
        args.poll_ms = args.poll_ms.max(40);
        Ok(args)
    }

    fn print_help() {
        println!(
            "swarm_flow_sim: deterministic mother/worker/tool-loop simulation\n\n\
             Flags:\n\
             --workers <N>         number of worker agents (default: {DEFAULT_WORKERS})\n\
             --max-wait-s <S>      max wait for completion (default: {DEFAULT_MAX_WAIT_S})\n\
             --mother-wait-s <S>   waitSeconds passed to agent_swarm batch_send (default: {DEFAULT_MOTHER_WAIT_S})\n\
             --poll-ms <MS>        poll interval while waiting runs to settle (default: {DEFAULT_POLL_MS})\n\
             --cleanup             remove generated artifacts on exit\n\
             --output <PATH>       write report json to custom path\n\
             --help                show this message"
        );
    }
}

#[derive(Debug, Clone, Default)]
struct MockScenario {
    worker_agent_ids: Vec<String>,
    mother_wait_s: f64,
}

#[derive(Default)]
struct MockLlmState {
    scenario: RwLock<MockScenario>,
    total_calls: AtomicU64,
    mother_calls: AtomicU64,
    worker_calls: AtomicU64,
    fallback_calls: AtomicU64,
}

#[derive(Debug, Serialize)]
struct FlowReport {
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    wall_time_s: f64,
    config: FlowConfigSnapshot,
    artifacts: FlowArtifacts,
    mother_response: MotherResponse,
    llm_calls: LlmCallStats,
    session_runs: SessionRunStats,
    worker_sessions: WorkerSessionStats,
    worker_latency: WorkerLatencyStats,
    monitor_events: BTreeMap<String, usize>,
    checks: FlowChecks,
}

#[derive(Debug, Serialize)]
struct FlowConfigSnapshot {
    workers: usize,
    max_wait_s: u64,
    mother_wait_s: f64,
    poll_ms: u64,
    worker_tool_loops: usize,
}

#[derive(Debug, Serialize)]
struct FlowArtifacts {
    root: String,
    db_path: String,
    workspace_root: String,
    llm_base_url: String,
    kept: bool,
}

#[derive(Debug, Serialize)]
struct MotherResponse {
    session_id: String,
    answer: String,
    stop_reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct LlmCallStats {
    total: u64,
    mother: u64,
    worker: u64,
    fallback: u64,
}

#[derive(Debug, Serialize)]
struct SessionRunStats {
    total: usize,
    by_status: BTreeMap<String, usize>,
    peak_concurrency: usize,
}

#[derive(Debug, Serialize)]
struct WorkerSessionStats {
    expected: usize,
    created: usize,
    tool_calls_total: usize,
    tool_calls_by_name: BTreeMap<String, usize>,
    items: Vec<WorkerSessionItem>,
}

#[derive(Debug, Serialize)]
struct WorkerSessionItem {
    session_id: String,
    agent_id: String,
    run_status: String,
    tool_calls: usize,
}

#[derive(Debug, Serialize)]
struct WorkerLatencyStats {
    samples: usize,
    queue_delay_ms_p50: f64,
    queue_delay_ms_p95: f64,
    exec_ms_p50: f64,
    exec_ms_p95: f64,
    end_to_end_ms_p50: f64,
    end_to_end_ms_p95: f64,
}

#[derive(Debug, Serialize)]
struct FlowChecks {
    mother_finished: bool,
    all_worker_runs_success: bool,
    each_worker_has_two_tool_calls: bool,
    no_active_runs_left: bool,
}

#[derive(Debug, Clone)]
struct SessionRunRow {
    session_id: String,
    status: String,
    queued_time: f64,
    started_time: f64,
    finished_time: f64,
}

#[derive(Debug, Clone)]
struct WorkerSessionRow {
    session_id: String,
    agent_id: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = SimArgs::parse_from_env()?;
    let started_at = Utc::now();
    let started = Instant::now();

    let artifact_root =
        env::temp_dir().join(format!("wunder_swarm_flow_{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&artifact_root)?;
    let db_path = artifact_root.join("wunder.flow.sqlite3");
    let workspace_root = artifact_root.join("workspaces");
    fs::create_dir_all(&workspace_root)?;

    let mock_state = Arc::new(MockLlmState::default());
    let (llm_base_url, _server_addr) = start_mock_llm_server(mock_state.clone()).await?;

    let state = configure_app_state(
        db_path.to_string_lossy().to_string(),
        workspace_root.to_string_lossy().to_string(),
        llm_base_url.clone(),
        args.workers,
    )
    .await?;

    let user_id = format!("swarm_flow_user_{}", Uuid::new_v4().simple());
    let (mother_agent_id, worker_agent_ids) = seed_agents(state.as_ref(), &user_id, args.workers)?;

    {
        let mut scenario = mock_state.scenario.write();
        scenario.worker_agent_ids = worker_agent_ids.clone();
        scenario.mother_wait_s = args.mother_wait_s;
    }

    let mother_session_id = create_mother_session(state.as_ref(), &user_id, &mother_agent_id)?;

    let mother_request = WunderRequest {
        user_id: user_id.clone(),
        question: format!(
            "{MOTHER_MARKER}: parse this request, dispatch worker swarm, gather results, then finish"
        ),
        tool_names: vec!["agent_swarm".to_string()],
        skip_tool_calls: false,
        stream: false,
        debug_payload: false,
        session_id: Some(mother_session_id.clone()),
        agent_id: Some(mother_agent_id),
        model_name: Some(MOCK_MODEL_NAME.to_string()),
        language: Some("zh-CN".to_string()),
        config_overrides: None,
        agent_prompt: None,
        attachments: None,
        allow_queue: true,
        is_admin: false,
    };

    let mother_response = state.orchestrator.run(mother_request).await?;

    wait_until_no_active_runs(&db_path, &user_id, args.max_wait_s, args.poll_ms).await?;
    let _ = state.workspace.flush_writes_async().await;

    let report = build_report(
        &args,
        started_at,
        started.elapsed().as_secs_f64(),
        &artifact_root,
        &db_path,
        &workspace_root,
        &llm_base_url,
        state.as_ref(),
        &user_id,
        &mother_session_id,
        &worker_agent_ids,
        &mother_response,
        mock_state.as_ref(),
    )?;

    let default_report_path = env::temp_dir()
        .join("wunder_swarm_reports")
        .join("swarm_flow_sim_report.json");
    let report_path = args.output.clone().unwrap_or(default_report_path);
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;

    println!("[swarm_flow_sim] done");
    println!("[swarm_flow_sim] wall_time_s={:.3}", report.wall_time_s);
    println!(
        "[swarm_flow_sim] llm_calls total={} mother={} worker={} fallback={}",
        report.llm_calls.total,
        report.llm_calls.mother,
        report.llm_calls.worker,
        report.llm_calls.fallback
    );
    println!(
        "[swarm_flow_sim] session_runs total={} peak_concurrency={} status={:?}",
        report.session_runs.total,
        report.session_runs.peak_concurrency,
        report.session_runs.by_status
    );
    println!(
        "[swarm_flow_sim] workers expected={} created={} tool_calls_total={}",
        report.worker_sessions.expected,
        report.worker_sessions.created,
        report.worker_sessions.tool_calls_total
    );
    println!(
        "[swarm_flow_sim] checks mother_finished={} all_worker_runs_success={} two_tool_calls_each={} no_active_runs_left={}",
        report.checks.mother_finished,
        report.checks.all_worker_runs_success,
        report.checks.each_worker_has_two_tool_calls,
        report.checks.no_active_runs_left
    );
    println!(
        "[swarm_flow_sim] report written: {}",
        report_path.to_string_lossy()
    );
    println!(
        "[swarm_flow_sim] artifacts_root={}",
        artifact_root.to_string_lossy()
    );

    if !args.keep_artifacts {
        let _ = fs::remove_dir_all(&artifact_root);
    }

    Ok(())
}

async fn start_mock_llm_server(state: Arc<MockLlmState>) -> Result<(String, SocketAddr)> {
    let app = Router::new()
        .route("/v1/chat/completions", post(mock_chat_completions))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            eprintln!("[swarm_flow_sim] mock llm server failed: {err}");
        }
    });
    Ok((format!("http://{addr}"), addr))
}

async fn mock_chat_completions(
    State(state): State<Arc<MockLlmState>>,
    Json(payload): Json<Value>,
) -> Json<Value> {
    state.total_calls.fetch_add(1, Ordering::Relaxed);

    let messages = payload
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let first_user = first_user_message(&messages);
    let observation_payloads = collect_observation_payloads(&messages);
    let observed_tools = observed_tool_names(&observation_payloads);

    let response = if first_user.contains(MOTHER_MARKER) {
        state.mother_calls.fetch_add(1, Ordering::Relaxed);
        mother_mock_response(&state.scenario.read(), &observation_payloads)
    } else if first_user.contains(WORKER_MARKER) {
        state.worker_calls.fetch_add(1, Ordering::Relaxed);
        worker_mock_response(&observed_tools)
    } else {
        state.fallback_calls.fetch_add(1, Ordering::Relaxed);
        fallback_mock_response(&first_user)
    };

    Json(response)
}

fn mother_mock_response(scenario: &MockScenario, observation_payloads: &[Value]) -> Value {
    if observation_payloads.is_empty() {
        let tasks = scenario
            .worker_agent_ids
            .iter()
            .enumerate()
            .map(|(index, agent_id)| {
                json!({
                    "agentId": agent_id,
                    "message": format!("{WORKER_MARKER}: worker={agent_id}; execute two tool rounds"),
                    "label": format!("sim-worker-{index}"),
                    "createIfMissing": true
                })
            })
            .collect::<Vec<_>>();
        let args = json!({
            "action": "batch_send",
            "tasks": tasks,
            "waitSeconds": scenario.mother_wait_s,
            "pollIntervalSeconds": 0.2,
            "createIfMissing": true,
            "includeCurrent": false
        });
        return openai_chat_response(
            "Mother preset round-1: parsed intent, dispatching worker swarm and waiting for completion.",
            Some(vec![function_tool_call("agent_swarm", &args)]),
        );
    }

    if let Some(summary) = extract_swarm_wait_summary(observation_payloads) {
        return openai_chat_response(
            &format!(
                "Mother preset final: swarm done. success={}/{} failed={} all_finished={} elapsed_s={:.3}.",
                summary.success_total,
                summary.total,
                summary.failed_total,
                summary.all_finished,
                summary.elapsed_s
            ),
            None,
        );
    }

    openai_chat_response(
        "Mother preset final: worker outputs merged after two tool rounds per worker. Flow complete.",
        None,
    )
}

fn worker_mock_response(observed_tools: &HashSet<String>) -> Value {
    let has_list_files = observed_tools.iter().any(|name| is_list_files_tool(name));
    let has_search_content = observed_tools
        .iter()
        .any(|name| is_search_content_tool(name));

    if !has_list_files {
        let args = json!({ "path": ".", "max_depth": 2 });
        return openai_chat_response(
            "Worker preset round-1: call list_files to inspect workspace context.",
            Some(vec![function_tool_call("list_files", &args)]),
        );
    }

    if !has_search_content {
        let args = json!({
            "query": "swarm",
            "path": ".",
            "max_depth": 2,
            "max_files": 40
        });
        return openai_chat_response(
            "Worker preset round-2: call search_content based on previous tool output.",
            Some(vec![function_tool_call("search_content", &args)]),
        );
    }

    openai_chat_response(
        "Worker preset final: two tool loops completed, returning deterministic summary.",
        None,
    )
}

fn fallback_mock_response(first_user: &str) -> Value {
    let safe = if first_user.trim().is_empty() {
        "unknown-session"
    } else {
        first_user.trim()
    };
    openai_chat_response(&format!("Fallback preset response for {safe}."), None)
}

fn openai_chat_response(content: &str, tool_calls: Option<Vec<Value>>) -> Value {
    let mut message = json!({
        "role": "assistant",
        "content": content,
    });
    if let Some(tool_calls) = tool_calls.filter(|calls| !calls.is_empty()) {
        message["tool_calls"] = Value::Array(tool_calls);
    }

    json!({
        "id": format!("chatcmpl_{}", Uuid::new_v4().simple()),
        "object": "chat.completion",
        "created": Utc::now().timestamp(),
        "model": "mock-swarm-flow",
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 64,
            "completion_tokens": 32,
            "total_tokens": 96
        }
    })
}

fn function_tool_call(name: &str, args: &Value) -> Value {
    let arguments = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    json!({
        "id": format!("call_{}", Uuid::new_v4().simple()),
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments,
        }
    })
}

fn first_user_message(messages: &[Value]) -> String {
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if role != "user" {
            continue;
        }
        let content = flatten_content_text(message.get("content"));
        if content.trim_start().starts_with(OBSERVATION_PREFIX) {
            continue;
        }
        if !content.trim().is_empty() {
            return content;
        }
    }
    String::new()
}

fn flatten_content_text(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    return Some(text.to_string());
                }
                part.as_str().map(ToString::to_string)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct SwarmWaitSummary {
    total: usize,
    success_total: usize,
    failed_total: usize,
    all_finished: bool,
    elapsed_s: f64,
}

fn collect_observation_payloads(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(parse_observation_payload)
        .collect::<Vec<_>>()
}

fn parse_observation_payload(message: &Value) -> Option<Value> {
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if role != "user" && role != "tool" {
        return None;
    }

    let content = flatten_content_text(message.get("content"));
    if content.trim().is_empty() {
        return None;
    }

    if role == "tool" {
        return parse_json_payload(&content);
    }

    let trimmed = content.trim_start();
    if !trimmed.starts_with(OBSERVATION_PREFIX) {
        return None;
    }
    let payload = trimmed
        .trim_start_matches(OBSERVATION_PREFIX)
        .trim_start_matches(':')
        .trim();
    parse_json_payload(payload)
}

fn parse_json_payload(text: &str) -> Option<Value> {
    serde_json::from_str::<Value>(text).ok()
}

fn observed_tool_names(observation_payloads: &[Value]) -> HashSet<String> {
    observation_payloads
        .iter()
        .filter_map(|payload| {
            payload
                .get("tool")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(ToString::to_string)
        })
        .collect::<HashSet<_>>()
}

fn is_list_files_tool(name: &str) -> bool {
    matches!(
        name.trim(),
        "list_files" | "\u{5217}\u{51fa}\u{6587}\u{4ef6}"
    )
}

fn is_search_content_tool(name: &str) -> bool {
    matches!(
        name.trim(),
        "search_content" | "\u{641c}\u{7d22}\u{5185}\u{5bb9}"
    )
}

fn extract_swarm_wait_summary(observation_payloads: &[Value]) -> Option<SwarmWaitSummary> {
    for payload in observation_payloads.iter().rev() {
        let Some(data) = payload.get("data") else {
            continue;
        };
        let Some(wait) = data.get("wait") else {
            continue;
        };
        if !wait.is_object() {
            continue;
        }

        let total = wait
            .get("total")
            .and_then(Value::as_u64)
            .or_else(|| data.get("task_total").and_then(Value::as_u64))
            .unwrap_or(0) as usize;
        let success_total = wait
            .get("success_total")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let failed_total = wait
            .get("failed_total")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let all_finished = wait
            .get("all_finished")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let elapsed_s = wait
            .get("elapsed_s")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        return Some(SwarmWaitSummary {
            total,
            success_total,
            failed_total,
            all_finished,
            elapsed_s,
        });
    }
    None
}

async fn configure_app_state(
    db_path: String,
    workspace_root: String,
    llm_base_url: String,
    workers: usize,
) -> Result<Arc<AppState>> {
    let override_path = env::temp_dir().join(format!(
        "wunder_swarm_flow_override_{}.yaml",
        Uuid::new_v4().simple()
    ));
    let config_store = ConfigStore::new(override_path);
    let model = mock_llm_model_config(&llm_base_url)?;

    config_store
        .update(move |cfg| {
            cfg.storage.backend = "sqlite".to_string();
            cfg.storage.db_path = db_path.clone();
            cfg.workspace.root = workspace_root.clone();
            cfg.server.max_active_sessions = (workers.saturating_mul(4)).max(16);
            cfg.tools.swarm.max_parallel_tasks_per_team = workers.max(1);
            cfg.tools.swarm.max_active_team_runs = workers.max(1);
            cfg.tools.swarm.max_retry = 0;
            cfg.llm.default = MOCK_MODEL_NAME.to_string();
            cfg.llm.models.clear();
            cfg.llm
                .models
                .insert(MOCK_MODEL_NAME.to_string(), model.clone());
        })
        .await?;

    let config = config_store.get().await;
    Ok(Arc::new(AppState::new(config_store, config)?))
}

fn mock_llm_model_config(base_url: &str) -> Result<LlmModelConfig> {
    serde_json::from_value(json!({
        "provider": "openai",
        "base_url": base_url,
        "api_key": "swarm-flow-mock-key",
        "model": "mock-swarm-flow",
        "stream": false,
        "retry": 0,
        "max_rounds": 6,
        "tool_call_mode": "tool_call",
        "temperature": 0.0
    }))
    .context("failed to build mock llm model config")
}

fn seed_agents(state: &AppState, user_id: &str, workers: usize) -> Result<(String, Vec<String>)> {
    state.user_store.ensure_default_hive(user_id)?;
    let now = now_ts();

    let mother_agent_id = format!("mother_{}", Uuid::new_v4().simple());
    let mother = UserAgentRecord {
        agent_id: mother_agent_id.clone(),
        user_id: user_id.to_string(),
        hive_id: DEFAULT_HIVE_ID.to_string(),
        name: "SwarmMotherSim".to_string(),
        description: "Deterministic mother agent for flow simulation".to_string(),
        system_prompt: "Follow the simulation protocol and use agent_swarm.".to_string(),
        tool_names: vec!["agent_swarm".to_string()],
        access_level: "A".to_string(),
        is_shared: false,
        status: "active".to_string(),
        icon: None,
        sandbox_container_id: 1,
        created_at: now,
        updated_at: now,
    };
    state.user_store.upsert_user_agent(&mother)?;

    let mut workers_ids = Vec::with_capacity(workers);
    for index in 0..workers {
        let agent_id = format!("worker_{}_{}", index + 1, Uuid::new_v4().simple());
        let worker = UserAgentRecord {
            agent_id: agent_id.clone(),
            user_id: user_id.to_string(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            name: format!("SwarmWorkerSim{}", index + 1),
            description: "Deterministic worker agent for tool-loop simulation".to_string(),
            system_prompt: "Execute two tool loops then return final summary.".to_string(),
            tool_names: vec!["list_files".to_string(), "search_content".to_string()],
            access_level: "A".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: (index + 2) as i32,
            created_at: now,
            updated_at: now,
        };
        state.user_store.upsert_user_agent(&worker)?;
        workers_ids.push(agent_id);
    }

    Ok((mother_agent_id, workers_ids))
}

fn create_mother_session(state: &AppState, user_id: &str, mother_agent_id: &str) -> Result<String> {
    let now = now_ts();
    let session_id = format!("sess_mother_{}", Uuid::new_v4().simple());
    let session = ChatSessionRecord {
        session_id: session_id.clone(),
        user_id: user_id.to_string(),
        title: "SwarmFlowMotherSession".to_string(),
        created_at: now,
        updated_at: now,
        last_message_at: now,
        agent_id: Some(mother_agent_id.to_string()),
        tool_overrides: vec!["agent_swarm".to_string()],
        parent_session_id: None,
        parent_message_id: None,
        spawn_label: None,
        spawned_by: None,
    };
    state.user_store.upsert_chat_session(&session)?;
    Ok(session_id)
}

async fn wait_until_no_active_runs(
    db_path: &Path,
    user_id: &str,
    max_wait_s: u64,
    poll_ms: u64,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(max_wait_s);
    loop {
        let conn = Connection::open(db_path)
            .with_context(|| format!("failed to open sqlite db: {}", db_path.display()))?;
        let active: i64 = conn.query_row(
            "SELECT COUNT(1) FROM session_runs WHERE user_id = ? AND status IN ('queued', 'running')",
            params![user_id],
            |row| row.get(0),
        )?;
        if active <= 0 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "timed out waiting session_runs to settle, active={active}"
            ));
        }
        sleep(Duration::from_millis(poll_ms)).await;
    }
}

#[allow(clippy::too_many_arguments)]
fn build_report(
    args: &SimArgs,
    started_at: DateTime<Utc>,
    wall_time_s: f64,
    artifact_root: &Path,
    db_path: &Path,
    workspace_root: &Path,
    llm_base_url: &str,
    state: &AppState,
    user_id: &str,
    mother_session_id: &str,
    worker_agent_ids: &[String],
    mother_result: &wunder_server::schemas::WunderResponse,
    mock_state: &MockLlmState,
) -> Result<FlowReport> {
    let conn = Connection::open(db_path)
        .with_context(|| format!("failed to open sqlite db: {}", db_path.display()))?;

    let run_rows = load_session_runs(&conn, user_id)?;
    let mut run_status = BTreeMap::new();
    for row in &run_rows {
        *run_status.entry(row.status.clone()).or_insert(0) += 1;
    }

    let mut worker_session_rows =
        worker_sessions_from_mother_tool_result(state, mother_session_id, worker_agent_ids);
    if worker_session_rows.is_empty() {
        let (worker_sessions, _) =
            state
                .user_store
                .list_chat_sessions(user_id, None, Some(mother_session_id), 0, 256)?;
        worker_session_rows = worker_sessions
            .into_iter()
            .filter_map(|session| {
                let agent_id = session.agent_id?;
                if !worker_agent_ids.iter().any(|id| id == &agent_id) {
                    return None;
                }
                Some(WorkerSessionRow {
                    session_id: session.session_id,
                    agent_id,
                })
            })
            .collect::<Vec<_>>();
    }

    worker_session_rows.sort_by(|a, b| a.session_id.cmp(&b.session_id));

    let worker_session_ids = worker_session_rows
        .iter()
        .map(|session| session.session_id.clone())
        .collect::<Vec<_>>();
    let worker_status_map = latest_status_by_session(&run_rows);
    let tool_calls_by_session =
        load_tool_call_counts_by_session(&conn, user_id, &worker_session_ids)?;
    let tool_calls_by_name = load_tool_call_counts_by_name(&conn, user_id)?;

    let mut worker_items = Vec::new();
    let mut all_worker_success = true;
    let mut each_worker_two_tools = true;
    for session in &worker_session_rows {
        let session_id = session.session_id.clone();
        let status = worker_status_map
            .get(&session_id)
            .cloned()
            .unwrap_or_else(|| "missing".to_string());
        if status != "success" {
            all_worker_success = false;
        }
        let tool_calls = tool_calls_by_session.get(&session_id).copied().unwrap_or(0);
        if tool_calls != 2 {
            each_worker_two_tools = false;
        }
        worker_items.push(WorkerSessionItem {
            session_id,
            agent_id: session.agent_id.clone(),
            run_status: status,
            tool_calls,
        });
    }

    let worker_latency = build_worker_latency_stats(&run_rows, &worker_session_rows);
    let monitor_events = mother_event_counts(state, mother_session_id);

    let active_left = run_rows
        .iter()
        .any(|row| matches!(row.status.as_str(), "queued" | "running"));

    Ok(FlowReport {
        started_at,
        finished_at: Utc::now(),
        wall_time_s,
        config: FlowConfigSnapshot {
            workers: args.workers,
            max_wait_s: args.max_wait_s,
            mother_wait_s: args.mother_wait_s,
            poll_ms: args.poll_ms,
            worker_tool_loops: 2,
        },
        artifacts: FlowArtifacts {
            root: artifact_root.to_string_lossy().to_string(),
            db_path: db_path.to_string_lossy().to_string(),
            workspace_root: workspace_root.to_string_lossy().to_string(),
            llm_base_url: llm_base_url.to_string(),
            kept: args.keep_artifacts,
        },
        mother_response: MotherResponse {
            session_id: mother_result.session_id.clone(),
            answer: mother_result.answer.clone(),
            stop_reason: mother_result.stop_reason.clone(),
        },
        llm_calls: LlmCallStats {
            total: mock_state.total_calls.load(Ordering::Relaxed),
            mother: mock_state.mother_calls.load(Ordering::Relaxed),
            worker: mock_state.worker_calls.load(Ordering::Relaxed),
            fallback: mock_state.fallback_calls.load(Ordering::Relaxed),
        },
        session_runs: SessionRunStats {
            total: run_rows.len(),
            by_status: run_status,
            peak_concurrency: compute_peak_concurrency(&run_rows),
        },
        worker_sessions: WorkerSessionStats {
            expected: args.workers,
            created: worker_items.len(),
            tool_calls_total: tool_calls_by_session.values().sum(),
            tool_calls_by_name,
            items: worker_items,
        },
        worker_latency,
        monitor_events,
        checks: FlowChecks {
            mother_finished: !mother_result.answer.trim().is_empty(),
            all_worker_runs_success: all_worker_success,
            each_worker_has_two_tool_calls: each_worker_two_tools,
            no_active_runs_left: !active_left,
        },
    })
}

fn load_session_runs(conn: &Connection, user_id: &str) -> Result<Vec<SessionRunRow>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, status, COALESCE(queued_time, 0), COALESCE(started_time, 0), COALESCE(finished_time, 0) \
         FROM session_runs WHERE user_id = ? ORDER BY queued_time ASC",
    )?;
    let rows = stmt.query_map(params![user_id], |row| {
        Ok(SessionRunRow {
            session_id: row.get(0)?,
            status: normalize_status(row.get::<_, String>(1)?),
            queued_time: row.get(2)?,
            started_time: row.get(3)?,
            finished_time: row.get(4)?,
        })
    })?;
    let mut output = Vec::new();
    for row in rows {
        output.push(row?);
    }
    Ok(output)
}

fn latest_status_by_session(rows: &[SessionRunRow]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for row in rows {
        map.insert(row.session_id.clone(), row.status.clone());
    }
    map
}

fn load_tool_call_counts_by_session(
    conn: &Connection,
    user_id: &str,
    session_ids: &[String],
) -> Result<HashMap<String, usize>> {
    let mut output = HashMap::new();
    let mut stmt =
        conn.prepare("SELECT COUNT(1) FROM tool_logs WHERE user_id = ? AND session_id = ?")?;
    for session_id in session_ids {
        let count: i64 = stmt.query_row(params![user_id, session_id], |row| row.get(0))?;
        output.insert(session_id.clone(), count.max(0) as usize);
    }
    Ok(output)
}

fn load_tool_call_counts_by_name(
    conn: &Connection,
    user_id: &str,
) -> Result<BTreeMap<String, usize>> {
    let mut stmt = conn.prepare(
        "SELECT tool, COUNT(1) FROM tool_logs WHERE user_id = ? GROUP BY tool ORDER BY tool ASC",
    )?;
    let rows = stmt.query_map(params![user_id], |row| {
        let name: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        Ok((name, count.max(0) as usize))
    })?;
    let mut output = BTreeMap::new();
    for row in rows {
        let (name, count) = row?;
        output.insert(name, count);
    }
    Ok(output)
}

fn build_worker_latency_stats(
    run_rows: &[SessionRunRow],
    worker_session_rows: &[WorkerSessionRow],
) -> WorkerLatencyStats {
    let session_ids = worker_session_rows
        .iter()
        .map(|row| row.session_id.as_str())
        .collect::<HashSet<_>>();
    let mut queue_delay_ms = Vec::new();
    let mut exec_ms = Vec::new();
    let mut end_to_end_ms = Vec::new();

    for row in run_rows {
        if !session_ids.contains(row.session_id.as_str()) {
            continue;
        }
        if row.queued_time > 0.0 && row.started_time >= row.queued_time {
            queue_delay_ms.push((row.started_time - row.queued_time) * 1000.0);
        }
        if row.started_time > 0.0 && row.finished_time >= row.started_time {
            exec_ms.push((row.finished_time - row.started_time) * 1000.0);
        }
        if row.queued_time > 0.0 && row.finished_time >= row.queued_time {
            end_to_end_ms.push((row.finished_time - row.queued_time) * 1000.0);
        }
    }

    WorkerLatencyStats {
        samples: queue_delay_ms
            .len()
            .max(exec_ms.len())
            .max(end_to_end_ms.len()),
        queue_delay_ms_p50: percentile_ms(&queue_delay_ms, 50.0),
        queue_delay_ms_p95: percentile_ms(&queue_delay_ms, 95.0),
        exec_ms_p50: percentile_ms(&exec_ms, 50.0),
        exec_ms_p95: percentile_ms(&exec_ms, 95.0),
        end_to_end_ms_p50: percentile_ms(&end_to_end_ms, 50.0),
        end_to_end_ms_p95: percentile_ms(&end_to_end_ms, 95.0),
    }
}

fn percentile_ms(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let ratio = percentile.clamp(0.0, 100.0) / 100.0;
    let index = ((sorted.len() - 1) as f64 * ratio).round() as usize;
    round_millis(sorted[index])
}

fn round_millis(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn worker_sessions_from_mother_tool_result(
    state: &AppState,
    mother_session_id: &str,
    worker_agent_ids: &[String],
) -> Vec<WorkerSessionRow> {
    let Some(record) = state.monitor.get_record(mother_session_id) else {
        return Vec::new();
    };
    let Some(events) = record.get("events").and_then(Value::as_array) else {
        return Vec::new();
    };

    let expected_agents = worker_agent_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();

    for event in events.iter().rev() {
        if event.get("type").and_then(Value::as_str) != Some("tool_result") {
            continue;
        }
        let Some(items) = event
            .get("data")
            .and_then(|payload| payload.get("data"))
            .and_then(|payload| payload.get("items"))
            .and_then(Value::as_array)
        else {
            continue;
        };

        let mut rows = Vec::new();
        let mut seen_sessions = HashSet::new();
        for item in items {
            let Some(agent_id) = item
                .get("agent_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            if !expected_agents.contains(agent_id) {
                continue;
            }
            let Some(session_id) = item
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            if !seen_sessions.insert(session_id.to_string()) {
                continue;
            }
            rows.push(WorkerSessionRow {
                session_id: session_id.to_string(),
                agent_id: agent_id.to_string(),
            });
        }
        if !rows.is_empty() {
            return rows;
        }
    }

    Vec::new()
}

fn mother_event_counts(state: &AppState, mother_session_id: &str) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    let Some(record) = state.monitor.get_record(mother_session_id) else {
        return counts;
    };
    let Some(events) = record.get("events").and_then(Value::as_array) else {
        return counts;
    };
    for event in events {
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .trim()
            .to_string();
        if kind.is_empty() {
            continue;
        }
        *counts.entry(kind).or_insert(0) += 1;
    }
    counts
}

fn compute_peak_concurrency(rows: &[SessionRunRow]) -> usize {
    let mut events = Vec::<(f64, i32)>::new();
    for row in rows {
        if row.started_time <= 0.0 || row.finished_time <= 0.0 {
            continue;
        }
        if row.finished_time < row.started_time {
            continue;
        }
        events.push((row.started_time, 1));
        events.push((row.finished_time, -1));
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut current = 0i32;
    let mut peak = 0i32;
    for (_, delta) in events {
        current += delta;
        if current > peak {
            peak = current;
        }
    }
    peak.max(0) as usize
}

fn normalize_status(status: String) -> String {
    status.trim().to_ascii_lowercase()
}

fn parse_usize_flag(flag: &str, value: Option<String>) -> Result<usize> {
    let raw = value.ok_or_else(|| anyhow!("{flag} requires a value"))?;
    raw.parse::<usize>()
        .map_err(|err| anyhow!("invalid value for {flag}: {err}"))
}

fn parse_u64_flag(flag: &str, value: Option<String>) -> Result<u64> {
    let raw = value.ok_or_else(|| anyhow!("{flag} requires a value"))?;
    raw.parse::<u64>()
        .map_err(|err| anyhow!("invalid value for {flag}: {err}"))
}

fn parse_f64_flag(flag: &str, value: Option<String>) -> Result<f64> {
    let raw = value.ok_or_else(|| anyhow!("{flag} requires a value"))?;
    raw.parse::<f64>()
        .map_err(|err| anyhow!("invalid value for {flag}: {err}"))
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}
