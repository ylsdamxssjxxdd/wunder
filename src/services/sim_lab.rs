use crate::schemas::WunderRequest;
use crate::state::AppState;
use crate::storage::{
    ChatSessionRecord, UserAgentRecord, DEFAULT_HIVE_ID, MAX_SANDBOX_CONTAINER_ID,
    MIN_SANDBOX_CONTAINER_ID,
};
use anyhow::{anyhow, Result};
use axum::{extract::State, routing::post, Json, Router};
use chrono::{DateTime, Utc};
use futures::stream::{FuturesUnordered, StreamExt};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use uuid::Uuid;

const DEFAULT_WORKERS: usize = 100;
const DEFAULT_MAX_WAIT_S: u64 = 180;
const DEFAULT_MOTHER_WAIT_S: f64 = 30.0;
const DEFAULT_POLL_MS: u64 = 120;
const DEFAULT_WORKER_TASK_ROUNDS: usize = 3;
const MAX_WORKER_TASK_ROUNDS: usize = 20;
const MOCK_MODEL_NAME: &str = "__swarm_flow_mock__";
const MOTHER_MARKER: &str = "MOTHER_SIM_START";
const WORKER_MARKER: &str = "WORKER_SIM_TASK";
const OBSERVATION_PREFIX: &str = "tool_response:";
const PROJECT_SWARM_FLOW: &str = "swarm_flow";
const SIM_USER_ID: &str = "wunder-sim";
const SIM_USER_PASSWORD: &str = "wunder-sim-password";
const SIM_MOTHER_AGENT_ID: &str = "wunder_sim_mother";

#[derive(Default)]
struct SimRunControl {
    cancel_requested: AtomicBool,
    user_id: Mutex<Option<String>>,
    mother_session_id: Mutex<Option<String>>,
}

static SIM_RUN_REGISTRY: OnceLock<Mutex<HashMap<String, Arc<SimRunControl>>>> = OnceLock::new();

fn sim_run_registry() -> &'static Mutex<HashMap<String, Arc<SimRunControl>>> {
    SIM_RUN_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimLabRunRequest {
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub options: Option<Value>,
    #[serde(default)]
    pub keep_artifacts: Option<bool>,
    #[serde(default)]
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimLabProject {
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub defaults: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimLabRunReport {
    pub run_id: String,
    pub mode: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub wall_time_s: f64,
    pub project_total: usize,
    pub project_success: usize,
    pub project_failed: usize,
    pub projects: Vec<SimLabProjectReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimLabProjectReport {
    pub project_id: String,
    pub status: String,
    pub wall_time_s: f64,
    pub report: Option<Value>,
    pub error: Option<String>,
}

pub fn list_projects() -> Vec<SimLabProject> {
    vec![SimLabProject {
        project_id: PROJECT_SWARM_FLOW.to_string(),
        title: "Swarm Chain Test".to_string(),
        description: "Seeded mother/worker/tool-loop simulation for swarm chain validation using in-process mock model and real orchestrator pipeline, with multi-round randomized worker tasks.".to_string(),
        defaults: json!({
            "workers": DEFAULT_WORKERS,
            "max_wait_s": DEFAULT_MAX_WAIT_S,
            "mother_wait_s": DEFAULT_MOTHER_WAIT_S,
            "poll_ms": DEFAULT_POLL_MS,
            "worker_task_rounds": DEFAULT_WORKER_TASK_ROUNDS,
            "keep_artifacts": false
        }),
    }]
}

pub async fn run_sim_lab(
    state: Arc<AppState>,
    request: SimLabRunRequest,
) -> Result<SimLabRunReport> {
    let run_id = normalize_run_id(request.run_id.as_deref())
        .unwrap_or_else(|| format!("simlab_{}", Uuid::new_v4().simple()));
    let run_control = Arc::new(SimRunControl::default());
    {
        let mut registry = sim_run_registry().lock();
        if registry.contains_key(&run_id) {
            return Err(anyhow!("sim run id already exists: {run_id}"));
        }
        registry.insert(run_id.clone(), run_control.clone());
    }

    let run_result = async {
        let mut projects = normalize_project_list(&request.projects);
        if projects.is_empty() {
            projects.push(PROJECT_SWARM_FLOW.to_string());
        }

        let mode = "parallel".to_string();
        let options_by_project = extract_options_by_project(request.options.as_ref());
        let keep_artifacts = request.keep_artifacts.unwrap_or(false);
        let started_at = Utc::now();
        let started = Instant::now();
        let run_seed = splitmix64(stable_string_hash64(&run_id));

        let mut tasks = FuturesUnordered::new();
        for project_id in projects {
            let state = state.clone();
            let run_control = run_control.clone();
            let project_options = options_by_project.get(&project_id).cloned();
            let project_seed = splitmix64(run_seed ^ stable_string_hash64(&project_id));
            tasks.push(async move {
                run_project(
                    state,
                    run_control,
                    &project_id,
                    project_options.as_ref(),
                    keep_artifacts,
                    project_seed,
                )
                .await
            });
        }

        let mut reports = Vec::new();
        while let Some(item) = tasks.next().await {
            reports.push(item);
        }

        reports.sort_by(|left, right| left.project_id.cmp(&right.project_id));
        let project_success = reports
            .iter()
            .filter(|item| item.status == "success")
            .count();
        let project_failed = reports.len().saturating_sub(project_success);

        Ok(SimLabRunReport {
            run_id: run_id.clone(),
            mode,
            started_at,
            finished_at: Utc::now(),
            wall_time_s: started.elapsed().as_secs_f64(),
            project_total: reports.len(),
            project_success,
            project_failed,
            projects: reports,
        })
    }
    .await;

    sim_run_registry().lock().remove(&run_id);
    run_result
}

fn normalize_run_id(raw: Option<&str>) -> Option<String> {
    let mut output = String::new();
    for ch in raw?.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            output.push(ch.to_ascii_lowercase());
        }
    }
    if output.is_empty() {
        None
    } else {
        Some(output)
    }
}

fn cancelled_error() -> anyhow::Error {
    anyhow!("simulation cancelled")
}

fn is_cancelled_error(err: &anyhow::Error) -> bool {
    err.to_string()
        .to_ascii_lowercase()
        .contains("simulation cancelled")
}

fn apply_cancel_signal(state: &AppState, run_control: &SimRunControl) {
    if !run_control.cancel_requested.load(Ordering::Relaxed) {
        return;
    }

    if let Some(session_id) = run_control.mother_session_id.lock().clone() {
        let _ = state.monitor.cancel(&session_id);
    }

    let user_id = run_control.user_id.lock().clone();
    let Some(user_id) = user_id else {
        return;
    };

    let active_sessions = state.monitor.list_sessions(true);
    for session in active_sessions {
        let same_user = session
            .get("user_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .map(|value| value == user_id)
            .unwrap_or(false);
        if !same_user {
            continue;
        }
        let Some(session_id) = session
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let _ = state.monitor.cancel(session_id);
    }
}

pub fn cancel_run(state: &AppState, run_id: &str) -> Result<bool> {
    let Some(run_id) = normalize_run_id(Some(run_id)) else {
        return Ok(false);
    };

    let run_control = sim_run_registry().lock().get(&run_id).cloned();
    let Some(run_control) = run_control else {
        return Ok(false);
    };

    run_control.cancel_requested.store(true, Ordering::Relaxed);
    apply_cancel_signal(state, run_control.as_ref());
    Ok(true)
}

pub fn is_run_active(run_id: &str) -> bool {
    normalize_run_id(Some(run_id))
        .map(|cleaned| sim_run_registry().lock().contains_key(&cleaned))
        .unwrap_or(false)
}

fn normalize_project_list(items: &[String]) -> Vec<String> {
    let mut values = Vec::new();
    let mut seen = HashSet::new();
    for item in items {
        let project_id = item.trim().to_ascii_lowercase();
        if project_id.is_empty() || !seen.insert(project_id.clone()) {
            continue;
        }
        values.push(project_id);
    }
    values
}

fn extract_options_by_project(root: Option<&Value>) -> HashMap<String, Value> {
    let Some(map) = root.and_then(Value::as_object) else {
        return HashMap::new();
    };
    map.iter()
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.clone()))
        .filter(|(key, _)| !key.is_empty())
        .collect::<HashMap<_, _>>()
}

