use crate::core::schemas::WunderRequest;
use crate::services::swarm::beeroom::{
    claim_mother_agent, get_mother_agent_id, resolve_or_create_agent_main_session,
    set_mother_agent, snapshot_team_run,
};
use crate::state::AppState;
use crate::storage::{normalize_hive_id, UserAgentRecord};
use crate::tools::resolve_tool_name;
use crate::user_store::build_default_agent_record_from_storage;
use anyhow::{anyhow, Result};
use axum::{extract::State, routing::post, Json, Router};
use chrono::Utc;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration, Instant};
use uuid::Uuid;

const DEMO_MODEL_NAME: &str = "__beeroom_demo_mock__";
const MOTHER_MARKER: &str = "BEEROOM_DEMO_MOTHER_START";
const WORKER_MARKER: &str = "BEEROOM_DEMO_WORKER_TASK";
const OBS_PREFIX: &str = "tool_response:";

const STATUS_STARTING: &str = "starting";
const STATUS_RUNNING: &str = "running";
const STATUS_CANCELLING: &str = "cancelling";
const STATUS_COMPLETED: &str = "completed";
const STATUS_FAILED: &str = "failed";
const STATUS_CANCELLED: &str = "cancelled";
const DEMO_STATUS_EVENT: &str = "beeroom_demo_status";
const DEMO_TERMINAL_WAIT_TIMEOUT_S: u64 = 90;
const DEMO_TERMINAL_WAIT_POLL_MS: u64 = 250;
const DEMO_TEAM_RUN_LOOKUP_TIMEOUT_S: u64 = 8;
const DEMO_TEAM_RUN_LOOKUP_POLL_MS: u64 = 150;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct StartBeeroomDemoRequest {
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default, alias = "workerCount", alias = "worker_count")]
    pub worker_count: Option<i64>,
    #[serde(default, alias = "workerCountMode", alias = "worker_count_mode")]
    pub worker_count_mode: Option<String>,
    #[serde(default)]
    pub speed: Option<String>,
    #[serde(default)]
    pub scenario: Option<String>,
    #[serde(default, alias = "toolProfile", alias = "tool_profile")]
    pub tool_profile: Option<String>,
    #[serde(default, alias = "motherAgentId", alias = "mother_agent_id")]
    pub mother_agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BeeroomDemoRunSnapshot {
    pub run_id: String,
    pub group_id: String,
    pub status: String,
    pub team_run_id: Option<String>,
    pub mother_session_id: Option<String>,
    pub selected_worker_ids: Vec<String>,
    pub seed: u64,
    pub started_at: f64,
    pub updated_at: f64,
    pub finished_at: Option<f64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
struct DemoWorkerPlan {
    agent_id: String,
    tool_step_one: Option<String>,
    tool_step_two: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum DemoSpeed {
    Fast,
    Normal,
    Slow,
}

impl DemoSpeed {
    fn from_raw(raw: Option<&str>) -> Self {
        match raw
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .unwrap_or("normal")
            .to_ascii_lowercase()
            .as_str()
        {
            "fast" => Self::Fast,
            "slow" => Self::Slow,
            _ => Self::Normal,
        }
    }

    fn wait_seconds(self) -> f64 {
        match self {
            Self::Fast => 8.0,
            Self::Normal => 14.0,
            Self::Slow => 22.0,
        }
    }

    fn worker_sleep_seconds(self) -> f64 {
        match self {
            Self::Fast => 0.35,
            Self::Normal => 0.65,
            Self::Slow => 1.0,
        }
    }

    fn as_text(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Normal => "normal",
            Self::Slow => "slow",
        }
    }
}

#[derive(Debug, Clone)]
struct DemoPlan {
    user_id: String,
    mother: UserAgentRecord,
    workers: Vec<DemoWorkerPlan>,
    team_run_id: String,
    seed: u64,
    speed: DemoSpeed,
    scenario: String,
}

#[derive(Debug, Clone)]
struct DemoRunState {
    status: String,
    team_run_id: Option<String>,
    mother_session_id: Option<String>,
    selected_worker_ids: Vec<String>,
    updated_at: f64,
    finished_at: Option<f64>,
    error: Option<String>,
}

struct DemoTerminalOutcome {
    status: String,
    error: Option<String>,
}

struct DemoRunControl {
    run_id: String,
    user_id: String,
    group_id: String,
    seed: u64,
    started_at: f64,
    cancel_requested: AtomicBool,
    state: Mutex<DemoRunState>,
}

impl DemoRunControl {
    fn new(run_id: String, user_id: String, group_id: String, seed: u64) -> Self {
        let now = now_ts();
        Self {
            run_id,
            user_id,
            group_id,
            seed,
            started_at: now,
            cancel_requested: AtomicBool::new(false),
            state: Mutex::new(DemoRunState {
                status: STATUS_STARTING.to_string(),
                team_run_id: None,
                mother_session_id: None,
                selected_worker_ids: Vec::new(),
                updated_at: now,
                finished_at: None,
                error: None,
            }),
        }
    }

    fn snapshot(&self) -> BeeroomDemoRunSnapshot {
        let state = self.state.lock();
        BeeroomDemoRunSnapshot {
            run_id: self.run_id.clone(),
            group_id: self.group_id.clone(),
            status: state.status.clone(),
            team_run_id: state.team_run_id.clone(),
            mother_session_id: state.mother_session_id.clone(),
            selected_worker_ids: state.selected_worker_ids.clone(),
            seed: self.seed,
            started_at: self.started_at,
            updated_at: state.updated_at,
            finished_at: state.finished_at,
            error: state.error.clone(),
        }
    }

    fn mark_running(&self, mother_session_id: &str, workers: &[DemoWorkerPlan]) {
        let mut state = self.state.lock();
        state.status = STATUS_RUNNING.to_string();
        state.mother_session_id = Some(mother_session_id.to_string());
        state.selected_worker_ids = workers
            .iter()
            .map(|worker| worker.agent_id.clone())
            .collect();
        state.updated_at = now_ts();
        state.error = None;
    }

    fn mark_cancelling(&self) {
        let mut state = self.state.lock();
        state.status = STATUS_CANCELLING.to_string();
        state.updated_at = now_ts();
    }

    fn update_team_run_id(&self, team_run_id: Option<String>) {
        if team_run_id.is_none() {
            return;
        }
        let mut state = self.state.lock();
        state.team_run_id = team_run_id;
        state.updated_at = now_ts();
    }

    fn mark_terminal(&self, status: &str, error: Option<String>) {
        let now = now_ts();
        let mut state = self.state.lock();
        state.status = status.to_string();
        state.error = error;
        state.updated_at = now;
        state.finished_at = Some(now);
    }
}

#[derive(Debug, Clone, Default)]
struct MockScenario {
    run_id: String,
    team_run_id: String,
    workers: Vec<DemoWorkerPlan>,
    seed: u64,
    speed: String,
    wait_seconds: f64,
    worker_sleep_seconds: f64,
}

#[derive(Default)]
struct MockLlmState {
    scenario: RwLock<MockScenario>,
}

static DEMO_RUN_REGISTRY: OnceLock<Mutex<HashMap<String, Arc<DemoRunControl>>>> = OnceLock::new();

fn run_registry() -> &'static Mutex<HashMap<String, Arc<DemoRunControl>>> {
    DEMO_RUN_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn start_demo_run(
    state: Arc<AppState>,
    user_id: &str,
    group_id: &str,
    payload: StartBeeroomDemoRequest,
) -> Result<BeeroomDemoRunSnapshot> {
    let cleaned_user = user_id.trim();
    let cleaned_group = normalize_hive_id(group_id);
    if cleaned_user.is_empty() || cleaned_group.is_empty() {
        return Err(anyhow!("user_id/group_id is required"));
    }
    if let Some(active) = find_active_run(cleaned_user, &cleaned_group) {
        return Err(anyhow!("demo run already active: {}", active.run_id));
    }

    let plan = build_demo_plan(state.as_ref(), cleaned_user, &cleaned_group, &payload)?;
    let run_id = format!("br_demo_{}", Uuid::new_v4().simple());
    let control = Arc::new(DemoRunControl::new(
        run_id.clone(),
        cleaned_user.to_string(),
        cleaned_group.clone(),
        plan.seed,
    ));
    {
        let mut registry = run_registry().lock();
        registry.insert(run_id, control.clone());
    }
    publish_demo_status(state.as_ref(), control.as_ref()).await;

    let run_state = state.clone();
    let run_control = control.clone();
    tokio::spawn(async move {
        execute_demo_run(run_state, run_control, plan).await;
    });
    Ok(control.snapshot())
}

pub fn get_demo_run_snapshot(
    user_id: &str,
    group_id: &str,
    run_id: &str,
) -> Result<Option<BeeroomDemoRunSnapshot>> {
    let cleaned_user = user_id.trim();
    let cleaned_group = normalize_hive_id(group_id);
    let cleaned_run = run_id.trim();
    if cleaned_user.is_empty() || cleaned_group.is_empty() || cleaned_run.is_empty() {
        return Ok(None);
    }
    let control = {
        let registry = run_registry().lock();
        registry.get(cleaned_run).cloned()
    };
    let Some(control) = control else {
        return Ok(None);
    };
    if control.user_id != cleaned_user || control.group_id != cleaned_group {
        return Ok(None);
    }
    Ok(Some(control.snapshot()))
}

pub async fn cancel_demo_run(
    state: Arc<AppState>,
    user_id: &str,
    group_id: &str,
    run_id: &str,
) -> Result<Option<BeeroomDemoRunSnapshot>> {
    let cleaned_user = user_id.trim();
    let cleaned_group = normalize_hive_id(group_id);
    let cleaned_run = run_id.trim();
    if cleaned_user.is_empty() || cleaned_group.is_empty() || cleaned_run.is_empty() {
        return Ok(None);
    }
    let control = {
        let registry = run_registry().lock();
        registry.get(cleaned_run).cloned()
    };
    let Some(control) = control else {
        return Ok(None);
    };
    if control.user_id != cleaned_user || control.group_id != cleaned_group {
        return Ok(None);
    }

    control.cancel_requested.store(true, Ordering::Relaxed);
    control.mark_cancelling();
    let snapshot = control.snapshot();
    publish_demo_status(state.as_ref(), control.as_ref()).await;
    if let Some(session_id) = snapshot.mother_session_id.as_deref() {
        let _ = state.monitor.cancel(session_id);
    }
    if let Some(team_run_id) = snapshot.team_run_id.as_deref() {
        state.team_run_runner.cancel(team_run_id).await;
    }
    Ok(Some(snapshot))
}

fn find_active_run(user_id: &str, group_id: &str) -> Option<BeeroomDemoRunSnapshot> {
    let registry = run_registry().lock();
    registry
        .values()
        .filter(|control| control.user_id == user_id && control.group_id == group_id)
        .map(|control| control.snapshot())
        .find(|snapshot| {
            matches!(
                snapshot.status.as_str(),
                STATUS_STARTING | STATUS_RUNNING | STATUS_CANCELLING
            )
        })
}

fn build_demo_plan(
    state: &AppState,
    user_id: &str,
    group_id: &str,
    payload: &StartBeeroomDemoRequest,
) -> Result<DemoPlan> {
    let agents = state
        .user_store
        .list_user_agents_by_hive_with_default(user_id, group_id)?;
    if agents.is_empty() {
        return Err(anyhow!("no agents found in beeroom group"));
    }
    // Demo runs must use agents resolvable by swarm runtime; synthetic snapshots
    // (e.g. default alias injected from meta only) will fail in `agent_swarm`.
    let mut resolvable_agents = Vec::with_capacity(agents.len());
    for agent in agents {
        if is_swarm_resolvable_agent(state.storage.as_ref(), user_id, &agent.agent_id)? {
            resolvable_agents.push(agent);
        }
    }
    let agents = resolvable_agents;
    if agents.is_empty() {
        return Err(anyhow!(
            "no swarm-resolvable agents found in beeroom group; please add persisted agents"
        ));
    }

    let mother = resolve_mother_agent(
        state,
        user_id,
        group_id,
        payload.mother_agent_id.as_deref(),
        &agents,
    )?;
    let worker_candidates = agents
        .into_iter()
        .filter(|agent| agent.agent_id != mother.agent_id)
        .collect::<Vec<_>>();
    if worker_candidates.is_empty() {
        return Err(anyhow!("beeroom group has no worker candidates"));
    }

    let seed = payload.seed.unwrap_or_else(default_seed);
    let speed = DemoSpeed::from_raw(payload.speed.as_deref());
    let scenario = payload
        .scenario
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or("standard")
        .to_string();
    let mode = payload
        .worker_count_mode
        .as_deref()
        .map(str::trim)
        .unwrap_or("random")
        .to_ascii_lowercase();
    let profile = payload
        .tool_profile
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or("safe")
        .to_ascii_lowercase();

    let count = resolve_worker_count(payload.worker_count, &mode, worker_candidates.len(), seed);
    let workers = pick_workers(worker_candidates, count, seed)
        .into_iter()
        .map(|agent| {
            let (tool_step_one, tool_step_two) = choose_worker_tools(&agent.tool_names, &profile);
            DemoWorkerPlan {
                agent_id: agent.agent_id,
                tool_step_one,
                tool_step_two,
            }
        })
        .collect::<Vec<_>>();

    Ok(DemoPlan {
        user_id: user_id.to_string(),
        mother,
        workers,
        team_run_id: format!("team_{}", Uuid::new_v4().simple()),
        seed,
        speed,
        scenario,
    })
}

fn resolve_mother_agent(
    state: &AppState,
    user_id: &str,
    group_id: &str,
    requested_mother_agent_id: Option<&str>,
    agents: &[UserAgentRecord],
) -> Result<UserAgentRecord> {
    let requested = requested_mother_agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(requested) = requested {
        let agent = agents
            .iter()
            .find(|item| item.agent_id == requested)
            .cloned()
            .ok_or_else(|| {
                anyhow!("requested demo mother agent is not available in current beeroom")
            })?;
        set_mother_agent(state.storage.as_ref(), user_id, group_id, &agent.agent_id)?;
        return Ok(agent);
    }

    let mother_id = get_mother_agent_id(state.storage.as_ref(), user_id, group_id)?;
    if let Some(mother_id) = mother_id {
        if let Some(agent) = agents.iter().find(|item| item.agent_id == mother_id) {
            return Ok(agent.clone());
        }
    }
    let fallback = agents
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("mother agent not found"))?;
    let _ = claim_mother_agent(
        state.storage.as_ref(),
        user_id,
        group_id,
        &fallback.agent_id,
    )?;
    Ok(fallback)
}

