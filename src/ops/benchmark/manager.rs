use super::aggregate::{build_run_summary, build_task_aggregate};
use super::executor::execute_prompt;
use super::grader_auto::grade_automated;
use super::grader_judge::grade_with_judge;
use super::loader::{default_tasks_dir, load_task_specs};
use super::models::BenchmarkEvent;
use super::spec::{BenchmarkGradingType, BenchmarkTaskSpec};
use super::workspace::{
    apply_task_placeholders, build_artifact_manifest, build_attempt_root, prepare_attempt_workspace,
};
use crate::config::Config;
use crate::config_store::ConfigStore;
use crate::i18n;
use crate::llm::is_llm_model;
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::schemas::WunderRequest;
use crate::skills::SkillRegistry;
use crate::storage::StorageBackend;
use crate::tools::{builtin_aliases, collect_available_tool_names, resolve_tool_name};
use crate::user_tools::{UserToolBindings, UserToolManager};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};
use uuid::Uuid;

use super::models::BenchmarkStartRequest;

#[derive(Clone)]
pub struct BenchmarkManager {
    config_store: ConfigStore,
    storage: Arc<dyn StorageBackend>,
    workspace: Arc<WorkspaceManager>,
    monitor: Arc<MonitorState>,
    orchestrator: Arc<Orchestrator>,
    skills: Arc<RwLock<SkillRegistry>>,
    user_tool_manager: Arc<UserToolManager>,
    state: Arc<Mutex<BenchmarkState>>,
}

struct BenchmarkState {
    runs: HashMap<String, BenchmarkRunHandle>,
}

struct BenchmarkRunHandle {
    cancel_flag: Arc<AtomicBool>,
    sender: broadcast::Sender<BenchmarkEvent>,
    current_session_id: Option<String>,
}

struct BenchmarkRunContext {
    run_id: String,
    started_time: f64,
    user_id: String,
    model_name: Option<String>,
    judge_model_name: Option<String>,
    suite_ids: Vec<String>,
    requested_tool_names: Vec<String>,
    tool_snapshot: Vec<String>,
    runs_per_task: u32,
    capture_artifacts: bool,
    capture_transcript: bool,
    config_overrides: Option<Value>,
    tasks: Vec<BenchmarkTaskSpec>,
    cancel_flag: Arc<AtomicBool>,
    sender: broadcast::Sender<BenchmarkEvent>,
    storage: Arc<dyn StorageBackend>,
    workspace: Arc<WorkspaceManager>,
    monitor: Arc<MonitorState>,
    orchestrator: Arc<Orchestrator>,
    state: Arc<Mutex<BenchmarkState>>,
    config: Config,
}

impl BenchmarkManager {
    pub fn new(
        config_store: ConfigStore,
        storage: Arc<dyn StorageBackend>,
        workspace: Arc<WorkspaceManager>,
        monitor: Arc<MonitorState>,
        orchestrator: Arc<Orchestrator>,
        skills: Arc<RwLock<SkillRegistry>>,
        user_tool_manager: Arc<UserToolManager>,
    ) -> Self {
        Self {
            config_store,
            storage,
            workspace,
            monitor,
            orchestrator,
            skills,
            user_tool_manager,
            state: Arc::new(Mutex::new(BenchmarkState {
                runs: HashMap::new(),
            })),
        }
    }