async fn run_project(
    state: Arc<AppState>,
    run_control: Arc<SimRunControl>,
    project_id: &str,
    options: Option<&Value>,
    keep_artifacts: bool,
    run_seed: u64,
) -> SimLabProjectReport {
    let started = Instant::now();
    let result = match project_id {
        PROJECT_SWARM_FLOW => {
            let args = SimArgs::from_json(options, keep_artifacts);
            run_swarm_flow(state, args, run_control, run_seed)
                .await
                .map(|report| serde_json::to_value(report).unwrap_or(Value::Null))
        }
        _ => Err(anyhow!("unsupported sim project: {project_id}")),
    };

    match result {
        Ok(report) => SimLabProjectReport {
            project_id: project_id.to_string(),
            status: "success".to_string(),
            wall_time_s: started.elapsed().as_secs_f64(),
            report: Some(report),
            error: None,
        },
        Err(err) => SimLabProjectReport {
            project_id: project_id.to_string(),
            status: if is_cancelled_error(&err) {
                "cancelled".to_string()
            } else {
                "failed".to_string()
            },
            wall_time_s: started.elapsed().as_secs_f64(),
            report: None,
            error: Some(err.to_string()),
        },
    }
}

#[derive(Debug, Clone)]
struct SimArgs {
    workers: usize,
    max_wait_s: u64,
    mother_wait_s: f64,
    poll_ms: u64,
    worker_task_rounds: usize,
    keep_artifacts: bool,
}

impl Default for SimArgs {
    fn default() -> Self {
        Self {
            workers: DEFAULT_WORKERS,
            max_wait_s: DEFAULT_MAX_WAIT_S,
            mother_wait_s: DEFAULT_MOTHER_WAIT_S,
            poll_ms: DEFAULT_POLL_MS,
            worker_task_rounds: DEFAULT_WORKER_TASK_ROUNDS,
            keep_artifacts: false,
        }
    }
}

