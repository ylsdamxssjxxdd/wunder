use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use uuid::Uuid;
use wunder_server::config::LlmModelConfig;
use wunder_server::config_store::ConfigStore;
use wunder_server::state::AppState;
use wunder_server::storage::{
    ChatSessionRecord, TeamRunRecord, TeamTaskRecord, UserAgentRecord, DEFAULT_HIVE_ID,
};

const DEFAULT_RUNS: usize = 12;
const DEFAULT_TASKS_PER_RUN: usize = 8;
const DEFAULT_AGENT_COUNT: usize = 8;
const DEFAULT_MAX_ACTIVE_RUNS: usize = 6;
const DEFAULT_MAX_PARALLEL_TASKS: usize = 4;
const DEFAULT_TASK_TIMEOUT_S: u64 = 45;
const DEFAULT_MAX_WAIT_S: u64 = 300;
const DEFAULT_POLL_MS: u64 = 200;
const MOCK_MODEL_NAME: &str = "__swarm_bench_mock__";

const TEAM_RUN_TERMINAL: [&str; 4] = ["success", "failed", "timeout", "cancelled"];
const TEAM_TASK_TERMINAL: [&str; 4] = ["success", "failed", "timeout", "cancelled"];

const QUESTION_TEMPLATES: [&str; 8] = [
    "Build a concise incident timeline with assumptions and open risks.",
    "Summarize key findings and produce three actionable next steps.",
    "Extract constraints, dependencies, and execution checkpoints.",
    "Compare two implementation paths and estimate delivery risk.",
    "List likely failure modes and propose mitigation controls.",
    "Create a short handoff note for engineering and operations.",
    "Draft a minimal validation plan with measurable acceptance criteria.",
    "Provide a compact status update for stakeholders in bullet points.",
];

#[derive(Debug, Clone)]
struct BenchArgs {
    runs: usize,
    tasks_per_run: usize,
    agent_count: usize,
    max_active_runs: usize,
    max_parallel_tasks: usize,
    task_timeout_s: u64,
    max_wait_s: u64,
    poll_ms: u64,
    same_agent: bool,
    keep_artifacts: bool,
    output: Option<PathBuf>,
}

impl Default for BenchArgs {
    fn default() -> Self {
        Self {
            runs: DEFAULT_RUNS,
            tasks_per_run: DEFAULT_TASKS_PER_RUN,
            agent_count: DEFAULT_AGENT_COUNT,
            max_active_runs: DEFAULT_MAX_ACTIVE_RUNS,
            max_parallel_tasks: DEFAULT_MAX_PARALLEL_TASKS,
            task_timeout_s: DEFAULT_TASK_TIMEOUT_S,
            max_wait_s: DEFAULT_MAX_WAIT_S,
            poll_ms: DEFAULT_POLL_MS,
            same_agent: false,
            keep_artifacts: true,
            output: None,
        }
    }
}