    pub async fn start(&self, request: BenchmarkStartRequest) -> Result<Value> {
        let user_id = request.user_id.trim();
        if user_id.is_empty() {
            return Err(anyhow!("user_id required"));
        }
        let config = self.config_store.get().await;
        let model_name = resolve_model_name(request.model_name.as_deref(), &config);
        let judge_model_name = resolve_model_name(request.judge_model_name.as_deref(), &config)
            .or_else(|| model_name.clone());
        let skills_snapshot = self.skills.read().await.clone();
        let user_tool_bindings =
            self.user_tool_manager
                .build_bindings(&config, &skills_snapshot, user_id);
        let allowed_tool_names = resolve_allowed_tool_names(
            &config,
            &skills_snapshot,
            Some(&user_tool_bindings),
            &request.tool_names,
        );

        let mut tasks = load_task_specs(&default_tasks_dir())?;
        tasks = filter_tasks(tasks, &request.suite_ids, &request.task_ids);
        if tasks.is_empty() {
            return Err(anyhow!("no benchmark tasks available"));
        }
        let suite_ids = if request.suite_ids.is_empty() {
            let mut values = tasks
                .iter()
                .map(|task| task.frontmatter.suite.clone())
                .collect::<Vec<_>>();
            values.sort();
            values.dedup();
            values
        } else {
            request.suite_ids.clone()
        };
        let runs_per_task = request.runs_per_task.unwrap_or(3).clamp(1, 10);
        let capture_artifacts = request.capture_artifacts.unwrap_or(true);
        let capture_transcript = request.capture_transcript.unwrap_or(true);
        let run_id = Uuid::new_v4().simple().to_string();
        let started_time = now_ts();
        let mut tool_snapshot = allowed_tool_names.into_iter().collect::<Vec<_>>();
        tool_snapshot.sort();
        let run_payload = json!({
            "run_id": run_id,
            "user_id": user_id,
            "model_name": model_name.clone().unwrap_or_default(),
            "judge_model_name": judge_model_name.clone().unwrap_or_default(),
            "suite_ids": suite_ids.clone(),
            "tool_names": request.tool_names,
            "tool_snapshot": tool_snapshot.clone(),
            "status": "running",
            "task_count": tasks.len(),
            "attempt_count": tasks.len() as u32 * runs_per_task,
            "total_score": 0.0,
            "started_time": started_time,
            "finished_time": 0.0,
            "elapsed_s": 0.0,
            "capture_artifacts": capture_artifacts,
            "capture_transcript": capture_transcript,
            "config_overrides": request.config_overrides.clone().unwrap_or(Value::Null),
        });
        self.storage.create_benchmark_run(&run_payload)?;

        let (sender, _) = broadcast::channel(256);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        {
            let mut guard = self.state.lock().await;
            guard.runs.insert(
                run_id.clone(),
                BenchmarkRunHandle {
                    cancel_flag: cancel_flag.clone(),
                    sender: sender.clone(),
                    current_session_id: None,
                },
            );
        }

        let ctx = BenchmarkRunContext {
            run_id: run_id.clone(),
            started_time,
            user_id: user_id.to_string(),
            model_name,
            judge_model_name,
            suite_ids: suite_ids.clone(),
            requested_tool_names: request.tool_names.clone(),
            tool_snapshot: tool_snapshot.clone(),
            runs_per_task,
            capture_artifacts,
            capture_transcript,
            config_overrides: request.config_overrides.clone(),
            tasks,
            cancel_flag,
            sender,
            storage: self.storage.clone(),
            workspace: self.workspace.clone(),
            monitor: self.monitor.clone(),
            orchestrator: self.orchestrator.clone(),
            state: self.state.clone(),
            config,
        };
        tokio::spawn(async move {
            run_benchmark(ctx).await;
        });
        Ok(json!({
            "run_id": run_id,
            "status": "running",
            "task_count": run_payload["task_count"],
            "attempt_count": run_payload["attempt_count"],
            "suite_ids": suite_ids,
        }))
    }

    pub async fn cancel(&self, run_id: &str) -> Result<Value> {
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Err(anyhow!("run_id required"));
        }
        let mut guard = self.state.lock().await;
        if let Some(handle) = guard.runs.get_mut(cleaned) {
            handle.cancel_flag.store(true, Ordering::SeqCst);
            if let Some(session_id) = handle.current_session_id.clone() {
                self.monitor.cancel(&session_id);
            }
            let _ = handle.sender.send(BenchmarkEvent {
                event: "benchmark_log".to_string(),
                data: json!({ "run_id": cleaned, "message": "cancel requested" }),
            });
            return Ok(json!({ "ok": true, "run_id": cleaned, "message": "cancel requested" }));
        }
        drop(guard);