fn is_swarm_resolvable_agent(
    storage: &dyn crate::storage::StorageBackend,
    user_id: &str,
    agent_id: &str,
) -> Result<bool> {
    let cleaned_user = user_id.trim();
    let cleaned_agent = agent_id.trim();
    if cleaned_user.is_empty() || cleaned_agent.is_empty() {
        return Ok(false);
    }
    if is_default_agent_alias(cleaned_agent) {
        return build_default_agent_record_from_storage(storage, cleaned_user).map(|_| true);
    }
    if storage
        .get_user_agent(cleaned_user, cleaned_agent)?
        .is_some()
    {
        return Ok(true);
    }
    Ok(storage.get_user_agent_by_id(cleaned_agent)?.is_some())
}

fn resolve_worker_count(requested: Option<i64>, mode: &str, total: usize, seed: u64) -> usize {
    let capped_total = total.clamp(1, 6);
    if let Some(requested) = requested {
        return (requested.max(1) as usize).min(capped_total);
    }
    if mode == "all" {
        return capped_total;
    }
    if capped_total <= 2 {
        return capped_total;
    }
    let range = capped_total - 1;
    let offset = (splitmix64(seed ^ 0xDDBB_AA55) % range as u64) as usize;
    (offset + 1).max(2).min(capped_total)
}