impl SimArgs {
    fn from_json(raw: Option<&Value>, keep_artifacts: bool) -> Self {
        let mut args = Self::default();
        let Some(map) = raw.and_then(Value::as_object) else {
            args.keep_artifacts = keep_artifacts;
            return args;
        };

        args.workers = read_u64(map, "workers")
            .map(|value| value as usize)
            .unwrap_or(DEFAULT_WORKERS)
            .max(1);
        args.max_wait_s = read_u64(map, "max_wait_s")
            .unwrap_or(DEFAULT_MAX_WAIT_S)
            .max(10);
        args.mother_wait_s = read_f64(map, "mother_wait_s")
            .unwrap_or(DEFAULT_MOTHER_WAIT_S)
            .max(1.0);
        args.poll_ms = read_u64(map, "poll_ms").unwrap_or(DEFAULT_POLL_MS).max(40);
        args.worker_task_rounds = read_u64(map, "worker_task_rounds")
            .map(|value| value as usize)
            .unwrap_or(DEFAULT_WORKER_TASK_ROUNDS)
            .clamp(1, MAX_WORKER_TASK_ROUNDS);
        args.keep_artifacts = read_bool(map, "keep_artifacts").unwrap_or(keep_artifacts);
        args
    }
}

fn read_u64(map: &Map<String, Value>, key: &str) -> Option<u64> {
    map.get(key).and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|signed| u64::try_from(signed).ok()))
    })
}

fn read_f64(map: &Map<String, Value>, key: &str) -> Option<f64> {
    map.get(key).and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_i64().map(|signed| signed as f64))
            .or_else(|| value.as_u64().map(|unsigned| unsigned as f64))
    })
}

fn read_bool(map: &Map<String, Value>, key: &str) -> Option<bool> {
    map.get(key).and_then(Value::as_bool)
}

#[derive(Debug, Clone, Default)]
struct MockScenario {
    worker_agent_ids: Vec<String>,
    mother_wait_s: f64,
    worker_task_rounds: usize,
    run_seed: u64,
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
    worker_task_rounds: usize,
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
    peak_concurrency_workers: usize,
    peak_concurrency_all: usize,
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
    each_worker_has_expected_tool_calls: bool,
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

async fn run_swarm_flow(
    state: Arc<AppState>,
    args: SimArgs,
    run_control: Arc<SimRunControl>,
    run_seed: u64,
) -> Result<FlowReport> {
    let started_at = Utc::now();
    let started = Instant::now();

    let artifact_root = env::temp_dir().join(format!(
        "wunder_swarm_flow_shared_{}",
        Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&artifact_root)?;

    let mock_state = Arc::new(MockLlmState::default());
    let (llm_base_url, _server_addr, mock_server_task) =
        start_mock_llm_server(mock_state.clone()).await?;

    let config_snapshot = state.config_store.get().await;
    let db_path = config_snapshot.storage.db_path.clone();
    let workspace_root = config_snapshot.workspace.root.clone();
    let artifact_root_display = artifact_root.to_string_lossy().to_string();

    let run_result = async {
        let user_id = SIM_USER_ID.to_string();
        *run_control.user_id.lock() = Some(user_id.clone());

        ensure_swarm_sim_user(state.as_ref(), &user_id)?;
        reset_swarm_sim_user_runtime(state.as_ref(), &user_id)?;
        let (mother_agent_id, worker_agent_ids) =
            seed_swarm_agents(state.as_ref(), &user_id, args.workers)?;

        {
            let mut scenario = mock_state.scenario.write();
            scenario.worker_agent_ids = worker_agent_ids.clone();
            scenario.mother_wait_s = args.mother_wait_s;
            scenario.worker_task_rounds = args.worker_task_rounds;
            scenario.run_seed = run_seed;
        }

        let mother_session_id = create_mother_session(state.as_ref(), &user_id, &mother_agent_id)?;
        *run_control.mother_session_id.lock() = Some(mother_session_id.clone());
        seed_swarm_workspace(state.as_ref(), &user_id)?;
        let config_overrides = build_mock_request_overrides(&args, &llm_base_url);

        apply_cancel_signal(state.as_ref(), run_control.as_ref());

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
            agent_id: Some(mother_agent_id.clone()),
            model_name: Some(MOCK_MODEL_NAME.to_string()),
            language: Some("zh-CN".to_string()),
            config_overrides: Some(config_overrides),
            agent_prompt: None,
            attachments: None,
            allow_queue: true,
            is_admin: false,
            approval_tx: None,
        };

        if run_control.cancel_requested.load(Ordering::Relaxed) {
            return Err(cancelled_error());
        }

        let mother_response = state.orchestrator.run(mother_request).await?;

        wait_until_no_active_runs(
            state.as_ref(),
            run_control.as_ref(),
            &user_id,
            args.max_wait_s,
            args.poll_ms,
        )
        .await?;
        let _ = state.workspace.flush_writes_async().await;

        let report = build_report(
            &args,
            started_at,
            started.elapsed().as_secs_f64(),
            &artifact_root_display,
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

        Ok(report)
    }
    .await;

    mock_server_task.abort();
    if !args.keep_artifacts {
        let _ = fs::remove_dir_all(&artifact_root);
    }

    run_result
}

fn build_mock_request_overrides(args: &SimArgs, llm_base_url: &str) -> Value {
    let max_rounds = args.worker_task_rounds.saturating_add(4).clamp(6, 64);
    json!({
        "server": {
            "max_active_sessions": (args.workers.saturating_mul(4)).max(16)
        },
        "tools": {
            "swarm": {
                "max_parallel_tasks_per_team": args.workers.max(1),
                "max_active_team_runs": args.workers.max(1),
                "max_retry": 0
            }
        },
        "llm": {
            "default": MOCK_MODEL_NAME,
            "models": {
                MOCK_MODEL_NAME: {
                    "provider": "openai",
                    "base_url": llm_base_url,
                    "api_key": "swarm-flow-mock-key",
                    "model": "mock-swarm-flow",
                    "stream": false,
                    "retry": 0,
                    "max_rounds": max_rounds,
                    "tool_call_mode": "tool_call",
                    "temperature": 0.0
                }
            }
        }
    })
}

fn ensure_swarm_sim_user(state: &AppState, user_id: &str) -> Result<()> {
    if let Some(mut user) = state.user_store.get_user_by_id(user_id)? {
        let mut changed = false;
        if !user.status.trim().eq_ignore_ascii_case("active") {
            user.status = "active".to_string();
            changed = true;
        }
        if !user.roles.iter().any(|role| role == "user") {
            user.roles.push("user".to_string());
            changed = true;
        }
        if user.access_level.trim().is_empty() {
            user.access_level = "A".to_string();
            changed = true;
        }
        if changed {
            user.updated_at = now_ts();
            state.user_store.update_user(&user)?;
        }
    } else {
        let _ = state.user_store.create_user(
            user_id,
            None,
            SIM_USER_PASSWORD,
            Some("A"),
            None,
            vec!["user".to_string()],
            "active",
            false,
        )?;
    }
    state.user_store.ensure_default_hive(user_id)?;
    Ok(())
}

fn reset_swarm_sim_user_runtime(state: &AppState, user_id: &str) -> Result<()> {
    state.monitor.purge_user_sessions(user_id);
    let _ = state.workspace.purge_user_data(user_id);

    let (sessions, _) = state
        .user_store
        .list_chat_sessions(user_id, None, None, 0, 4096)?;
    for session in sessions {
        let _ = state
            .storage
            .delete_chat_session(user_id, &session.session_id);
        let _ = state.storage.delete_monitor_record(&session.session_id);
    }

    let _ = state.storage.delete_monitor_records_by_user(user_id);
    let _ = state.storage.delete_session_locks_by_user(user_id);
    let _ = state.storage.delete_stream_events_by_user(user_id);

    let agents = state.user_store.list_user_agents(user_id)?;
    for agent in agents {
        let _ = state.user_store.delete_user_agent(user_id, &agent.agent_id);
        let _ = state
            .user_store
            .delete_agent_thread(user_id, &agent.agent_id);
    }

    Ok(())
}

async fn start_mock_llm_server(
    state: Arc<MockLlmState>,
) -> Result<(String, SocketAddr, JoinHandle<()>)> {
    let app = Router::new()
        .route("/v1/chat/completions", post(mock_chat_completions))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            eprintln!("[swarm_flow_sim] mock llm server failed: {err}");
        }
    });
    Ok((format!("http://{addr}"), addr, handle))
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
    let user_message = last_user_message(&messages);
    let observation_payloads = collect_observation_payloads(&messages);
    let current_observation_payloads = collect_current_observation_payloads(&messages);
    let observed_tools = observed_tool_names(&current_observation_payloads);

    let response = if user_message.contains(MOTHER_MARKER) {
        state.mother_calls.fetch_add(1, Ordering::Relaxed);
        mother_mock_response(&state.scenario.read(), &observation_payloads)
    } else if user_message.contains(WORKER_MARKER) {
        state.worker_calls.fetch_add(1, Ordering::Relaxed);
        worker_mock_response(&state.scenario.read(), &user_message, &observed_tools)
    } else {
        state.fallback_calls.fetch_add(1, Ordering::Relaxed);
        fallback_mock_response(&user_message)
    };

    Json(response)
}