impl BenchArgs {
    fn parse_from_env() -> Result<Self> {
        let mut args = Self::default();
        let mut iter = env::args().skip(1);
        while let Some(flag) = iter.next() {
            match flag.as_str() {
                "--help" | "-h" => {
                    Self::print_help();
                    std::process::exit(0);
                }
                "--runs" => args.runs = parse_usize_flag("--runs", iter.next())?,
                "--tasks-per-run" => {
                    args.tasks_per_run = parse_usize_flag("--tasks-per-run", iter.next())?
                }
                "--agent-count" => {
                    args.agent_count = parse_usize_flag("--agent-count", iter.next())?
                }
                "--max-active-runs" => {
                    args.max_active_runs = parse_usize_flag("--max-active-runs", iter.next())?
                }
                "--max-parallel-tasks" => {
                    args.max_parallel_tasks = parse_usize_flag("--max-parallel-tasks", iter.next())?
                }
                "--task-timeout-s" => {
                    args.task_timeout_s = parse_u64_flag("--task-timeout-s", iter.next())?
                }
                "--max-wait-s" => args.max_wait_s = parse_u64_flag("--max-wait-s", iter.next())?,
                "--poll-ms" => args.poll_ms = parse_u64_flag("--poll-ms", iter.next())?,
                "--same-agent" => args.same_agent = true,
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

        args.runs = args.runs.max(1);
        args.tasks_per_run = args.tasks_per_run.max(1);
        args.agent_count = args.agent_count.max(1);
        args.max_active_runs = args.max_active_runs.max(1);
        args.max_parallel_tasks = args.max_parallel_tasks.max(1);
        args.task_timeout_s = args.task_timeout_s.max(1);
        args.max_wait_s = args.max_wait_s.max(5);
        args.poll_ms = args.poll_ms.max(20);

        Ok(args)
    }

    fn print_help() {
        println!(
            "swarm_sim: deterministic TeamRun benchmark without external LLM output\n\n\
             Flags:\n\
             --runs <N>                number of team runs (default: {DEFAULT_RUNS})\n\
             --tasks-per-run <N>       tasks per run (default: {DEFAULT_TASKS_PER_RUN})\n\
             --agent-count <N>         number of worker agents (default: {DEFAULT_AGENT_COUNT})\n\
             --max-active-runs <N>     runner max active team runs (default: {DEFAULT_MAX_ACTIVE_RUNS})\n\
             --max-parallel-tasks <N>  max parallel tasks per run (default: {DEFAULT_MAX_PARALLEL_TASKS})\n\
             --task-timeout-s <S>      timeout per task in seconds (default: {DEFAULT_TASK_TIMEOUT_S})\n\
             --max-wait-s <S>          global wait timeout in seconds (default: {DEFAULT_MAX_WAIT_S})\n\
             --poll-ms <MS>            polling interval in milliseconds (default: {DEFAULT_POLL_MS})\n\
             --same-agent              force all tasks to use one agent (stress same-app concurrency)\n\
             --cleanup                 remove generated benchmark artifacts on exit\n\
             --output <PATH>           write full JSON report to a file\n\
             --help                    show this help message"
        );
    }
}

#[derive(Debug, Serialize)]
struct BenchReport {
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    wall_time_s: f64,
    config: BenchConfigSnapshot,
    artifacts: ArtifactSnapshot,
    team_runs_total: usize,
    team_tasks_total: usize,
    session_runs_total: usize,
    run_status: BTreeMap<String, usize>,
    task_status: BTreeMap<String, usize>,
    run_elapsed_s: Option<DistributionStats>,
    task_elapsed_s: Option<DistributionStats>,
    queue_wait_s: Option<DistributionStats>,
    task_throughput_per_s: f64,
    peak_task_concurrency: usize,
    concurrency_utilization: f64,
    completed_ratio: f64,
    slowest_runs: Vec<RunSample>,
}

#[derive(Debug, Serialize)]
struct BenchConfigSnapshot {
    runs: usize,
    tasks_per_run: usize,
    agent_count: usize,
    max_active_runs: usize,
    max_parallel_tasks: usize,
    task_timeout_s: u64,
    max_wait_s: u64,
    poll_ms: u64,
    same_agent: bool,
    mock_llm: bool,
}

#[derive(Debug, Serialize)]
struct ArtifactSnapshot {
    root: String,
    db_path: String,
    workspace_root: String,
    override_path: String,
    kept: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DistributionStats {
    count: usize,
    min: f64,
    max: f64,
    avg: f64,
    p50: f64,
    p90: f64,
    p99: f64,
}

#[derive(Debug, Clone, Serialize)]
struct RunSample {
    team_run_id: String,
    status: String,
    elapsed_s: f64,
    task_success: i64,
    task_failed: i64,
}

#[derive(Debug)]
struct SessionRunSample {
    status: String,
    queued_time: f64,
    started_time: f64,
    finished_time: f64,
}

#[derive(Debug)]
struct ArtifactPaths {
    root: PathBuf,
    db_path: PathBuf,
    workspace_root: PathBuf,
    override_path: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = BenchArgs::parse_from_env()?;
    let started_at = Utc::now();
    let wall_started = Instant::now();

    let artifacts = create_artifacts_root()?;
    let state = init_bench_state(&args, &artifacts).await?;

    let user_id = format!("swarm_bench_{}", Uuid::new_v4().simple());
    let agent_ids = seed_agents(state.as_ref(), &user_id, args.agent_count)?;

    println!(
        "[swarm_sim] submit team runs={}, tasks_per_run={}, agents={}, same_agent={}",
        args.runs, args.tasks_per_run, args.agent_count, args.same_agent
    );

    let team_run_ids = create_and_enqueue_runs(state.as_ref(), &args, &user_id, &agent_ids).await?;

    let runs = wait_for_runs(state.as_ref(), &team_run_ids, &args).await?;
    let tasks = collect_all_tasks(state.as_ref(), &team_run_ids)?;
    let session_runs = load_session_runs(&artifacts.db_path, &user_id)?;

    let wall_time_s = wall_started.elapsed().as_secs_f64();
    let finished_at = Utc::now();

    let report = build_report(BuildReportInput {
        args: &args,
        artifacts: &artifacts,
        started_at,
        finished_at,
        wall_time_s,
        runs,
        tasks,
        session_runs,
    });

    print_report_summary(&report);

    if let Some(output) = args.output.as_ref() {
        if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create output parent directory: {}",
                    parent.display()
                )
            })?;
        }
        fs::write(output, serde_json::to_string_pretty(&report)?)
            .with_context(|| format!("failed to write report: {}", output.display()))?;
        println!("[swarm_sim] report written: {}", output.display());
    }

    if !args.keep_artifacts {
        if let Err(err) = fs::remove_dir_all(&artifacts.root) {
            eprintln!(
                "[swarm_sim] cleanup failed for {}: {err}",
                artifacts.root.display()
            );
        }
    }

    Ok(())
}