fn pick_workers(candidates: Vec<UserAgentRecord>, count: usize, seed: u64) -> Vec<UserAgentRecord> {
    let mut scored = candidates
        .into_iter()
        .map(|agent| (splitmix64(seed ^ stable_hash64(&agent.agent_id)), agent))
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| left.0.cmp(&right.0));
    scored
        .into_iter()
        .take(count)
        .map(|(_, agent)| agent)
        .collect()
}

fn choose_worker_tools(tool_names: &[String], profile: &str) -> (Option<String>, Option<String>) {
    let normalized = tool_names
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(|name| (name.to_string(), resolve_tool_name(name)))
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        return (None, None);
    }
    if profile != "safe" {
        let first = normalized.first().map(|(raw, _)| raw.clone());
        let second = normalized
            .get(1)
            .map(|(raw, _)| raw.clone())
            .filter(|tool| Some(tool) != first.as_ref());
        return (first, second);
    }

    let first = normalized
        .iter()
        .find(|(_, canonical)| is_sleep_tool(canonical) || is_list_tool(canonical))
        .map(|(raw, _)| raw.clone())
        .or_else(|| {
            normalized
                .iter()
                .find(|(_, canonical)| is_search_tool(canonical))
                .map(|(raw, _)| raw.clone())
        })
        .or_else(|| {
            normalized
                .iter()
                .find(|(_, canonical)| is_read_tool(canonical))
                .map(|(raw, _)| raw.clone())
        })
        .or_else(|| normalized.first().map(|(raw, _)| raw.clone()));
    let second = normalized
        .iter()
        .find(|(raw, canonical)| Some(raw) != first.as_ref() && is_search_tool(canonical))
        .map(|(raw, _)| raw.clone())
        .or_else(|| {
            normalized
                .iter()
                .find(|(raw, canonical)| Some(raw) != first.as_ref() && is_read_tool(canonical))
                .map(|(raw, _)| raw.clone())
        })
        .or_else(|| {
            normalized
                .iter()
                .find(|(raw, canonical)| Some(raw) != first.as_ref() && is_list_tool(canonical))
                .map(|(raw, _)| raw.clone())
        });
    let second = second.or_else(|| {
        normalized
            .iter()
            .find(|(raw, _)| Some(raw) != first.as_ref())
            .map(|(raw, _)| raw.clone())
    });
    (first, second)
}