        let Some(mut run_payload) = self.storage.load_benchmark_run(cleaned)? else {
            return Err(anyhow!("run not found"));
        };
        let status = run_payload
            .get("status")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_lowercase())
            .unwrap_or_default();
        if status != "running" {
            let message = if status == "cancelled" {
                "run already cancelled"
            } else if status.is_empty() {
                "run not running"
            } else {
                "run already finished"
            };
            return Ok(json!({
                "ok": true,
                "run_id": cleaned,
                "status": status,
                "message": message,
            }));
        }

        let task_aggregates = self.storage.load_benchmark_task_aggregates(cleaned)?;
        let attempts = self.storage.load_benchmark_attempts(cleaned)?;
        let finished_time = now_ts();
        let started_time = run_payload
            .get("started_time")
            .and_then(Value::as_f64)
            .unwrap_or(finished_time);
        let suite_ids = read_string_list(run_payload.get("suite_ids"));
        let tool_snapshot = read_string_list(run_payload.get("tool_snapshot"));
        let config_overrides = run_payload
            .get("config_overrides")
            .cloned()
            .unwrap_or(Value::Null);
        let summary = build_run_summary(
            cleaned,
            run_payload
                .get("user_id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            run_payload
                .get("model_name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            run_payload
                .get("judge_model_name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            &suite_ids,
            &tool_snapshot,
            &task_aggregates,
            &attempts,
            started_time,
            finished_time,
            "cancelled",
            &config_overrides,
        );

        if let Value::Object(ref mut map) = run_payload {
            map.insert("status".to_string(), Value::String("cancelled".to_string()));
            map.insert("task_count".to_string(), json!(task_aggregates.len()));
            map.insert("attempt_count".to_string(), json!(attempts.len()));
            map.insert("total_score".to_string(), summary["total_score"].clone());
            map.insert("summary".to_string(), summary);
            map.insert("finished_time".to_string(), json!(finished_time));
            map.insert(
                "elapsed_s".to_string(),
                json!((finished_time - started_time).max(0.0)),
            );
            map.insert("suite_ids".to_string(), json!(suite_ids));
            map.insert("tool_snapshot".to_string(), json!(tool_snapshot));
            map.insert("config_overrides".to_string(), config_overrides);
        }
        self.storage.update_benchmark_run(cleaned, &run_payload)?;
        Ok(json!({
            "ok": true,
            "run_id": cleaned,
            "status": "cancelled",
            "message": "stale run marked cancelled",
        }))
    }

    pub async fn subscribe(&self, run_id: &str) -> Option<broadcast::Receiver<BenchmarkEvent>> {
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return None;
        }
        let guard = self.state.lock().await;
        guard
            .runs
            .get(cleaned)
            .map(|handle| handle.sender.subscribe())
    }

    pub fn load_runs(
        &self,
        user_id: Option<&str>,
        status: Option<&str>,
        model_name: Option<&str>,
        since_time: Option<f64>,
        until_time: Option<f64>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        self.storage
            .load_benchmark_runs(user_id, status, model_name, since_time, until_time, limit)
    }

    pub fn load_run(&self, run_id: &str) -> Result<Option<Value>> {
        self.storage.load_benchmark_run(run_id)
    }

    pub fn load_attempts(&self, run_id: &str) -> Result<Vec<Value>> {
        self.storage.load_benchmark_attempts(run_id)
    }

    pub fn load_task_aggregates(&self, run_id: &str) -> Result<Vec<Value>> {
        self.storage.load_benchmark_task_aggregates(run_id)
    }

    pub async fn delete_run(&self, run_id: &str) -> Result<Option<i64>> {
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Err(anyhow!("run_id required"));
        }
        let deleted = self.storage.delete_benchmark_run(cleaned)?;
        Ok((deleted > 0).then_some(deleted))
    }
}