fn parse_usize_flag(name: &str, value: Option<String>) -> Result<usize> {
    let raw = value.ok_or_else(|| anyhow!("{name} requires a value"))?;
    raw.parse::<usize>()
        .with_context(|| format!("invalid value for {name}: {raw}"))
}

fn parse_u64_flag(name: &str, value: Option<String>) -> Result<u64> {
    let raw = value.ok_or_else(|| anyhow!("{name} requires a value"))?;
    raw.parse::<u64>()
        .with_context(|| format!("invalid value for {name}: {raw}"))
}

fn create_artifacts_root() -> Result<ArtifactPaths> {
    let root = env::temp_dir().join(format!("wunder_swarm_sim_{}", Uuid::new_v4().simple()));
    let workspace_root = root.join("workspace");
    let override_path = root.join("wunder.swarm.override.yaml");
    let db_path = root.join("wunder.swarm.sqlite3");
    fs::create_dir_all(&workspace_root).with_context(|| {
        format!(
            "failed to create workspace root: {}",
            workspace_root.display()
        )
    })?;
    Ok(ArtifactPaths {
        root,
        db_path,
        workspace_root,
        override_path,
    })
}

async fn init_bench_state(args: &BenchArgs, artifacts: &ArtifactPaths) -> Result<Arc<AppState>> {
    let config_store = ConfigStore::new(artifacts.override_path.clone());
    let mock_model = mock_llm_model_config()?;
    let db_path = artifacts.db_path.to_string_lossy().to_string();
    let workspace_root = artifacts.workspace_root.to_string_lossy().to_string();
    let max_active_runs = args.max_active_runs;
    let max_parallel_tasks = args.max_parallel_tasks;
    let task_timeout_s = args.task_timeout_s;

    config_store
        .update(move |cfg| {
            cfg.storage.backend = "sqlite".to_string();
            cfg.storage.db_path = db_path.clone();
            cfg.workspace.root = workspace_root.clone();

            cfg.tools.swarm.runner = "teamrun".to_string();
            cfg.tools.swarm.max_active_team_runs = max_active_runs;
            cfg.tools.swarm.max_parallel_tasks_per_team = max_parallel_tasks;
            cfg.tools.swarm.default_timeout_s = task_timeout_s;
            cfg.tools.swarm.max_retry = 0;

            cfg.llm.default = MOCK_MODEL_NAME.to_string();
            cfg.llm.models.clear();
            cfg.llm
                .models
                .insert(MOCK_MODEL_NAME.to_string(), mock_model.clone());
        })
        .await?;

    let config = config_store.get().await;
    let state = AppState::new(config_store, config)?;
    Ok(Arc::new(state))
}

fn mock_llm_model_config() -> Result<LlmModelConfig> {
    serde_json::from_value(json!({
        "mock_if_unconfigured": true,
        "stream": false,
        "retry": 0,
        "max_rounds": 1,
        "tool_call_mode": "tool_call"
    }))
    .context("failed to build mock llm model config")
}