async fn execute_demo_run(state: Arc<AppState>, control: Arc<DemoRunControl>, plan: DemoPlan) {
    let run_result = async {
        let (mother_session, _created) = resolve_or_create_agent_main_session(
            state.storage.as_ref(),
            &plan.user_id,
            &plan.mother,
        )?;
        control.mark_running(&mother_session.session_id, &plan.workers);
        publish_demo_status(state.as_ref(), control.as_ref()).await;

        if control.cancel_requested.load(Ordering::Relaxed) {
            return Err(anyhow!("demo cancelled"));
        }

        let mock_state = Arc::new(MockLlmState::default());
        {
            let mut scenario = mock_state.scenario.write();
            *scenario = MockScenario {
                run_id: control.run_id.clone(),
                team_run_id: plan.team_run_id.clone(),
                workers: plan.workers.clone(),
                seed: plan.seed,
                speed: plan.speed.as_text().to_string(),
                wait_seconds: plan.speed.wait_seconds(),
                worker_sleep_seconds: plan.speed.worker_sleep_seconds(),
            };
        }

        let (base_url, _addr, server_task) = start_mock_llm_server(mock_state).await?;
        control.update_team_run_id(Some(plan.team_run_id.clone()));
        publish_demo_status(state.as_ref(), control.as_ref()).await;
        let request = build_mother_request(
            &plan,
            &mother_session.session_id,
            &base_url,
            &control.run_id,
        );
        let response = state.orchestrator.run(request).await;
        server_task.abort();
        response?;
        wait_for_demo_team_run_created(state.as_ref(), &plan.team_run_id).await?;
        if control.cancel_requested.load(Ordering::Relaxed) {
            state.team_run_runner.cancel(&plan.team_run_id).await;
            return Err(anyhow!("demo cancelled"));
        }
        wait_for_demo_terminal(state.as_ref(), control.as_ref(), &plan.team_run_id).await
    }
    .await;

    match run_result {
        Ok(outcome) => match outcome.status.as_str() {
            STATUS_COMPLETED => control.mark_terminal(STATUS_COMPLETED, None),
            STATUS_CANCELLED => control.mark_terminal(STATUS_CANCELLED, None),
            _ => control.mark_terminal(
                STATUS_FAILED,
                outcome
                    .error
                    .or_else(|| Some("demo team run finished abnormally".to_string())),
            ),
        },
        Err(err) => {
            if control.cancel_requested.load(Ordering::Relaxed) || is_cancel_like(&err) {
                control.mark_terminal(STATUS_CANCELLED, None);
            } else {
                control.mark_terminal(STATUS_FAILED, Some(err.to_string()));
            }
        }
    }
    publish_demo_status(state.as_ref(), control.as_ref()).await;
}