fn mother_mock_response(scenario: &MockScenario, observation_payloads: &[Value]) -> Value {
    let total_rounds = scenario.worker_task_rounds.max(1);
    let summaries = extract_swarm_wait_summaries(observation_payloads);
    let completed_rounds = summaries.len();

    if completed_rounds < total_rounds {
        if completed_rounds > 0 {
            if let Some(summary) = summaries.last() {
                if !summary.all_finished {
                    return openai_chat_response(
                        &format!(
                            "Mother preset final: swarm round {completed_rounds}/{total_rounds} incomplete. success={}/{} failed={} all_finished={} elapsed_s={:.3}.",
                            summary.success_total,
                            summary.total,
                            summary.failed_total,
                            summary.all_finished,
                            summary.elapsed_s
                        ),
                        None,
                    );
                }
            }
        }

        let round = completed_rounds + 1;
        let tasks = scenario
            .worker_agent_ids
            .iter()
            .enumerate()
            .map(|(index, agent_id)| {
                let mission = choose_worker_mission(scenario.run_seed, agent_id, round);
                json!({
                    "agentId": agent_id,
                    "message": format!("{WORKER_MARKER}: worker={agent_id}; task_round={round}/{total_rounds}; mission={mission}"),
                    "label": format!("sim-worker-{index}-r{round}"),
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
            &format!(
                "Mother preset round-{round}/{total_rounds}: dispatching worker swarm and waiting for completion."
            ),
            Some(vec![function_tool_call("agent_swarm", &args)]),
        );
    }

    if let Some(summary) = summaries
        .last()
        .copied()
        .or_else(|| extract_swarm_wait_summary(observation_payloads))
    {
        return openai_chat_response(
            &format!(
                "Mother preset final: swarm rounds done {completed_rounds}/{total_rounds}. last_round_success={}/{} last_round_failed={} last_round_all_finished={} last_round_elapsed_s={:.3}.",
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
        &format!(
            "Mother preset final: worker outputs merged after {total_rounds} rounds per worker. Flow complete."
        ),
        None,
    )
}

#[derive(Debug, Clone, Copy)]
enum WorkerToolKind {
    ListFiles,
    SearchContent,
    ReadFile,
    WriteFile,
    ReplaceText,
    EditFile,
    ExecuteCommand,
    ProgrammaticToolCall,
    FinalResponse,
}

impl WorkerToolKind {
    fn function_name(self) -> &'static str {
        match self {
            Self::ListFiles => "list_files",
            Self::SearchContent => "search_content",
            Self::ReadFile => "read_file",
            Self::WriteFile => "write_file",
            Self::ReplaceText => "replace_text",
            Self::EditFile => "edit_file",
            Self::ExecuteCommand => "execute_command",
            Self::ProgrammaticToolCall => "programmatic_tool_call",
            Self::FinalResponse => "final_response",
        }
    }

    fn observed_by(self, observed_tools: &HashSet<String>) -> bool {
        observed_tools.iter().any(|tool| self.matches_tool(tool))
    }

    fn matches_tool(self, tool_name: &str) -> bool {
        match self {
            Self::ListFiles => is_list_files_tool(tool_name),
            Self::SearchContent => is_search_content_tool(tool_name),
            Self::ReadFile => is_read_file_tool(tool_name),
            Self::WriteFile => is_write_file_tool(tool_name),
            Self::ReplaceText => is_replace_text_tool(tool_name),
            Self::EditFile => is_edit_file_tool(tool_name),
            Self::ExecuteCommand => is_execute_command_tool(tool_name),
            Self::ProgrammaticToolCall => is_ptc_tool(tool_name),
            Self::FinalResponse => is_final_response_tool(tool_name),
        }
    }

    fn args(self, worker_tag: &str, task_round: usize, task_seed: u64) -> Value {
        let suffix = format!("{:03}", task_seed % 1000);
        let worker_file = format!("sim_lab/workers/{worker_tag}_r{task_round}_{suffix}.txt");
        match self {
            Self::ListFiles => json!({
                "path": ".",
                "max_depth": 1 + (task_seed % 3),
            }),
            Self::SearchContent => json!({
                "query": choose_search_query(task_seed),
                "path": ".",
                "max_depth": 2,
                "max_files": 40,
            }),
            Self::ReadFile => json!({
                "files": [{
                    "path": "sim_lab/shared/context.txt",
                    "start_line": 1,
                    "end_line": 10 + (task_seed % 31),
                }]
            }),
            Self::WriteFile => json!({
                "path": worker_file,
                "content": format!("worker={worker_tag}\nphase=alpha\nresult=pending\n"),
            }),
            Self::ReplaceText => json!({
                "path": worker_file,
                "old_string": "phase=alpha",
                "new_string": "phase=beta",
                "expected_replacements": 1,
            }),
            Self::EditFile => json!({
                "path": worker_file,
                "edits": [{
                    "action": "replace",
                    "start_line": 3,
                    "end_line": 3,
                    "new_content": format!("result=edited;r{task_round};{suffix}"),
                }],
                "ensure_newline_at_eof": true,
            }),
            Self::ExecuteCommand => json!({
                "content": format!("echo swarm-worker-{worker_tag}-r{task_round}-{suffix}"),
                "workdir": ".",
                "timeout_s": 5,
            }),
            Self::ProgrammaticToolCall => json!({
                "filename": format!("sim_worker_{worker_tag}_r{task_round}_{suffix}.py"),
                "workdir": ".",
                "content": format!("print('swarm worker script prepared r{task_round} {suffix}')"),
            }),
            Self::FinalResponse => json!({
                "content": format!("worker {worker_tag} final response via tool r{task_round} {suffix}"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum WorkerToolProfile {
    InspectSearch,
    InspectRead,
    WriteReplace,
    WriteEdit,
    CommandRead,
    SearchFinal,
    PtcSearch,
    CommandPtc,
}

impl WorkerToolProfile {
    fn label(self) -> &'static str {
        match self {
            Self::InspectSearch => "inspect_search",
            Self::InspectRead => "inspect_read",
            Self::WriteReplace => "write_replace",
            Self::WriteEdit => "write_edit",
            Self::CommandRead => "command_read",
            Self::SearchFinal => "search_final",
            Self::PtcSearch => "ptc_search",
            Self::CommandPtc => "command_ptc",
        }
    }

    fn round_tools(self) -> (WorkerToolKind, WorkerToolKind) {
        match self {
            Self::InspectSearch => (WorkerToolKind::ListFiles, WorkerToolKind::SearchContent),
            Self::InspectRead => (WorkerToolKind::ListFiles, WorkerToolKind::ReadFile),
            Self::WriteReplace => (WorkerToolKind::WriteFile, WorkerToolKind::ReplaceText),
            Self::WriteEdit => (WorkerToolKind::WriteFile, WorkerToolKind::EditFile),
            Self::CommandRead => (WorkerToolKind::ExecuteCommand, WorkerToolKind::ReadFile),
            Self::SearchFinal => (WorkerToolKind::SearchContent, WorkerToolKind::FinalResponse),
            Self::PtcSearch => (
                WorkerToolKind::ProgrammaticToolCall,
                WorkerToolKind::SearchContent,
            ),
            Self::CommandPtc => (
                WorkerToolKind::ExecuteCommand,
                WorkerToolKind::ProgrammaticToolCall,
            ),
        }
    }
}

fn worker_mock_response(
    scenario: &MockScenario,
    user_message: &str,
    observed_tools: &HashSet<String>,
) -> Value {
    let worker_tag = worker_tag(user_message);
    let task_round = extract_task_round(user_message);
    let task_seed = derive_worker_task_seed(scenario.run_seed, &worker_tag, task_round);
    let profile = choose_worker_profile(task_seed);
    let (round_one_tool, round_two_tool) = profile.round_tools();

    if !round_one_tool.observed_by(observed_tools) {
        let args = round_one_tool.args(&worker_tag, task_round, task_seed);
        return openai_chat_response(
            &format!(
                "Worker preset task {task_round}/{} round-1 ({}) call {}.",
                scenario.worker_task_rounds.max(1),
                profile.label(),
                round_one_tool.function_name()
            ),
            Some(vec![function_tool_call(
                round_one_tool.function_name(),
                &args,
            )]),
        );
    }

    if !round_two_tool.observed_by(observed_tools) {
        let args = round_two_tool.args(&worker_tag, task_round, task_seed);
        return openai_chat_response(
            &format!(
                "Worker preset task {task_round}/{} round-2 ({}) call {}.",
                scenario.worker_task_rounds.max(1),
                profile.label(),
                round_two_tool.function_name()
            ),
            Some(vec![function_tool_call(
                round_two_tool.function_name(),
                &args,
            )]),
        );
    }

    openai_chat_response(
        &format!(
            "Worker preset final: task {task_round}/{} profile={} complete for {}.",
            scenario.worker_task_rounds.max(1),
            profile.label(),
            worker_tag
        ),
        None,
    )
}

fn choose_worker_profile(task_seed: u64) -> WorkerToolProfile {
    match (task_seed % 8) as usize {
        0 => WorkerToolProfile::InspectSearch,
        1 => WorkerToolProfile::InspectRead,
        2 => WorkerToolProfile::WriteReplace,
        3 => WorkerToolProfile::WriteEdit,
        4 => WorkerToolProfile::CommandRead,
        5 => WorkerToolProfile::SearchFinal,
        6 => WorkerToolProfile::PtcSearch,
        _ => WorkerToolProfile::CommandPtc,
    }
}

fn choose_search_query(task_seed: u64) -> &'static str {
    const QUERIES: [&str; 6] = [
        "swarm",
        "worker",
        "context",
        "tool",
        "orchestration",
        "metrics",
    ];
    QUERIES[(task_seed as usize) % QUERIES.len()]
}

fn extract_task_round(user_message: &str) -> usize {
    let marker = "task_round=";
    let Some(marker_pos) = user_message.find(marker) else {
        return 1;
    };

    let value = &user_message[marker_pos + marker.len()..];
    let raw = value
        .split(';')
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim();
    if raw.is_empty() {
        return 1;
    }

    let raw = raw.split('/').next().unwrap_or(raw).trim();
    raw.parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(1)
        .clamp(1, MAX_WORKER_TASK_ROUNDS)
}

fn stable_string_hash64(text: &str) -> u64 {
    text.bytes().fold(0u64, |state, byte| {
        state.wrapping_mul(131).wrapping_add(byte as u64)
    })
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = value;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn derive_worker_task_seed(run_seed: u64, worker_tag: &str, task_round: usize) -> u64 {
    let hash = stable_string_hash64(worker_tag);
    splitmix64(run_seed ^ hash ^ (task_round as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93))
}

fn extract_worker_agent_hint(first_user: &str) -> Option<String> {
    let marker = "worker=";
    let marker_pos = first_user.find(marker)?;
    let value = &first_user[marker_pos + marker.len()..];
    let raw = value
        .split(';')
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

fn worker_tag(first_user: &str) -> String {
    let raw = extract_worker_agent_hint(first_user).unwrap_or_else(|| "worker".to_string());
    let mut cleaned = raw
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(18)
        .collect::<String>();
    if cleaned.is_empty() {
        cleaned = "worker".to_string();
    }
    cleaned
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

fn last_user_message(messages: &[Value]) -> String {
    last_user_message_index(messages)
        .and_then(|index| messages.get(index))
        .map(|message| flatten_content_text(message.get("content")))
        .unwrap_or_default()
}

fn last_user_message_index(messages: &[Value]) -> Option<usize> {
    for (index, message) in messages.iter().enumerate().rev() {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if role != "user" {
            continue;
        }
        let content = flatten_content_text(message.get("content"));
        if content.trim().is_empty() {
            continue;
        }
        if content.trim_start().starts_with(OBSERVATION_PREFIX) {
            continue;
        }
        return Some(index);
    }
    None
}

fn collect_current_observation_payloads(messages: &[Value]) -> Vec<Value> {
    let Some(last_user_index) = last_user_message_index(messages) else {
        return Vec::new();
    };
    if last_user_index + 1 >= messages.len() {
        return Vec::new();
    }
    collect_observation_payloads(&messages[last_user_index + 1..])
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

fn choose_worker_mission(run_seed: u64, agent_id: &str, task_round: usize) -> &'static str {
    const MISSIONS: [&str; 8] = [
        "inspect_workspace",
        "trace_swarm_context",
        "validate_tool_chain",
        "mutate_worker_file",
        "sample_latency_path",
        "stress_search_branch",
        "exercise_ptc_branch",
        "verify_final_response",
    ];
    let seed = derive_worker_task_seed(run_seed, agent_id, task_round);
    MISSIONS[(seed as usize) % MISSIONS.len()]
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

fn is_read_file_tool(name: &str) -> bool {
    matches!(
        name.trim(),
        "read_file" | "\u{8bfb}\u{53d6}\u{6587}\u{4ef6}"
    )
}

fn is_write_file_tool(name: &str) -> bool {
    matches!(
        name.trim(),
        "write_file" | "\u{5199}\u{5165}\u{6587}\u{4ef6}"
    )
}

fn is_replace_text_tool(name: &str) -> bool {
    matches!(
        name.trim(),
        "replace_text" | "\u{66ff}\u{6362}\u{6587}\u{672c}"
    )
}

fn is_edit_file_tool(name: &str) -> bool {
    matches!(
        name.trim(),
        "edit_file" | "\u{7f16}\u{8f91}\u{6587}\u{4ef6}"
    )
}

fn is_execute_command_tool(name: &str) -> bool {
    matches!(
        name.trim(),
        "execute_command" | "\u{6267}\u{884c}\u{547d}\u{4ee4}"
    )
}

fn is_ptc_tool(name: &str) -> bool {
    matches!(name.trim(), "programmatic_tool_call" | "ptc")
}

fn is_final_response_tool(name: &str) -> bool {
    matches!(
        name.trim(),
        "final_response" | "\u{6700}\u{7ec8}\u{56de}\u{590d}"
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

fn extract_swarm_wait_summaries(observation_payloads: &[Value]) -> Vec<SwarmWaitSummary> {
    let mut summaries = Vec::new();
    for payload in observation_payloads {
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
        summaries.push(SwarmWaitSummary {
            total,
            success_total,
            failed_total,
            all_finished,
            elapsed_s,
        });
    }
    summaries
}

fn seed_swarm_workspace(state: &AppState, user_id: &str) -> Result<()> {
    let shared_context = [
        "swarm simulation context",
        "- deterministic worker loops",
        "- validate tool orchestration",
        "- collect concurrency metrics",
    ]
    .join("\n");
    state
        .workspace
        .write_file(user_id, "sim_lab/shared/context.txt", &shared_context, true)?;
    Ok(())
}

fn seed_swarm_agents(
    state: &AppState,
    user_id: &str,
    workers: usize,
) -> Result<(String, Vec<String>)> {
    state.user_store.ensure_default_hive(user_id)?;
    let now = now_ts();

    let mother_agent_id = SIM_MOTHER_AGENT_ID.to_string();
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
        sandbox_container_id: random_sandbox_container_id(workers),
        created_at: now,
        updated_at: now,
    };
    state.user_store.upsert_user_agent(&mother)?;

    let worker_tools = seeded_worker_tools();
    let mut worker_ids = Vec::with_capacity(workers);
    for index in 0..workers {
        let agent_id = format!("wunder_sim_worker_{:03}", index + 1);
        let worker = UserAgentRecord {
            agent_id: agent_id.clone(),
            user_id: user_id.to_string(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            name: format!("SwarmWorkerSim{}", index + 1),
            description: "Deterministic worker agent for tool-loop simulation".to_string(),
            system_prompt:
                "Execute assigned task in two tool loops, then await the next task round."
                    .to_string(),
            tool_names: worker_tools.clone(),
            access_level: "A".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: random_sandbox_container_id(index),
            created_at: now,
            updated_at: now,
        };
        state.user_store.upsert_user_agent(&worker)?;
        worker_ids.push(agent_id);
    }

    Ok((mother_agent_id, worker_ids))
}

fn seeded_worker_tools() -> Vec<String> {
    vec![
        "list_files".to_string(),
        "search_content".to_string(),
        "read_file".to_string(),
        "write_file".to_string(),
        "replace_text".to_string(),
        "edit_file".to_string(),
        "execute_command".to_string(),
        "programmatic_tool_call".to_string(),
        "final_response".to_string(),
    ]
}

fn random_sandbox_container_id(seed: usize) -> i32 {
    let span = (MAX_SANDBOX_CONTAINER_ID - MIN_SANDBOX_CONTAINER_ID + 1) as u128;
    let mixed = Uuid::new_v4().as_u128() ^ ((seed as u128 + 1) * 0x9E37_79B9_7F4A_7C15_u128);
    MIN_SANDBOX_CONTAINER_ID + (mixed % span) as i32
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
    state: &AppState,
    run_control: &SimRunControl,
    user_id: &str,
    max_wait_s: u64,
    poll_ms: u64,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(max_wait_s);
    let mut cancelled = false;
    loop {
        if run_control.cancel_requested.load(Ordering::Relaxed) {
            cancelled = true;
            apply_cancel_signal(state, run_control);
        }

        let active_locks = state.user_store.list_session_locks_by_user(user_id)?.len();
        let active_sessions = state
            .monitor
            .list_sessions(true)
            .into_iter()
            .filter(|session| {
                session
                    .get("user_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .map(|value| value == user_id)
                    .unwrap_or(false)
            })
            .count();

        if active_locks == 0 && active_sessions == 0 {
            if cancelled {
                return Err(cancelled_error());
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "timed out waiting active runs to settle, active_locks={active_locks}, active_sessions={active_sessions}"
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
    artifact_root: &str,
    db_path: &str,
    workspace_root: &str,
    llm_base_url: &str,
    state: &AppState,
    user_id: &str,
    mother_session_id: &str,
    worker_agent_ids: &[String],
    mother_result: &crate::schemas::WunderResponse,
    mock_state: &MockLlmState,
) -> Result<FlowReport> {
    let run_rows = load_session_runs(state, user_id)?;
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
    let worker_session_set = worker_session_ids.iter().cloned().collect::<HashSet<_>>();
    let peak_concurrency_workers = compute_peak_concurrency(&run_rows, Some(&worker_session_set));
    let peak_concurrency_all = compute_peak_concurrency(&run_rows, None);
    let worker_status_map = latest_status_by_session(&run_rows);
    let tool_calls_by_session = load_tool_call_counts_by_session(state, &worker_session_ids);
    let tool_calls_by_name = load_tool_call_counts_by_name(state, user_id)?;

    let mut worker_items = Vec::new();
    let mut all_worker_success = true;
    let mut each_worker_two_tools = true;
    let mut each_worker_expected_tools = true;
    let expected_tool_calls_per_worker = args.worker_task_rounds.saturating_mul(2);
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
        if tool_calls != expected_tool_calls_per_worker {
            each_worker_expected_tools = false;
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

    let active_left = run_rows.iter().any(|row| {
        matches!(
            row.status.as_str(),
            "queued" | "running" | "waiting" | "cancelling"
        )
    });

    Ok(FlowReport {
        started_at,
        finished_at: Utc::now(),
        wall_time_s,
        config: FlowConfigSnapshot {
            workers: args.workers,
            max_wait_s: args.max_wait_s,
            mother_wait_s: args.mother_wait_s,
            poll_ms: args.poll_ms,
            worker_task_rounds: args.worker_task_rounds,
            worker_tool_loops: 2,
        },
        artifacts: FlowArtifacts {
            root: artifact_root.to_string(),
            db_path: db_path.to_string(),
            workspace_root: workspace_root.to_string(),
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
            peak_concurrency: peak_concurrency_workers,
            peak_concurrency_workers,
            peak_concurrency_all,
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
            each_worker_has_expected_tool_calls: each_worker_expected_tools,
            no_active_runs_left: !active_left,
        },
    })
}

fn load_session_runs(state: &AppState, user_id: &str) -> Result<Vec<SessionRunRow>> {
    let (sessions, _) = state
        .user_store
        .list_chat_sessions(user_id, None, None, 0, 4096)?;
    let mut output = Vec::with_capacity(sessions.len());

    for session in sessions {
        let record = state.monitor.get_record(&session.session_id);
        let status = record
            .as_ref()
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| "missing".to_string());
        let started_time = record
            .as_ref()
            .and_then(|value| value.get("start_time"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let updated_time = record
            .as_ref()
            .and_then(|value| value.get("updated_time"))
            .and_then(Value::as_f64)
            .unwrap_or(started_time);
        let finished_time = record
            .as_ref()
            .and_then(|value| value.get("ended_time"))
            .and_then(Value::as_f64)
            .unwrap_or(updated_time.max(started_time));

        output.push(SessionRunRow {
            session_id: session.session_id,
            status: normalize_status(status),
            queued_time: started_time,
            started_time,
            finished_time,
        });
    }

    output.sort_by(|left, right| left.queued_time.total_cmp(&right.queued_time));
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
    state: &AppState,
    session_ids: &[String],
) -> HashMap<String, usize> {
    let mut output = HashMap::new();
    for session_id in session_ids {
        let count = state
            .monitor
            .get_record(session_id)
            .as_ref()
            .map(count_tool_result_events)
            .unwrap_or(0);
        output.insert(session_id.clone(), count);
    }
    output
}

fn load_tool_call_counts_by_name(
    state: &AppState,
    user_id: &str,
) -> Result<BTreeMap<String, usize>> {
    let (sessions, _) = state
        .user_store
        .list_chat_sessions(user_id, None, None, 0, 4096)?;
    let mut output = BTreeMap::new();

    for session in sessions {
        let Some(record) = state.monitor.get_record(&session.session_id) else {
            continue;
        };
        for event in monitor_events(&record) {
            if event.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let Some(tool_name) = tool_name_from_event(event) else {
                continue;
            };
            *output.entry(tool_name).or_insert(0) += 1;
        }
    }

    Ok(output)
}

fn monitor_events(record: &Value) -> &[Value] {
    record
        .get("events")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn count_tool_result_events(record: &Value) -> usize {
    monitor_events(record)
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("tool_result"))
        .count()
}

fn tool_name_from_event(event: &Value) -> Option<String> {
    event
        .get("data")
        .and_then(|payload| payload.get("tool"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
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

fn compute_peak_concurrency(
    rows: &[SessionRunRow],
    include_sessions: Option<&HashSet<String>>,
) -> usize {
    let mut events = Vec::<(f64, i32)>::new();
    for row in rows {
        if include_sessions
            .as_ref()
            .is_some_and(|session_ids| !session_ids.contains(&row.session_id))
        {
            continue;
        }
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
    match status.trim().to_ascii_lowercase().as_str() {
        "finished" | "success" => "success".to_string(),
        "error" | "failed" | "cancelled" => "failed".to_string(),
        "running" | "queued" => "running".to_string(),
        "waiting" => "waiting".to_string(),
        "cancelling" => "cancelling".to_string(),
        other => other.to_string(),
    }
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}