async fn run_benchmark(ctx: BenchmarkRunContext) {
    let total_attempts = ctx.tasks.len() as u32 * ctx.runs_per_task;
    let _ = ctx.sender.send(BenchmarkEvent {
        event: "benchmark_started".to_string(),
        data: json!({
            "run_id": ctx.run_id,
            "task_count": ctx.tasks.len(),
            "attempt_count": total_attempts,
        }),
    });

    let mut all_attempts = Vec::new();
    let mut task_aggregates = Vec::new();
    let mut completed_attempts = 0u32;
    let mut completed_tasks = 0u32;
    let mut run_status = "finished".to_string();

    for task in &ctx.tasks {
        let mut task_attempts = Vec::new();
        for attempt_no in 1..=ctx.runs_per_task {
            if ctx.cancel_flag.load(Ordering::SeqCst) {
                run_status = "cancelled".to_string();
                break;
            }
            let attempt_payload = run_attempt(&ctx, task, attempt_no).await;
            let _ = ctx
                .storage
                .upsert_benchmark_attempt(&ctx.run_id, &attempt_payload);
            let _ = ctx.sender.send(BenchmarkEvent {
                event: "task_attempt_finished".to_string(),
                data: attempt_payload.clone(),
            });
            task_attempts.push(attempt_payload.clone());
            all_attempts.push(attempt_payload);
            completed_attempts += 1;
            let _ = ctx.sender.send(BenchmarkEvent {
                event: "benchmark_progress".to_string(),
                data: json!({
                    "run_id": ctx.run_id,
                    "completed_attempts": completed_attempts,
                    "total_attempts": total_attempts,
                    "completed_tasks": completed_tasks,
                    "total_tasks": ctx.tasks.len(),
                    "current_task_id": task.id(),
                }),
            });
        }
        let aggregate = build_task_aggregate(task, &task_attempts);
        let _ = ctx
            .storage
            .upsert_benchmark_task_aggregate(&ctx.run_id, &aggregate);
        let _ = ctx.sender.send(BenchmarkEvent {
            event: "task_aggregated".to_string(),
            data: aggregate.clone(),
        });
        task_aggregates.push(aggregate);
        completed_tasks += 1;
        if ctx.cancel_flag.load(Ordering::SeqCst) {
            run_status = "cancelled".to_string();
            break;
        }
    }

    let finished_time = now_ts();
    let summary = build_run_summary(
        &ctx.run_id,
        &ctx.user_id,
        ctx.model_name.as_deref().unwrap_or(""),
        ctx.judge_model_name.as_deref().unwrap_or(""),
        &ctx.suite_ids,
        &ctx.tool_snapshot,
        &task_aggregates,
        &all_attempts,
        ctx.started_time,
        finished_time,
        &run_status,
        &ctx.config_overrides.clone().unwrap_or(Value::Null),
    );
    let run_payload = json!({
        "run_id": ctx.run_id,
        "user_id": ctx.user_id,
        "model_name": ctx.model_name.unwrap_or_default(),
        "judge_model_name": ctx.judge_model_name.unwrap_or_default(),
        "suite_ids": ctx.suite_ids,
        "tool_snapshot": ctx.tool_snapshot,
        "task_count": task_aggregates.len(),
        "attempt_count": all_attempts.len(),
        "status": run_status,
        "total_score": summary["total_score"].clone(),
        "summary": summary,
        "started_time": ctx.started_time,
        "finished_time": finished_time,
        "elapsed_s": (finished_time - ctx.started_time).max(0.0),
        "config_overrides": ctx.config_overrides.unwrap_or(Value::Null),
    });
    let _ = ctx.storage.update_benchmark_run(&ctx.run_id, &run_payload);
    let _ = ctx.sender.send(BenchmarkEvent {
        event: "benchmark_finished".to_string(),
        data: run_payload,
    });
    update_current_session(&ctx.state, &ctx.run_id, None).await;
    remove_run(&ctx.state, &ctx.run_id).await;
}