fn build_mother_request(
    plan: &DemoPlan,
    mother_session_id: &str,
    base_url: &str,
    run_id: &str,
) -> WunderRequest {
    let config_overrides = json!({
        "server": {
            "max_active_sessions": (plan.workers.len().saturating_mul(4)).max(12)
        },
        "tools": {
            "swarm": {
                "max_parallel_tasks_per_team": plan.workers.len().max(1),
                "max_active_team_runs": plan.workers.len().max(1),
                "max_retry": 0
            }
        },
        "llm": {
            "default": DEMO_MODEL_NAME,
            "models": {
                DEMO_MODEL_NAME: {
                    "provider": "openai",
                    "base_url": base_url,
                    "api_key": "beeroom-demo-key",
                    "model": "beeroom-demo-model",
                    "stream": false,
                    "retry": 0,
                    "max_rounds": 36,
                    "tool_call_mode": "tool_call",
                    "temperature": 0.0
                }
            }
        }
    });
    let question = format!(
        "{MOTHER_MARKER}: run_id={run_id}; scenario={}; speed={}; seed={}; dispatch worker swarm, wait and merge.",
        plan.scenario,
        plan.speed.as_text(),
        plan.seed
    );
    WunderRequest {
        user_id: plan.user_id.clone(),
        question,
        tool_names: vec!["agent_swarm".to_string()],
        skip_tool_calls: false,
        stream: false,
        debug_payload: false,
        session_id: Some(mother_session_id.to_string()),
        agent_id: Some(plan.mother.agent_id.clone()),
        model_name: Some(DEMO_MODEL_NAME.to_string()),
        language: Some("zh-CN".to_string()),
        config_overrides: Some(config_overrides),
        agent_prompt: None,
        attachments: None,
        allow_queue: true,
        is_admin: false,
        approval_tx: None,
    }
}

async fn wait_for_demo_team_run_created(state: &AppState, team_run_id: &str) -> Result<()> {
    let lookup_started = Instant::now();
    loop {
        if state.user_store.get_team_run(team_run_id)?.is_some() {
            return Ok(());
        }
        if lookup_started.elapsed() >= Duration::from_secs(DEMO_TEAM_RUN_LOOKUP_TIMEOUT_S) {
            return Err(anyhow!("demo team run was not created"));
        }
        sleep(Duration::from_millis(DEMO_TEAM_RUN_LOOKUP_POLL_MS)).await;
    }
}

async fn wait_for_demo_terminal(
    state: &AppState,
    control: &DemoRunControl,
    team_run_id: &str,
) -> Result<DemoTerminalOutcome> {
    let timeout = Duration::from_secs(DEMO_TERMINAL_WAIT_TIMEOUT_S);
    let started = Instant::now();
    loop {
        if control.cancel_requested.load(Ordering::Relaxed) {
            return Err(anyhow!("demo cancelled"));
        }
        let run = state
            .user_store
            .get_team_run(team_run_id)?
            .ok_or_else(|| anyhow!("demo team run not found: {team_run_id}"))?;
        let snapshot =
            snapshot_team_run(state.storage.as_ref(), Some(state.monitor.as_ref()), &run)?;
        let completion_status = snapshot.completion_status.trim().to_ascii_lowercase();
        let terminal = matches!(
            completion_status.as_str(),
            "completed" | "failed" | "cancelled"
        );
        if terminal && snapshot.all_tasks_terminal && snapshot.all_agents_idle {
            return Ok(DemoTerminalOutcome {
                status: completion_status,
                error: snapshot.run.error.clone(),
            });
        }
        if started.elapsed() >= timeout {
            return Err(anyhow!(
                "demo team run did not reach terminal state before timeout"
            ));
        }
        sleep(Duration::from_millis(DEMO_TERMINAL_WAIT_POLL_MS)).await;
    }
}

async fn publish_demo_status(state: &AppState, control: &DemoRunControl) {
    let snapshot = control.snapshot();
    let payload = json!({
        "run_id": snapshot.run_id,
        "group_id": snapshot.group_id,
        "status": snapshot.status,
        "team_run_id": snapshot.team_run_id,
        "mother_session_id": snapshot.mother_session_id,
        "selected_worker_ids": snapshot.selected_worker_ids,
        "seed": snapshot.seed,
        "started_at": snapshot.started_at,
        "updated_at": snapshot.updated_at,
        "finished_at": snapshot.finished_at,
        "error": snapshot.error,
    });
    state
        .beeroom_realtime
        .publish_group_event(
            &control.user_id,
            &control.group_id,
            DEMO_STATUS_EVENT,
            payload,
        )
        .await;
}

async fn start_mock_llm_server(
    state: Arc<MockLlmState>,
) -> Result<(String, std::net::SocketAddr, JoinHandle<()>)> {
    let app = Router::new()
        .route("/v1/chat/completions", post(mock_chat_completions))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            eprintln!("[beeroom_demo] mock llm server failed: {err}");
        }
    });
    Ok((format!("http://{addr}"), addr, handle))
}