fn seed_agents(state: &AppState, user_id: &str, agent_count: usize) -> Result<Vec<String>> {
    state.user_store.ensure_default_hive(user_id)?;
    let now = now_ts();
    let mut agent_ids = Vec::with_capacity(agent_count);

    for index in 0..agent_count {
        let agent_id = format!("agent_{}_{}", index + 1, Uuid::new_v4().simple());
        let record = UserAgentRecord {
            agent_id: agent_id.clone(),
            user_id: user_id.to_string(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            name: format!("SwarmBenchAgent{}", index + 1),
            description: "Deterministic swarm benchmark worker".to_string(),
            system_prompt: "Return a short deterministic completion for benchmark validation."
                .to_string(),
            tool_names: Vec::new(),
            access_level: "A".to_string(),
            approval_mode: "full_auto".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: (index + 1) as i32,
            created_at: now,
            updated_at: now,
        };
        state.user_store.upsert_user_agent(&record)?;
        agent_ids.push(agent_id);
    }

    Ok(agent_ids)
}

async fn create_and_enqueue_runs(
    state: &AppState,
    args: &BenchArgs,
    user_id: &str,
    agent_ids: &[String],
) -> Result<Vec<String>> {
    let mut team_run_ids = Vec::with_capacity(args.runs);

    for run_index in 0..args.runs {
        let now = now_ts();
        let parent_session_id = format!("sess_parent_{}_{}", run_index, Uuid::new_v4().simple());
        let question = QUESTION_TEMPLATES[run_index % QUESTION_TEMPLATES.len()];

        let session = ChatSessionRecord {
            session_id: parent_session_id.clone(),
            user_id: user_id.to_string(),
            title: format!("SwarmBenchParent{}", run_index + 1),
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: None,
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        };
        state.user_store.upsert_chat_session(&session)?;
        state
            .monitor
            .register(&parent_session_id, user_id, "", question, false, false);

        let team_run_id = format!("team_{}_{}", run_index, Uuid::new_v4().simple());
        let run = TeamRunRecord {
            team_run_id: team_run_id.clone(),
            user_id: user_id.to_string(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            parent_session_id: parent_session_id.clone(),
            parent_agent_id: None,
            strategy: "parallel_all".to_string(),
            // Avoid runner picking up this run before all tasks are persisted.
            status: "preparing".to_string(),
            task_total: args.tasks_per_run as i64,
            task_success: 0,
            task_failed: 0,
            context_tokens_total: 0,
            context_tokens_peak: 0,
            model_round_total: 0,
            started_time: Some(now),
            finished_time: None,
            elapsed_s: None,
            summary: Some(format!(
                "merge_policy=collect; timeout_s={}",
                args.task_timeout_s
            )),
            error: None,
            updated_time: now,
        };
        state.user_store.upsert_team_run(&run)?;

        for task_index in 0..args.tasks_per_run {
            let agent_id = if args.same_agent {
                agent_ids[0].clone()
            } else {
                agent_ids[(run_index + task_index) % agent_ids.len()].clone()
            };
            let task = TeamTaskRecord {
                task_id: format!(
                    "task_{}_{}_{}",
                    run_index,
                    task_index,
                    Uuid::new_v4().simple()
                ),
                team_run_id: team_run_id.clone(),
                user_id: user_id.to_string(),
                hive_id: DEFAULT_HIVE_ID.to_string(),
                agent_id,
                target_session_id: None,
                spawned_session_id: None,
                status: "queued".to_string(),
                retry_count: 0,
                priority: (args.tasks_per_run - task_index) as i64,
                started_time: None,
                finished_time: None,
                elapsed_s: None,
                result_summary: None,
                error: None,
                updated_time: now,
            };
            state.user_store.upsert_team_task(&task)?;
        }

        let mut queued = run.clone();
        queued.status = "queued".to_string();
        queued.updated_time = now_ts();
        state.user_store.upsert_team_run(&queued)?;

        state.team_run_runner.enqueue(&team_run_id).await;
        team_run_ids.push(team_run_id);
    }

    Ok(team_run_ids)
}

async fn wait_for_runs(
    state: &AppState,
    team_run_ids: &[String],
    args: &BenchArgs,
) -> Result<Vec<TeamRunRecord>> {
    let deadline = Instant::now() + Duration::from_secs(args.max_wait_s);

    loop {
        let mut runs = Vec::with_capacity(team_run_ids.len());
        let mut done = 0usize;
        let mut status_count: HashMap<String, usize> = HashMap::new();

        for team_run_id in team_run_ids {
            let run = state
                .user_store
                .get_team_run(team_run_id)?
                .ok_or_else(|| anyhow!("missing team run: {team_run_id}"))?;
            let normalized = normalize_status(&run.status);
            *status_count.entry(normalized.clone()).or_insert(0) += 1;
            if TEAM_RUN_TERMINAL.contains(&normalized.as_str()) {
                done += 1;
            }
            runs.push(run);
        }

        if done == team_run_ids.len() {
            return Ok(runs);
        }

        if Instant::now() >= deadline {
            return Err(anyhow!(
                "team runs not completed before timeout, done={done}/{}, status={status_count:?}",
                team_run_ids.len()
            ));
        }

        sleep(Duration::from_millis(args.poll_ms)).await;
    }
}

fn collect_all_tasks(state: &AppState, team_run_ids: &[String]) -> Result<Vec<TeamTaskRecord>> {
    let mut tasks = Vec::new();
    for team_run_id in team_run_ids {
        tasks.extend(state.user_store.list_team_tasks(team_run_id)?);
    }
    Ok(tasks)
}

fn load_session_runs(db_path: &Path, user_id: &str) -> Result<Vec<SessionRunSample>> {
    let conn = Connection::open(db_path)
        .with_context(|| format!("failed to open sqlite db: {}", db_path.display()))?;
    let mut stmt = conn.prepare(
        "SELECT status, \
                COALESCE(queued_time, 0), \
                COALESCE(started_time, 0), \
                COALESCE(finished_time, 0) \
         FROM session_runs \
         WHERE user_id = ? \
         ORDER BY queued_time ASC",
    )?;

    let rows = stmt.query_map([user_id], |row| {
        Ok(SessionRunSample {
            status: row.get::<_, String>(0)?,
            queued_time: row.get::<_, f64>(1)?,
            started_time: row.get::<_, f64>(2)?,
            finished_time: row.get::<_, f64>(3)?,
        })
    })?;

    let mut output = Vec::new();
    for row in rows {
        output.push(row?);
    }
    Ok(output)
}

struct BuildReportInput<'a> {
    args: &'a BenchArgs,
    artifacts: &'a ArtifactPaths,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    wall_time_s: f64,
    runs: Vec<TeamRunRecord>,
    tasks: Vec<TeamTaskRecord>,
    session_runs: Vec<SessionRunSample>,
}

fn build_report(input: BuildReportInput<'_>) -> BenchReport {
    let BuildReportInput {
        args,
        artifacts,
        started_at,
        finished_at,
        wall_time_s,
        runs,
        tasks,
        session_runs,
    } = input;
    let run_status = count_status(runs.iter().map(|run| run.status.as_str()));
    let task_status = count_status(tasks.iter().map(|task| task.status.as_str()));

    let run_elapsed_values = runs
        .iter()
        .filter_map(|run| run.elapsed_s)
        .filter(|value| *value >= 0.0)
        .collect::<Vec<_>>();

    let task_elapsed_values = tasks
        .iter()
        .filter_map(|task| task.elapsed_s)
        .filter(|value| *value >= 0.0)
        .collect::<Vec<_>>();

    let queue_wait_values = session_runs
        .iter()
        .filter_map(|row| {
            if row.started_time > 0.0 && row.queued_time > 0.0 {
                Some((row.started_time - row.queued_time).max(0.0))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let completed_tasks = tasks
        .iter()
        .filter(|task| TEAM_TASK_TERMINAL.contains(&normalize_status(&task.status).as_str()))
        .count();

    let completed_ratio = if tasks.is_empty() {
        0.0
    } else {
        completed_tasks as f64 / tasks.len() as f64
    };

    let peak_task_concurrency = estimate_peak_concurrency(&session_runs);
    let task_elapsed_sum: f64 = task_elapsed_values.iter().sum();
    let concurrency_denominator = wall_time_s * peak_task_concurrency.max(1) as f64;
    let concurrency_utilization = if concurrency_denominator > 0.0 {
        (task_elapsed_sum / concurrency_denominator).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let task_throughput_per_s = if wall_time_s > 0.0 {
        completed_tasks as f64 / wall_time_s
    } else {
        0.0
    };

    let mut slowest_runs = runs
        .iter()
        .map(|run| RunSample {
            team_run_id: run.team_run_id.clone(),
            status: run.status.clone(),
            elapsed_s: run.elapsed_s.unwrap_or_default(),
            task_success: run.task_success,
            task_failed: run.task_failed,
        })
        .collect::<Vec<_>>();
    slowest_runs.sort_by(|a, b| b.elapsed_s.total_cmp(&a.elapsed_s));
    slowest_runs.truncate(5);

    BenchReport {
        started_at,
        finished_at,
        wall_time_s,
        config: BenchConfigSnapshot {
            runs: args.runs,
            tasks_per_run: args.tasks_per_run,
            agent_count: args.agent_count,
            max_active_runs: args.max_active_runs,
            max_parallel_tasks: args.max_parallel_tasks,
            task_timeout_s: args.task_timeout_s,
            max_wait_s: args.max_wait_s,
            poll_ms: args.poll_ms,
            same_agent: args.same_agent,
            mock_llm: true,
        },
        artifacts: ArtifactSnapshot {
            root: artifacts.root.to_string_lossy().to_string(),
            db_path: artifacts.db_path.to_string_lossy().to_string(),
            workspace_root: artifacts.workspace_root.to_string_lossy().to_string(),
            override_path: artifacts.override_path.to_string_lossy().to_string(),
            kept: args.keep_artifacts,
        },
        team_runs_total: runs.len(),
        team_tasks_total: tasks.len(),
        session_runs_total: session_runs.len(),
        run_status,
        task_status,
        run_elapsed_s: distribution_stats(&run_elapsed_values),
        task_elapsed_s: distribution_stats(&task_elapsed_values),
        queue_wait_s: distribution_stats(&queue_wait_values),
        task_throughput_per_s,
        peak_task_concurrency,
        concurrency_utilization,
        completed_ratio,
        slowest_runs,
    }
}

fn count_status<'a>(statuses: impl Iterator<Item = &'a str>) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for status in statuses {
        *map.entry(normalize_status(status)).or_insert(0) += 1;
    }
    map
}

fn normalize_status(status: &str) -> String {
    status.trim().to_ascii_lowercase()
}

fn distribution_stats(values: &[f64]) -> Option<DistributionStats> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(f64::total_cmp);
    let sum: f64 = sorted.iter().sum();
    Some(DistributionStats {
        count: sorted.len(),
        min: *sorted.first().unwrap_or(&0.0),
        max: *sorted.last().unwrap_or(&0.0),
        avg: sum / sorted.len() as f64,
        p50: percentile(&sorted, 50.0),
        p90: percentile(&sorted, 90.0),
        p99: percentile(&sorted, 99.0),
    })
}

fn percentile(sorted_values: &[f64], p: f64) -> f64 {
    if sorted_values.is_empty() {
        return 0.0;
    }
    let clamped = p.clamp(0.0, 100.0) / 100.0;
    let rank = ((sorted_values.len() - 1) as f64 * clamped).round() as usize;
    sorted_values[rank.min(sorted_values.len() - 1)]
}

fn estimate_peak_concurrency(session_runs: &[SessionRunSample]) -> usize {
    let mut events = Vec::new();
    for run in session_runs {
        let status = normalize_status(&run.status);
        if run.started_time <= 0.0 || run.finished_time <= 0.0 {
            continue;
        }
        if !TEAM_TASK_TERMINAL.contains(&status.as_str()) {
            continue;
        }
        events.push((run.started_time, 1i32));
        events.push((run.finished_time, -1i32));
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

fn print_report_summary(report: &BenchReport) {
    println!("[swarm_sim] done");
    println!("[swarm_sim] wall_time_s={:.3}", report.wall_time_s);
    println!(
        "[swarm_sim] runs={} tasks={} session_runs={}",
        report.team_runs_total, report.team_tasks_total, report.session_runs_total
    );
    println!("[swarm_sim] run_status={:?}", report.run_status);
    println!("[swarm_sim] task_status={:?}", report.task_status);
    println!(
        "[swarm_sim] throughput={:.3} task/s peak_concurrency={} utilization={:.3} completed_ratio={:.3}",
        report.task_throughput_per_s,
        report.peak_task_concurrency,
        report.concurrency_utilization,
        report.completed_ratio,
    );
    if let Some(stats) = report.task_elapsed_s.as_ref() {
        println!(
            "[swarm_sim] task_elapsed_s avg={:.3} p50={:.3} p90={:.3} p99={:.3} max={:.3}",
            stats.avg, stats.p50, stats.p90, stats.p99, stats.max
        );
    }
    if let Some(stats) = report.queue_wait_s.as_ref() {
        println!(
            "[swarm_sim] queue_wait_s avg={:.3} p50={:.3} p90={:.3} p99={:.3} max={:.3}",
            stats.avg, stats.p50, stats.p90, stats.p99, stats.max
        );
    }
    println!("[swarm_sim] artifacts_root={}", report.artifacts.root);
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}