async fn run_attempt(
    ctx: &BenchmarkRunContext,
    task: &BenchmarkTaskSpec,
    attempt_no: u32,
) -> Value {
    let started_time = now_ts();
    let session_id = format!("bench-{}-{}-{attempt_no}", ctx.run_id, task.id());
    let (workspace_dir, attempt_root) = match prepare_attempt_workspace(
        &ctx.workspace,
        &ctx.user_id,
        &ctx.run_id,
        task,
        attempt_no,
    ) {
        Ok(value) => value,
        Err(err) => {
            return json!({
                "run_id": ctx.run_id,
                "task_id": task.id(),
                "attempt_no": attempt_no,
                "status": "error",
                "error": err.to_string(),
                "started_time": started_time,
                "finished_time": now_ts(),
                "elapsed_s": 0.0,
                "final_score": 0.0,
            });
        }
    };
    update_current_session(&ctx.state, &ctx.run_id, Some(session_id.clone())).await;
    let _ = ctx.sender.send(BenchmarkEvent {
        event: "task_attempt_started".to_string(),
        data: json!({
            "run_id": ctx.run_id,
            "task_id": task.id(),
            "name": task.frontmatter.name,
            "suite": task.frontmatter.suite,
            "category": task.frontmatter.category,
            "attempt_no": attempt_no,
            "status": "running",
            "attempt_root": attempt_root,
            "started_time": started_time,
        }),
    });

    // Keep task prompt templating explicit so every attempt is fully reproducible.
    let question = apply_task_placeholders(
        &task.prompt,
        &ctx.run_id,
        task.id(),
        attempt_no,
        &build_attempt_root(&ctx.run_id, task.id(), attempt_no),
    );
    let request = WunderRequest {
        user_id: ctx.user_id.clone(),
        question,
        tool_names: ctx.requested_tool_names.clone(),
        skip_tool_calls: false,
        stream: true,
        debug_payload: false,
        session_id: Some(session_id.clone()),
        agent_id: None,
        model_name: ctx.model_name.clone(),
        language: task
            .preferred_language()
            .or_else(|| Some(i18n::get_default_language())),
        config_overrides: ctx.config_overrides.clone(),
        agent_prompt: None,
        preview_skill: false,
        attachments: None,
        allow_queue: true,
        is_admin: false,
        approval_tx: None,
    };

    let execution = tokio::time::timeout(
        std::time::Duration::from_secs(task.timeout_seconds()),
        execute_prompt(
            ctx.orchestrator.clone(),
            ctx.monitor.clone(),
            request,
            ctx.cancel_flag.clone(),
            &session_id,
        ),
    )
    .await;

    let finished_time = now_ts();
    let elapsed_s = (finished_time - started_time).max(0.0);
    match execution {
        Ok(Ok(capture)) => {
            let automated = if matches!(
                task.grading_type(),
                BenchmarkGradingType::Automated | BenchmarkGradingType::Hybrid
            ) {
                grade_automated(task, &capture, &workspace_dir, &ctx.config)
                    .await
                    .unwrap_or_else(|err| {
                        json!({"score": 0.0, "breakdown": {}, "notes": "", "error": err.to_string()})
                    })
            } else {
                json!({"score": 0.0, "breakdown": {}, "notes": "", "error": ""})
            };
            let judge = if matches!(
                task.grading_type(),
                BenchmarkGradingType::LlmJudge | BenchmarkGradingType::Hybrid
            ) {
                let judge_session_id = format!("{}-judge", session_id);
                update_current_session(&ctx.state, &ctx.run_id, Some(judge_session_id.clone()))
                    .await;
                grade_with_judge(
                    task,
                    &capture,
                    ctx.orchestrator.clone(),
                    ctx.monitor.clone(),
                    ctx.cancel_flag.clone(),
                    &ctx.user_id,
                    &judge_session_id,
                    ctx.judge_model_name.clone(),
                    ctx.config_overrides.clone(),
                    Some(i18n::get_default_language()),
                )
                .await
                .unwrap_or_else(|err| {
                    json!({"score": 0.0, "breakdown": {}, "notes": "", "raw_response": "", "error": err.to_string()})
                })
            } else {
                json!({"score": 0.0, "breakdown": {}, "notes": "", "raw_response": "", "error": ""})
            };
            let final_score = combine_scores(task, &automated, &judge);
            let artifacts = if ctx.capture_artifacts {
                build_artifact_manifest(&workspace_dir).unwrap_or_default()
            } else {
                Vec::new()
            };
            let transcript_summary = build_transcript_summary(&capture);
            json!({
                "run_id": ctx.run_id,
                "task_id": task.id(),
                "name": task.frontmatter.name,
                "suite": task.frontmatter.suite,
                "category": task.frontmatter.category,
                "grading_type": task.frontmatter.grading_type,
                "attempt_no": attempt_no,
                "status": if ctx.cancel_flag.load(Ordering::SeqCst) { "cancelled" } else { "finished" },
                "attempt_root": attempt_root,
                "prompt": task.prompt,
                "expected_behavior": task.expected_behavior,
                "final_answer": capture.final_answer,
                "tool_calls": capture.tool_calls,
                "tool_results": capture.tool_results,
                "usage": capture.usage,
                "transcript": if ctx.capture_transcript { Value::Array(capture.transcript) } else { Value::Null },
                "transcript_summary": transcript_summary,
                "artifacts": artifacts,
                "automated": automated,
                "judge": judge,
                "final_score": final_score,
                "error": capture.error_message,
                "error_code": capture.error_code,
                "error_detail": capture.error_detail,
                "started_time": started_time,
                "finished_time": finished_time,
                "elapsed_s": elapsed_s,
            })
        }
        Ok(Err(err)) => json!({
            "run_id": ctx.run_id,
            "task_id": task.id(),
            "name": task.frontmatter.name,
            "suite": task.frontmatter.suite,
            "category": task.frontmatter.category,
            "grading_type": task.frontmatter.grading_type,
            "attempt_no": attempt_no,
            "status": "error",
            "attempt_root": attempt_root,
            "error": err,
            "started_time": started_time,
            "finished_time": finished_time,
            "elapsed_s": elapsed_s,
            "final_score": 0.0,
        }),
        Err(_) => {
            ctx.monitor.cancel(&session_id);
            json!({
                "run_id": ctx.run_id,
                "task_id": task.id(),
                "name": task.frontmatter.name,
                "suite": task.frontmatter.suite,
                "category": task.frontmatter.category,
                "grading_type": task.frontmatter.grading_type,
                "attempt_no": attempt_no,
                "status": "error",
                "attempt_root": attempt_root,
                "error": "timeout",
                "started_time": started_time,
                "finished_time": now_ts(),
                "elapsed_s": (now_ts() - started_time).max(0.0),
                "final_score": 0.0,
            })
        }
    }
}