async fn mock_chat_completions(
    State(state): State<Arc<MockLlmState>>,
    Json(payload): Json<Value>,
) -> Json<Value> {
    let messages = payload
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let user_message = last_user_message(&messages);
    let scenario = state.scenario.read().clone();
    let observations = collect_observation_payloads(&messages);
    let current_observations = collect_current_observation_payloads(&messages);
    let observed_tools = observed_tool_names(&current_observations);

    let response = if user_message.contains(MOTHER_MARKER) {
        if observations.is_empty() {
            mother_dispatch_response(&scenario)
        } else {
            mother_followup_response(&scenario, &current_observations)
        }
    } else if user_message.contains(WORKER_MARKER) {
        worker_response(&scenario, &user_message, &observed_tools)
    } else {
        openai_chat_response("Demo fallback response.", None)
    };
    Json(response)
}

fn mother_dispatch_response(scenario: &MockScenario) -> Value {
    let tasks = scenario
        .workers
        .iter()
        .enumerate()
        .map(|(index, worker)| {
            let tool1 = worker.tool_step_one.clone().unwrap_or_default();
            let tool2 = worker.tool_step_two.clone().unwrap_or_default();
            json!({
                "agentId": worker.agent_id,
                "message": format!(
                    "{WORKER_MARKER}: worker={}; run_id={}; speed={}; seed={}; tool1={tool1}; tool2={tool2}",
                    worker.agent_id,
                    scenario.run_id,
                    scenario.speed,
                    scenario.seed
                ),
                "label": format!("demo-worker-{}", index + 1),
                "createIfMissing": true
            })
        })
        .collect::<Vec<_>>();
    let args = json!({
        "action": "batch_send",
        "teamRunId": scenario.team_run_id,
        "tasks": tasks,
        "waitSeconds": scenario.wait_seconds,
        "pollIntervalSeconds": 0.25,
        "createIfMissing": true,
        "includeCurrent": false
    });
    openai_chat_response(
        &format!(
            "Demo mother dispatching {} workers.",
            scenario.workers.len()
        ),
        Some(vec![function_tool_call("agent_swarm", &args)]),
    )
}

fn mother_followup_response(scenario: &MockScenario, observations: &[Value]) -> Value {
    if let Some(wait_args) = build_mother_wait_args(scenario, observations) {
        return openai_chat_response(
            "Demo mother is still waiting for workers to finish.",
            Some(vec![function_tool_call("agent_swarm", &wait_args)]),
        );
    }
    openai_chat_response(
        "Demo mother merged worker outputs. Final report is ready.",
        None,
    )
}

fn build_mother_wait_args(scenario: &MockScenario, observations: &[Value]) -> Option<Value> {
    let latest = observations.last()?;
    let wait_payload = latest.get("wait").unwrap_or(latest);
    let all_finished = wait_payload
        .get("all_finished")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let run_ids = wait_payload
        .get("run_ids")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if all_finished || run_ids.is_empty() {
        return None;
    }
    Some(json!({
        "action": "wait",
        "runIds": run_ids,
        "waitSeconds": scenario.wait_seconds,
        "pollIntervalSeconds": 0.25
    }))
}

fn worker_response(
    scenario: &MockScenario,
    user_message: &str,
    observed_tools: &HashSet<String>,
) -> Value {
    let worker_id =
        extract_message_value(user_message, "worker").unwrap_or_else(|| "worker".to_string());
    let tool1 = extract_message_value(user_message, "tool1").filter(|tool| !tool.is_empty());
    let tool2 = extract_message_value(user_message, "tool2").filter(|tool| !tool.is_empty());

    if let Some(tool1) = tool1.as_ref() {
        if !observed_tools
            .iter()
            .any(|observed| tool_equal(observed, tool1))
        {
            let args = build_tool_args(tool1, &worker_id, scenario.worker_sleep_seconds);
            return openai_chat_response(
                &format!("Demo worker {worker_id} step-1 calling {tool1}."),
                Some(vec![function_tool_call(tool1, &args)]),
            );
        }
    }
    if let Some(tool2) = tool2.as_ref() {
        if !observed_tools
            .iter()
            .any(|observed| tool_equal(observed, tool2))
        {
            let args = build_tool_args(tool2, &worker_id, scenario.worker_sleep_seconds);
            return openai_chat_response(
                &format!("Demo worker {worker_id} step-2 calling {tool2}."),
                Some(vec![function_tool_call(tool2, &args)]),
            );
        }
    }

    openai_chat_response(
        &format!("Demo worker {worker_id} completed simulated workflow and returned result."),
        None,
    )
}

fn build_tool_args(tool_name: &str, worker_id: &str, sleep_seconds: f64) -> Value {
    if is_sleep_tool(tool_name) {
        return json!({
            "seconds": sleep_seconds.max(0.1),
            "reason": format!("beeroom demo worker={worker_id}")
        });
    }
    if is_list_tool(tool_name) {
        return json!({ "path": ".", "max_depth": 2 });
    }
    if is_search_tool(tool_name) {
        return json!({
            "query": "swarm",
            "path": ".",
            "max_depth": 2,
            "max_files": 30
        });
    }
    if is_read_tool(tool_name) {
        return json!({
            "files": [{
                "path": "README.md",
                "start_line": 1,
                "end_line": 60
            }]
        });
    }
    json!({})
}

fn openai_chat_response(content: &str, tool_calls: Option<Vec<Value>>) -> Value {
    let mut message = json!({ "role": "assistant", "content": content });
    if let Some(tool_calls) = tool_calls.filter(|items| !items.is_empty()) {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    json!({
        "id": format!("chatcmpl_{}", Uuid::new_v4().simple()),
        "object": "chat.completion",
        "created": Utc::now().timestamp(),
        "model": "beeroom-demo",
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 64,
            "completion_tokens": 24,
            "total_tokens": 88
        }
    })
}

fn function_tool_call(name: &str, args: &Value) -> Value {
    json!({
        "id": format!("call_{}", Uuid::new_v4().simple()),
        "type": "function",
        "function": {
            "name": name,
            "arguments": serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string())
        }
    })
}

fn last_user_message(messages: &[Value]) -> String {
    last_user_index(messages)
        .and_then(|index| messages.get(index))
        .map(|message| flatten_content(message.get("content")))
        .unwrap_or_default()
}

fn last_user_index(messages: &[Value]) -> Option<usize> {
    for (index, message) in messages.iter().enumerate().rev() {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if role != "user" {
            continue;
        }
        let content = flatten_content(message.get("content"));
        if content.trim().is_empty() || content.trim_start().starts_with(OBS_PREFIX) {
            continue;
        }
        return Some(index);
    }
    None
}

fn collect_current_observation_payloads(messages: &[Value]) -> Vec<Value> {
    let Some(index) = last_user_index(messages) else {
        return Vec::new();
    };
    if index + 1 >= messages.len() {
        return Vec::new();
    }
    collect_observation_payloads(&messages[index + 1..])
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
    let content = flatten_content(message.get("content"));
    if content.trim().is_empty() {
        return None;
    }
    if role == "tool" {
        return serde_json::from_str(&content).ok();
    }
    let trimmed = content.trim_start();
    if !trimmed.starts_with(OBS_PREFIX) {
        return None;
    }
    let payload = trimmed
        .trim_start_matches(OBS_PREFIX)
        .trim_start_matches(':')
        .trim();
    serde_json::from_str(payload).ok()
}

fn observed_tool_names(payloads: &[Value]) -> HashSet<String> {
    payloads
        .iter()
        .filter_map(|payload| payload.get("tool").and_then(Value::as_str))
        .map(str::trim)
        .filter(|tool| !tool.is_empty())
        .map(ToString::to_string)
        .collect::<HashSet<_>>()
}