fn combine_scores(task: &BenchmarkTaskSpec, automated: &Value, judge: &Value) -> f64 {
    let automated_score = automated
        .get("score")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let judge_score = judge.get("score").and_then(Value::as_f64).unwrap_or(0.0);
    match task.grading_type() {
        BenchmarkGradingType::Automated => automated_score,
        BenchmarkGradingType::LlmJudge => judge_score,
        BenchmarkGradingType::Hybrid => {
            let auto_weight = task
                .frontmatter
                .grading_weights
                .get("automated")
                .copied()
                .unwrap_or(0.5);
            let judge_weight = task
                .frontmatter
                .grading_weights
                .get("llm_judge")
                .copied()
                .unwrap_or(0.5);
            let total_weight = (auto_weight + judge_weight).max(f64::EPSILON);
            (automated_score * auto_weight + judge_score * judge_weight) / total_weight
        }
    }
}

fn build_transcript_summary(capture: &super::models::ExecutionCapture) -> String {
    let mut lines = Vec::new();
    for call in capture.tool_calls.iter().take(12) {
        lines.push(format!(
            "ToolCall {} {}",
            call.name,
            truncate_json(&call.args, 160)
        ));
    }
    for result in capture.tool_results.iter().take(12) {
        lines.push(format!("ToolResult {} {}", result.name, result.preview));
    }
    if !capture.final_answer.trim().is_empty() {
        lines.push(format!(
            "Final {}",
            capture.final_answer.chars().take(400).collect::<String>()
        ));
    }
    if !capture.error_message.trim().is_empty() {
        lines.push(format!("Error {}", capture.error_message));
    }
    lines.join("\n")
}