fn flatten_content(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    match content {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    return Some(text.to_string());
                }
                item.as_str().map(ToString::to_string)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

fn extract_message_value(message: &str, key: &str) -> Option<String> {
    let marker = format!("{key}=");
    let pos = message.find(&marker)?;
    let raw = &message[pos + marker.len()..];
    let value = raw
        .split(';')
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn tool_equal(left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    left.eq_ignore_ascii_case(right) || resolve_tool_name(left) == resolve_tool_name(right)
}

fn is_sleep_tool(name: &str) -> bool {
    let cleaned = name.trim();
    matches!(
        cleaned.to_ascii_lowercase().as_str(),
        "sleep" | "sleep_wait" | "pause"
    ) || resolve_tool_name(cleaned) == resolve_tool_name("sleep")
}

fn is_list_tool(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    normalized == "list" || resolve_tool_name(name.trim()) == resolve_tool_name("list_files")
}

fn is_search_tool(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    normalized == "search" || resolve_tool_name(name.trim()) == resolve_tool_name("search_content")
}

fn is_read_tool(name: &str) -> bool {
    resolve_tool_name(name.trim()) == resolve_tool_name("read_file")
}

fn is_default_agent_alias(agent_id: &str) -> bool {
    let cleaned = agent_id.trim();
    cleaned.eq_ignore_ascii_case("__default__") || cleaned.eq_ignore_ascii_case("default")
}

fn default_seed() -> u64 {
    splitmix64(now_ts().to_bits() ^ stable_hash64(&Uuid::new_v4().to_string()))
}

fn stable_hash64(text: &str) -> u64 {
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

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

fn is_cancel_like(err: &anyhow::Error) -> bool {
    let message = err.to_string().to_ascii_lowercase();
    message.contains("cancel")
        || message.contains("abort")
        || message.contains("stop")
        || message.contains("interrupt")
}

#[cfg(test)]
mod tests {
    use super::{
        choose_worker_tools, extract_message_value, is_swarm_resolvable_agent,
        mother_dispatch_response, normalize_hive_id, resolve_worker_count, DemoWorkerPlan,
        MockScenario,
    };
    use crate::storage::{SqliteStorage, StorageBackend, UserAgentRecord, DEFAULT_HIVE_ID};
    use crate::tools::resolve_tool_name;
    use serde_json::Value;
    use tempfile::tempdir;

    #[test]
    fn resolve_worker_count_respects_explicit_request_and_cap() {
        let total = 9usize;
        assert_eq!(resolve_worker_count(Some(1), "random", total, 11), 1);
        assert_eq!(resolve_worker_count(Some(999), "random", total, 11), 6);
        assert_eq!(resolve_worker_count(Some(-3), "random", total, 11), 1);
    }

    #[test]
    fn resolve_worker_count_random_mode_keeps_reasonable_range() {
        let count = resolve_worker_count(None, "random", 5, 42);
        assert!((2..=5).contains(&count));
    }

    #[test]
    fn resolve_worker_count_all_mode_uses_all_candidates_with_cap() {
        assert_eq!(resolve_worker_count(None, "all", 0, 1), 1);
        assert_eq!(resolve_worker_count(None, "all", 3, 1), 3);
        assert_eq!(resolve_worker_count(None, "all", 12, 1), 6);
    }

    #[test]
    fn choose_worker_tools_safe_profile_prefers_low_risk_tools() {
        let tools = vec![
            "read_file".to_string(),
            "search_content".to_string(),
            "list_files".to_string(),
        ];
        let (first, second) = choose_worker_tools(&tools, "safe");
        assert_eq!(first.as_deref(), Some("list_files"));
        assert_eq!(second.as_deref(), Some("search_content"));
    }

    #[test]
    fn choose_worker_tools_safe_profile_supports_canonical_builtin_names() {
        let read_tool = resolve_tool_name("read_file");
        let search_tool = resolve_tool_name("search_content");
        let list_tool = resolve_tool_name("list_files");
        let tools = vec![read_tool.clone(), search_tool.clone(), list_tool.clone()];
        let (first, second) = choose_worker_tools(&tools, "safe");
        assert_eq!(first.as_deref(), Some(list_tool.as_str()));
        assert_eq!(second.as_deref(), Some(search_tool.as_str()));
    }

    #[test]
    fn choose_worker_tools_safe_profile_falls_back_to_declared_order() {
        let tools = vec![
            "custom_first".to_string(),
            "custom_second".to_string(),
            "custom_third".to_string(),
        ];
        let (first, second) = choose_worker_tools(&tools, "safe");
        assert_eq!(first.as_deref(), Some("custom_first"));
        assert_eq!(second.as_deref(), Some("custom_second"));
    }

    #[test]
    fn choose_worker_tools_non_safe_follows_declared_order() {
        let tools = vec![
            "custom_a".to_string(),
            "custom_b".to_string(),
            "custom_c".to_string(),
        ];
        let (first, second) = choose_worker_tools(&tools, "demo");
        assert_eq!(first.as_deref(), Some("custom_a"));
        assert_eq!(second.as_deref(), Some("custom_b"));
    }

    #[test]
    fn mother_dispatch_response_carries_planned_team_run_id() {
        let scenario = MockScenario {
            run_id: "demo_run_1".to_string(),
            team_run_id: "team_demo_1".to_string(),
            workers: vec![DemoWorkerPlan {
                agent_id: "worker_a".to_string(),
                tool_step_one: Some("list_files".to_string()),
                tool_step_two: Some("search_content".to_string()),
            }],
            seed: 7,
            speed: "normal".to_string(),
            wait_seconds: 6.0,
            worker_sleep_seconds: 0.5,
        };

        let response = mother_dispatch_response(&scenario);
        let args = response["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .expect("tool call args");
        let payload: Value = serde_json::from_str(args).expect("parse tool args");

        assert_eq!(
            payload.get("teamRunId").and_then(Value::as_str),
            Some("team_demo_1")
        );
    }

    #[test]
    fn extract_message_value_handles_semicolon_delimited_pairs() {
        let message =
            "BEEROOM_DEMO_WORKER_TASK: worker=w-1; run_id=r-1; tool1=list_files; tool2=read_file";
        assert_eq!(
            extract_message_value(message, "worker").as_deref(),
            Some("w-1")
        );
        assert_eq!(
            extract_message_value(message, "tool1").as_deref(),
            Some("list_files")
        );
        assert_eq!(
            extract_message_value(message, "tool2").as_deref(),
            Some("read_file")
        );
        assert_eq!(extract_message_value(message, "missing"), None);
    }

    #[test]
    fn hive_id_normalization_is_stable_for_demo_scope() {
        let normalized = normalize_hive_id("  demo-hive  ");
        assert_eq!(normalized, "demo-hive");
    }

    #[test]
    fn swarm_resolvable_agent_accepts_virtual_default_alias_without_record() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("beeroom-demo-resolvable.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());

        let can_resolve =
            is_swarm_resolvable_agent(&storage, "u1", "__default__").expect("check resolvable");
        assert!(can_resolve);
    }

    #[test]
    fn swarm_resolvable_agent_accepts_persisted_agent() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("beeroom-demo-resolvable-persisted.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        let agent = UserAgentRecord {
            agent_id: "agent_worker_1".to_string(),
            user_id: "u1".to_string(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            name: "Worker 1".to_string(),
            description: String::new(),
            system_prompt: String::new(),
            model_name: None,
            ability_items: Vec::new(),
            tool_names: vec!["list_files".to_string()],
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            access_level: "A".to_string(),
            approval_mode: "full_auto".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: 0,
            created_at: 1.0,
            updated_at: 1.0,
            preset_binding: None,
        };
        storage
            .upsert_user_agent(&agent)
            .expect("upsert persisted agent");

        let can_resolve =
            is_swarm_resolvable_agent(&storage, "u1", "agent_worker_1").expect("check resolvable");
        assert!(can_resolve);
    }
}