fn truncate_json(value: &Value, max_chars: usize) -> String {
    let text = value.to_string();
    if text.chars().count() <= max_chars {
        return text;
    }
    text.chars().take(max_chars).collect::<String>()
}

async fn update_current_session(
    state: &Arc<Mutex<BenchmarkState>>,
    run_id: &str,
    session_id: Option<String>,
) {
    let mut guard = state.lock().await;
    if let Some(handle) = guard.runs.get_mut(run_id) {
        handle.current_session_id = session_id;
    }
}

async fn remove_run(state: &Arc<Mutex<BenchmarkState>>, run_id: &str) {
    let mut guard = state.lock().await;
    guard.runs.remove(run_id);
}

fn resolve_allowed_tool_names(
    config: &Config,
    skills: &SkillRegistry,
    user_tool_bindings: Option<&UserToolBindings>,
    requested: &[String],
) -> HashSet<String> {
    let default_mode = requested.is_empty();
    let available = collect_available_tool_names(config, skills, user_tool_bindings);
    let mut allowed = if default_mode {
        available
    } else {
        let expanded = expand_requested_tool_names(requested);
        available
            .into_iter()
            .filter(|name| expanded.contains(name))
            .collect()
    };
    if default_mode {
        allowed.remove("a2ui");
    }
    if allowed.contains("a2ui") {
        allowed.remove("final_response");
        allowed.remove(&resolve_tool_name("final_response"));
    }
    allowed
}

fn expand_requested_tool_names(requested: &[String]) -> HashSet<String> {
    let alias_map = builtin_aliases();
    let mut aliases_by_name: HashMap<String, Vec<String>> = HashMap::new();
    for (alias, canonical) in &alias_map {
        aliases_by_name
            .entry(canonical.clone())
            .or_default()
            .push(alias.clone());
    }
    let mut output = HashSet::new();
    for raw in requested {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        if let Some(canonical) = alias_map.get(name) {
            output.insert(canonical.clone());
            output.insert(name.to_string());
            if let Some(aliases) = aliases_by_name.get(canonical) {
                output.extend(aliases.iter().cloned());
            }
        } else {
            output.insert(name.to_string());
            let canonical = resolve_tool_name(name);
            output.insert(canonical.clone());
            if let Some(aliases) = aliases_by_name.get(&canonical) {
                output.extend(aliases.iter().cloned());
            }
        }
    }
    output
}

fn resolve_model_name(requested: Option<&str>, config: &Config) -> Option<String> {
    let requested = requested.map(str::trim).filter(|value| !value.is_empty());
    if let Some(name) = requested {
        if config
            .llm
            .models
            .get(name)
            .filter(|model| is_llm_model(model))
            .is_some()
        {
            return Some(name.to_string());
        }
    }
    let default_name = config.llm.default.trim();
    if !default_name.is_empty()
        && config
            .llm
            .models
            .get(default_name)
            .filter(|model| is_llm_model(model))
            .is_some()
    {
        return Some(default_name.to_string());
    }
    config
        .llm
        .models
        .iter()
        .find(|(_, model)| is_llm_model(model))
        .map(|(name, _)| name.clone())
}

fn filter_tasks(
    tasks: Vec<BenchmarkTaskSpec>,
    suite_ids: &[String],
    task_ids: &[String],
) -> Vec<BenchmarkTaskSpec> {
    let suite_filter = suite_ids
        .iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<HashSet<_>>();
    let task_filter = task_ids
        .iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<HashSet<_>>();
    tasks
        .into_iter()
        .filter(|task| {
            (suite_filter.is_empty()
                || suite_filter.contains(&task.frontmatter.suite.to_lowercase()))
                && (task_filter.is_empty()
                    || task_filter.contains(&task.frontmatter.id.to_lowercase()))
        })
        .collect()
}

fn read_string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}
