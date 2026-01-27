use crate::config::Config;
use crate::config_store::ConfigStore;
use crate::evaluation::{
    default_cases_dir, load_case_files, normalize_dimension_weights, DimensionWeights,
    EvaluationCase, EvaluationCaseFile, EvaluationChecker, EvaluationDimension,
};
use crate::i18n;
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
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio_stream::StreamExt;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct EvaluationEvent {
    pub event: String,
    pub data: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvaluationStartRequest {
    pub user_id: String,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub case_set: Option<String>,
    #[serde(default)]
    pub dimensions: Vec<EvaluationDimension>,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub config_overrides: Option<Value>,
    #[serde(default)]
    pub weights: Option<DimensionWeights>,
}

#[derive(Clone)]
pub struct EvaluationManager {
    config_store: ConfigStore,
    storage: Arc<dyn StorageBackend>,
    workspace: Arc<WorkspaceManager>,
    monitor: Arc<MonitorState>,
    orchestrator: Arc<Orchestrator>,
    skills: Arc<RwLock<SkillRegistry>>,
    user_tool_manager: Arc<UserToolManager>,
    state: Arc<Mutex<EvaluationState>>,
}

struct EvaluationState {
    runs: HashMap<String, EvaluationRunHandle>,
}

struct EvaluationRunHandle {
    cancel_flag: Arc<AtomicBool>,
    sender: broadcast::Sender<EvaluationEvent>,
    current_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ToolCallRecord {
    name: String,
    args: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaseStatus {
    Passed,
    Failed,
    Skipped,
    Error,
    Cancelled,
}

impl CaseStatus {
    fn as_str(&self) -> &'static str {
        match self {
            CaseStatus::Passed => "passed",
            CaseStatus::Failed => "failed",
            CaseStatus::Skipped => "skipped",
            CaseStatus::Error => "error",
            CaseStatus::Cancelled => "cancelled",
        }
    }
}

struct CaseExecution {
    status: CaseStatus,
    score: f64,
    detail: Value,
    final_answer: String,
    tool_calls: Vec<ToolCallRecord>,
    error: String,
    skip_reason: Option<String>,
    abort_run: bool,
}

#[derive(Default)]
struct DimensionAccumulator {
    weight_sum: f64,
    score_sum: f64,
    case_count: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    errored: usize,
}
impl EvaluationManager {
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
            state: Arc::new(Mutex::new(EvaluationState {
                runs: HashMap::new(),
            })),
        }
    }

    pub async fn start(&self, request: EvaluationStartRequest) -> Result<Value> {
        let user_id = request.user_id.trim();
        if user_id.is_empty() {
            return Err(anyhow!("user_id required"));
        }
        let case_set = request
            .case_set
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let case_set = if case_set.trim().is_empty() {
            "default".to_string()
        } else {
            case_set.trim().to_string()
        };
        let language = i18n::normalize_language(request.language.as_deref(), true);
        let requested_tool_names = request.tool_names.clone();
        let selected_dimensions = request.dimensions.clone();
        let config_overrides = request.config_overrides.clone();
        let config = self.config_store.get().await;
        let model_name = resolve_eval_model_name(request.model_name.as_deref(), &config);
        let skills_snapshot = self.skills.read().await.clone();
        let user_tool_bindings =
            self.user_tool_manager
                .build_bindings(&config, &skills_snapshot, user_id);
        let allowed_tool_names = resolve_allowed_tool_names(
            &config,
            &skills_snapshot,
            Some(&user_tool_bindings),
            &requested_tool_names,
        );

        let case_files = load_case_files(&default_cases_dir())?;
        let selected_cases = select_cases(&case_files, &case_set, &language, &selected_dimensions)?;
        if selected_cases.is_empty() {
            return Err(anyhow!("no evaluation cases available"));
        }

        let run_id = Uuid::new_v4().simple().to_string();
        let weights = normalize_dimension_weights(request.weights.clone().unwrap_or_default());
        let mut tool_snapshot = allowed_tool_names.iter().cloned().collect::<Vec<String>>();
        tool_snapshot.sort();
        let case_ids = selected_cases
            .iter()
            .map(|case| case.id.clone())
            .collect::<Vec<_>>();
        let case_score_map = build_case_score_map(&selected_cases, &weights);

        let started_time = now_ts();
        let run_payload = json!({
            "run_id": run_id,
            "user_id": user_id,
            "model_name": model_name.clone().unwrap_or_default(),
            "language": language.clone(),
            "case_set": case_set.clone(),
            "dimensions": selected_dimensions,
            "tool_names": requested_tool_names.clone(),
            "tool_snapshot": tool_snapshot.clone(),
            "weights": weights.clone(),
            "case_ids": case_ids.clone(),
            "status": "running",
            "total_score": 0.0,
            "dimension_scores": {},
            "case_count": selected_cases.len(),
            "passed_count": 0,
            "failed_count": 0,
            "skipped_count": 0,
            "error_count": 0,
            "started_time": started_time,
            "finished_time": 0.0,
            "elapsed_s": 0.0,
            "error": "",
            "config_overrides": config_overrides.clone().unwrap_or(Value::Null),
        });
        self.storage.create_evaluation_run(&run_payload)?;

        for case in &selected_cases {
            let max_score = case_score_map.get(&case.id).copied().unwrap_or(0.0);
            let item_payload =
                build_active_item_payload(case, &run_id, user_id, started_time, max_score);
            self.storage
                .upsert_evaluation_item(&run_id, &item_payload)?;
        }

        let (sender, _) = broadcast::channel(256);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let handle = EvaluationRunHandle {
            cancel_flag: cancel_flag.clone(),
            sender: sender.clone(),
            current_session_id: None,
        };
        {
            let mut guard = self.state.lock().await;
            guard.runs.insert(run_id.clone(), handle);
        }

        let ctx = EvaluationRunContext {
            run_id: run_id.clone(),
            started_time,
            user_id: user_id.to_string(),
            model_name,
            language,
            case_set,
            requested_tool_names: requested_tool_names.clone(),
            config_overrides,
            weights,
            cases: selected_cases,
            allowed_tool_names,
            sender,
            cancel_flag,
            storage: self.storage.clone(),
            workspace: self.workspace.clone(),
            monitor: self.monitor.clone(),
            orchestrator: self.orchestrator.clone(),
            state: self.state.clone(),
            tool_snapshot: tool_snapshot.clone(),
            case_ids,
            case_score_map,
        };

        tokio::spawn(async move {
            run_evaluation(ctx).await;
        });

        Ok(json!({
            "run_id": run_id,
            "status": "running",
            "case_count": run_payload.get("case_count").cloned().unwrap_or(json!(0)),
        }))
    }

    pub async fn cancel(&self, run_id: &str) -> Result<Value> {
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Err(anyhow!("run_id required"));
        }
        let mut guard = self.state.lock().await;
        let Some(handle) = guard.runs.get_mut(cleaned) else {
            return Ok(json!({ "ok": false, "message": "run not found" }));
        };
        handle.cancel_flag.store(true, Ordering::SeqCst);
        if let Some(session_id) = handle.current_session_id.clone() {
            self.monitor.cancel(&session_id);
        }
        let _ = handle.sender.send(EvaluationEvent {
            event: "eval_log".to_string(),
            data: json!({ "run_id": cleaned, "message": "cancel requested" }),
        });
        Ok(json!({ "ok": true }))
    }

    pub async fn subscribe(&self, run_id: &str) -> Option<broadcast::Receiver<EvaluationEvent>> {
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
            .load_evaluation_runs(user_id, status, model_name, since_time, until_time, limit)
    }

    pub fn load_run(&self, run_id: &str) -> Result<Option<Value>> {
        self.storage.load_evaluation_run(run_id)
    }

    pub fn load_items(&self, run_id: &str) -> Result<Vec<Value>> {
        self.storage.load_evaluation_items(run_id)
    }

    pub async fn delete_run(&self, run_id: &str) -> Result<Option<i64>> {
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Err(anyhow!("run_id required"));
        }
        {
            let guard = self.state.lock().await;
            if guard.runs.contains_key(cleaned) {
                return Err(anyhow!("run is still running"));
            }
        }
        let run = self.storage.load_evaluation_run(cleaned)?;
        if run.is_none() {
            return Ok(None);
        }
        let deleted = self.storage.delete_evaluation_run(cleaned)?;
        Ok(Some(deleted))
    }
}
struct EvaluationRunContext {
    run_id: String,
    started_time: f64,
    user_id: String,
    model_name: Option<String>,
    language: String,
    case_set: String,
    requested_tool_names: Vec<String>,
    config_overrides: Option<Value>,
    weights: DimensionWeights,
    cases: Vec<EvaluationCase>,
    allowed_tool_names: HashSet<String>,
    tool_snapshot: Vec<String>,
    case_ids: Vec<String>,
    case_score_map: HashMap<String, f64>,
    sender: broadcast::Sender<EvaluationEvent>,
    cancel_flag: Arc<AtomicBool>,
    storage: Arc<dyn StorageBackend>,
    workspace: Arc<WorkspaceManager>,
    monitor: Arc<MonitorState>,
    orchestrator: Arc<Orchestrator>,
    state: Arc<Mutex<EvaluationState>>,
}

async fn run_evaluation(ctx: EvaluationRunContext) {
    let run_id = ctx.run_id.clone();
    let start_ts = ctx.started_time;
    let _ = reset_eval_workspace(&ctx.workspace, &ctx.user_id);
    let _ = prepare_eval_workspace(&ctx.workspace, &ctx.user_id, &run_id);
    let total_expected = ctx.cases.len();
    let mut dimension_set = HashSet::new();
    for case in ctx.cases.iter() {
        dimension_set.insert(dimension_label(case.dimension).to_string());
    }
    let mut dimensions = dimension_set.into_iter().collect::<Vec<_>>();
    dimensions.sort();
    let weights = ctx.weights.clone();
    let _ = ctx.sender.send(EvaluationEvent {
        event: "eval_started".to_string(),
        data: json!({
            "run_id": run_id,
            "case_count": total_expected,
        }),
    });

    for case in ctx.cases.iter() {
        let max_score = ctx.case_score_map.get(&case.id).copied().unwrap_or(0.0);
        let item_payload =
            build_active_item_payload(case, &run_id, &ctx.user_id, start_ts, max_score);
        let _ = ctx.sender.send(EvaluationEvent {
            event: "eval_item".to_string(),
            data: item_payload,
        });
    }
    let _ = ctx.sender.send(EvaluationEvent {
        event: "eval_progress".to_string(),
        data: json!({
            "run_id": run_id,
            "completed": 0,
            "total": total_expected,
            "passed": 0,
            "failed": 0,
            "skipped": 0,
            "errors": 0,
        }),
    });

    let mut run_payload = json!({
        "run_id": ctx.run_id.clone(),
        "user_id": ctx.user_id.clone(),
        "model_name": ctx.model_name.clone().unwrap_or_default(),
        "language": ctx.language.clone(),
        "case_set": ctx.case_set.clone(),
        "dimensions": dimensions,
        "tool_names": ctx.requested_tool_names.clone(),
        "tool_snapshot": ctx.tool_snapshot.clone(),
        "case_ids": ctx.case_ids.clone(),
        "weights": weights.clone(),
        "status": "running",
        "total_score": 0.0,
        "dimension_scores": {},
        "case_count": total_expected,
        "passed_count": 0,
        "failed_count": 0,
        "skipped_count": 0,
        "error_count": 0,
        "started_time": start_ts,
        "finished_time": 0.0,
        "elapsed_s": 0.0,
        "error": "",
        "config_overrides": ctx.config_overrides.clone().unwrap_or(Value::Null),
    });

    let mut total_cases = 0usize;
    let mut passed_cases = 0usize;
    let mut failed_cases = 0usize;
    let mut skipped_cases = 0usize;
    let mut error_cases = 0usize;
    let mut run_status = "finished".to_string();
    let mut run_error = String::new();

    let mut dimension_stats: HashMap<EvaluationDimension, DimensionAccumulator> = HashMap::new();

    for case in &ctx.cases {
        if ctx.cancel_flag.load(Ordering::SeqCst) {
            run_status = "cancelled".to_string();
            break;
        }
        total_cases += 1;
        let case_start = now_ts();
        let session_id = build_eval_session_id(&run_id, case.id.as_str());
        update_current_session(&ctx.state, &run_id, Some(session_id.clone())).await;

        let result = run_case(&ctx, case, &session_id).await;
        let case_end = now_ts();

        let status = result.status;
        if status == CaseStatus::Cancelled {
            run_status = "cancelled".to_string();
        }
        if status == CaseStatus::Error {
            run_status = "failed".to_string();
            if run_error.is_empty() {
                run_error = result.error.clone();
            }
        }

        let dim_entry = dimension_stats
            .entry(case.dimension)
            .or_insert_with(DimensionAccumulator::default);
        dim_entry.case_count += 1;
        match status {
            CaseStatus::Passed => {
                passed_cases += 1;
                dim_entry.passed += 1;
                dim_entry.weight_sum += case.weight.max(0.0);
                dim_entry.score_sum += case.weight.max(0.0) * result.score;
            }
            CaseStatus::Failed => {
                failed_cases += 1;
                dim_entry.failed += 1;
                dim_entry.weight_sum += case.weight.max(0.0);
                dim_entry.score_sum += case.weight.max(0.0) * result.score;
            }
            CaseStatus::Skipped => {
                skipped_cases += 1;
                dim_entry.skipped += 1;
            }
            CaseStatus::Error => {
                error_cases += 1;
                dim_entry.errored += 1;
                dim_entry.weight_sum += case.weight.max(0.0);
                dim_entry.score_sum += case.weight.max(0.0) * result.score;
            }
            CaseStatus::Cancelled => {
                skipped_cases += 1;
                dim_entry.skipped += 1;
            }
        }

        let max_score = ctx.case_score_map.get(&case.id).copied().unwrap_or(0.0);
        let score = (max_score * result.score).max(0.0);
        let item_payload = json!({
            "run_id": run_id,
            "case_id": case.id.clone(),
            "dimension": dimension_label(case.dimension),
            "status": status.as_str(),
            "score": score,
            "max_score": max_score,
            "weight": case.weight.max(0.0),
            "prompt": apply_placeholders(&case.prompt, &run_id, &ctx.user_id),
            "checker": serde_json::to_value(&case.checker).unwrap_or(Value::Null),
            "final_answer": result.final_answer,
            "tool_calls": result.tool_calls,
            "checker_detail": result.detail,
            "started_time": case_start,
            "finished_time": case_end,
            "elapsed_s": (case_end - case_start).max(0.0),
            "error": result.error,
            "skip_reason": result.skip_reason,
            "session_id": session_id,
        });
        let _ = ctx.storage.upsert_evaluation_item(&run_id, &item_payload);
        let _ = ctx.sender.send(EvaluationEvent {
            event: "eval_item".to_string(),
            data: item_payload.clone(),
        });

        let _ = ctx.sender.send(EvaluationEvent {
            event: "eval_progress".to_string(),
            data: json!({
                "run_id": run_id,
                "completed": total_cases,
                "total": total_expected,
                "passed": passed_cases,
                "failed": failed_cases,
                "skipped": skipped_cases,
                "errors": error_cases,
            }),
        });

        if result.abort_run || status == CaseStatus::Cancelled {
            break;
        }
    }

    let dimension_scores = build_dimension_scores(&dimension_stats);
    let total_score = build_total_score(&dimension_scores, &weights);

    let finished_time = now_ts();
    if let Value::Object(ref mut map) = run_payload {
        map.insert("status".to_string(), json!(run_status));
        map.insert("total_score".to_string(), json!(total_score));
        map.insert("dimension_scores".to_string(), json!(dimension_scores));
        map.insert("passed_count".to_string(), json!(passed_cases));
        map.insert("failed_count".to_string(), json!(failed_cases));
        map.insert("skipped_count".to_string(), json!(skipped_cases));
        map.insert("error_count".to_string(), json!(error_cases));
        map.insert("finished_time".to_string(), json!(finished_time));
        map.insert(
            "elapsed_s".to_string(),
            json!((finished_time - start_ts).max(0.0)),
        );
        if !run_error.is_empty() {
            map.insert("error".to_string(), json!(run_error));
        }
    }
    let _ = ctx.storage.update_evaluation_run(&run_id, &run_payload);

    let _ = ctx.sender.send(EvaluationEvent {
        event: "eval_finished".to_string(),
        data: run_payload.clone(),
    });

    update_current_session(&ctx.state, &run_id, None).await;
    remove_run(&ctx.state, &run_id).await;
}

async fn update_current_session(
    state: &Arc<Mutex<EvaluationState>>,
    run_id: &str,
    session_id: Option<String>,
) {
    let mut guard = state.lock().await;
    if let Some(handle) = guard.runs.get_mut(run_id) {
        handle.current_session_id = session_id;
    }
}

async fn remove_run(state: &Arc<Mutex<EvaluationState>>, run_id: &str) {
    let mut guard = state.lock().await;
    guard.runs.remove(run_id);
}

fn build_dimension_scores(
    stats: &HashMap<EvaluationDimension, DimensionAccumulator>,
) -> HashMap<String, f64> {
    let mut output = HashMap::new();
    for (dimension, entry) in stats {
        if entry.weight_sum <= 0.0 {
            continue;
        }
        let score = (entry.score_sum / entry.weight_sum).max(0.0) * 100.0;
        output.insert(dimension_label(*dimension).to_string(), score);
    }
    output
}

fn build_total_score(scores: &HashMap<String, f64>, weights: &DimensionWeights) -> f64 {
    let weight_map = HashMap::from([
        ("tool".to_string(), weights.tool),
        ("logic".to_string(), weights.logic),
        ("common".to_string(), weights.common),
        ("complex".to_string(), weights.complex),
    ]);
    let total_weight = weights.tool + weights.logic + weights.common + weights.complex;
    let mut weighted_sum = 0.0;
    for (dimension, score) in scores {
        if let Some(weight) = weight_map.get(dimension) {
            if *weight > 0.0 {
                weighted_sum += score * weight;
            }
        }
    }
    if total_weight <= 0.0 {
        0.0
    } else {
        (weighted_sum / total_weight).max(0.0)
    }
}

fn build_case_score_map(
    cases: &[EvaluationCase],
    weights: &DimensionWeights,
) -> HashMap<String, f64> {
    let mut totals: HashMap<EvaluationDimension, f64> = HashMap::new();
    for case in cases {
        let entry = totals.entry(case.dimension).or_insert(0.0);
        *entry += case.weight.max(0.0);
    }
    let mut output = HashMap::new();
    for case in cases {
        let dimension_weight = dimension_weight(weights, case.dimension);
        let total_weight = totals.get(&case.dimension).copied().unwrap_or(0.0);
        let case_weight = case.weight.max(0.0);
        let max_score = if dimension_weight > 0.0 && total_weight > 0.0 {
            case_weight / total_weight * dimension_weight
        } else {
            0.0
        };
        output.insert(case.id.clone(), max_score);
    }
    output
}
fn select_cases(
    files: &[EvaluationCaseFile],
    case_set: &str,
    language: &str,
    dimensions: &[EvaluationDimension],
) -> Result<Vec<EvaluationCase>> {
    let normalized_case_set = case_set.trim().to_lowercase();
    let normalized_language = i18n::normalize_language(Some(language), true);
    let default_language = i18n::normalize_language(Some(&i18n::get_default_language()), true);
    let dimension_filter: HashSet<EvaluationDimension> =
        dimensions.iter().copied().collect::<HashSet<_>>();

    let collect = |target_language: &str| {
        let mut selected = Vec::new();
        let mut seen = HashSet::new();
        for file in files {
            let file_case_set = file.case_set.trim().to_lowercase();
            if !file_case_set.is_empty() && file_case_set != normalized_case_set {
                continue;
            }
            if !file.language.trim().is_empty() {
                let file_language = i18n::normalize_language(Some(&file.language), true);
                if file_language != target_language {
                    continue;
                }
            }
            for case in &file.cases {
                if !dimension_filter.is_empty() && !dimension_filter.contains(&case.dimension) {
                    continue;
                }
                if let Some(case_language) = case.language.as_deref() {
                    if !case_language.trim().is_empty() {
                        let normalized = i18n::normalize_language(Some(case_language), true);
                        if normalized != target_language {
                            continue;
                        }
                    }
                }
                if seen.insert(case.id.clone()) {
                    selected.push(case.clone());
                }
            }
        }
        selected
    };

    let mut cases = collect(&normalized_language);
    if cases.is_empty() && normalized_language != default_language {
        cases = collect(&default_language);
    }
    Ok(cases)
}

fn reset_eval_workspace(workspace: &WorkspaceManager, user_id: &str) -> Result<()> {
    let eval_root = workspace.resolve_path(user_id, "eval")?;
    if eval_root.exists() {
        std::fs::remove_dir_all(&eval_root)?;
        workspace.bump_version(user_id);
    }
    Ok(())
}

fn build_active_item_payload(
    case: &EvaluationCase,
    run_id: &str,
    user_id: &str,
    started_time: f64,
    max_score: f64,
) -> Value {
    json!({
        "run_id": run_id,
        "case_id": case.id.clone(),
        "dimension": dimension_label(case.dimension),
        "status": "active",
        "score": 0.0,
        "max_score": max_score,
        "weight": case.weight.max(0.0),
        "prompt": apply_placeholders(&case.prompt, run_id, user_id),
        "checker": serde_json::to_value(&case.checker).unwrap_or(Value::Null),
        "final_answer": "",
        "tool_calls": [],
        "checker_detail": Value::Null,
        "started_time": started_time,
        "finished_time": 0.0,
        "elapsed_s": 0.0,
        "error": "",
        "skip_reason": Value::Null,
        "session_id": "",
    })
}

fn prepare_eval_workspace(
    workspace: &WorkspaceManager,
    user_id: &str,
    run_id: &str,
) -> Result<PathBuf> {
    let base = workspace.resolve_path(user_id, &format!("eval/{run_id}"))?;
    let fixtures = base.join("fixtures");
    let outputs = base.join("outputs");
    std::fs::create_dir_all(&fixtures)?;
    std::fs::create_dir_all(&outputs)?;

    let list_dir = fixtures.join("list");
    std::fs::create_dir_all(&list_dir)?;
    std::fs::write(fixtures.join("alpha.txt"), "alpha beta gamma\n")?;
    std::fs::write(list_dir.join("a.txt"), "alpha\n")?;
    std::fs::write(list_dir.join("b.txt"), "beta\n")?;
    std::fs::write(outputs.join("replace.txt"), "hello world\n")?;
    std::fs::write(outputs.join("edit.txt"), "line1\nline2\n")?;

    workspace.bump_version(user_id);

    Ok(base)
}
struct CaseStreamOutput {
    last_output: String,
    final_answer: Option<String>,
    tool_calls: Vec<ToolCallRecord>,
    error_code: Option<String>,
    error_message: Option<String>,
    error_detail: Option<Value>,
}

async fn run_case(
    ctx: &EvaluationRunContext,
    case: &EvaluationCase,
    session_id: &str,
) -> CaseExecution {
    let missing_tools = missing_required_tools(case, &ctx.allowed_tool_names);
    if !missing_tools.is_empty() {
        return CaseExecution {
            status: CaseStatus::Skipped,
            score: 0.0,
            detail: json!({ "missing_tools": missing_tools }),
            final_answer: String::new(),
            tool_calls: Vec::new(),
            error: String::new(),
            skip_reason: Some("missing_tools".to_string()),
            abort_run: false,
        };
    }

    let prompt = apply_placeholders(&case.prompt, &ctx.run_id, &ctx.user_id);
    let request = WunderRequest {
        user_id: ctx.user_id.clone(),
        question: prompt,
        tool_names: ctx.requested_tool_names.clone(),
        skip_tool_calls: false,
        stream: true,
        debug_payload: false,
        session_id: Some(session_id.to_string()),
        model_name: ctx.model_name.clone(),
        language: Some(ctx.language.clone()),
        config_overrides: ctx.config_overrides.clone(),
        agent_prompt: None,
        attachments: None,
    };

    let stream_future = collect_case_stream(ctx, request, session_id);
    let stream_result = if let Some(timeout_s) = case.timeout_s.filter(|value| *value > 0) {
        tokio::time::timeout(Duration::from_secs(timeout_s), stream_future)
            .await
            .map_err(|_| "timeout")
    } else {
        Ok(stream_future.await)
    };

    let output = match stream_result {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => {
            return CaseExecution {
                status: CaseStatus::Error,
                score: 0.0,
                detail: json!({ "error": err }),
                final_answer: String::new(),
                tool_calls: Vec::new(),
                error: err,
                skip_reason: None,
                abort_run: true,
            };
        }
        Err(_) => {
            ctx.monitor.cancel(session_id);
            return CaseExecution {
                status: CaseStatus::Error,
                score: 0.0,
                detail: json!({ "error": "timeout" }),
                final_answer: String::new(),
                tool_calls: Vec::new(),
                error: "timeout".to_string(),
                skip_reason: None,
                abort_run: true,
            };
        }
    };

    if output.error_code.is_some() || output.error_message.is_some() {
        let code = output.error_code.as_deref().unwrap_or("UNKNOWN_ERROR");
        let message = output.error_message.clone().unwrap_or_default();
        let status = if code == "CANCELLED" {
            CaseStatus::Cancelled
        } else {
            CaseStatus::Error
        };
        return CaseExecution {
            status,
            score: 0.0,
            detail: json!({
                "error_code": code,
                "error_message": message,
                "error_detail": output.error_detail,
            }),
            final_answer: String::new(),
            tool_calls: output.tool_calls,
            error: message,
            skip_reason: None,
            abort_run: true,
        };
    }

    let final_answer = output
        .final_answer
        .clone()
        .unwrap_or_else(|| strip_tool_calls(&output.last_output).trim().to_string());

    let (passed, detail) = evaluate_checker(
        case,
        &final_answer,
        &output.tool_calls,
        &ctx.workspace,
        &ctx.user_id,
        &ctx.run_id,
    );

    CaseExecution {
        status: if passed {
            CaseStatus::Passed
        } else {
            CaseStatus::Failed
        },
        score: if passed { 1.0 } else { 0.0 },
        detail,
        final_answer,
        tool_calls: output.tool_calls,
        error: String::new(),
        skip_reason: None,
        abort_run: false,
    }
}

async fn collect_case_stream(
    ctx: &EvaluationRunContext,
    request: WunderRequest,
    session_id: &str,
) -> Result<CaseStreamOutput, String> {
    let mut stream = ctx
        .orchestrator
        .stream(request)
        .await
        .map_err(|err| err.to_string())?;

    let mut last_output = String::new();
    let mut final_answer = None;
    let mut tool_calls: Vec<ToolCallRecord> = Vec::new();
    let mut error_code = None;
    let mut error_message = None;
    let mut error_detail = None;

    while let Some(event) = stream.next().await {
        let event = event.expect("stream event");
        if ctx.cancel_flag.load(Ordering::SeqCst) {
            ctx.monitor.cancel(session_id);
        }
        let data = event.data.get("data").cloned().unwrap_or(Value::Null);
        match event.event.as_str() {
            "llm_output" => {
                if let Some(content) = data.get("content").and_then(Value::as_str) {
                    last_output = content.to_string();
                }
            }
            "tool_call" => {
                let tool = data.get("tool").and_then(Value::as_str).unwrap_or("");
                let tool = normalize_tool_name(tool);
                if !tool.is_empty() {
                    let args = data.get("args").cloned().unwrap_or(Value::Null);
                    tool_calls.push(ToolCallRecord { name: tool, args });
                }
            }
            "final" => {
                if let Some(answer) = data.get("answer").and_then(Value::as_str) {
                    final_answer = Some(answer.to_string());
                }
            }
            "error" => {
                if let Some(code) = data.get("code").and_then(Value::as_str) {
                    error_code = Some(code.to_string());
                }
                if let Some(message) = data.get("message").and_then(Value::as_str) {
                    error_message = Some(message.to_string());
                }
                if data.get("detail").is_some() {
                    error_detail = data.get("detail").cloned();
                }
            }
            _ => {}
        }
    }

    Ok(CaseStreamOutput {
        last_output,
        final_answer,
        tool_calls,
        error_code,
        error_message,
        error_detail,
    })
}
fn missing_required_tools(case: &EvaluationCase, allowed: &HashSet<String>) -> Vec<String> {
    let mut required = case.prerequisites.clone();
    match &case.checker {
        EvaluationChecker::ToolCalled { tool } => required.push(tool.clone()),
        EvaluationChecker::ToolArgs { tool, .. } => required.push(tool.clone()),
        _ => {}
    }
    let mut missing = Vec::new();
    let mut seen = HashSet::new();
    for tool in required {
        let cleaned = tool.trim();
        if cleaned.is_empty() {
            continue;
        }
        if !tool_available(cleaned, allowed) && seen.insert(cleaned.to_string()) {
            missing.push(cleaned.to_string());
        }
    }
    missing.sort();
    missing
}

fn evaluate_checker(
    case: &EvaluationCase,
    answer: &str,
    tool_calls: &[ToolCallRecord],
    workspace: &WorkspaceManager,
    user_id: &str,
    run_id: &str,
) -> (bool, Value) {
    match &case.checker {
        EvaluationChecker::Choice { answer: expected } => {
            let normalized = answer
                .trim()
                .chars()
                .next()
                .map(|ch| ch.to_ascii_uppercase().to_string())
                .unwrap_or_default();
            let expected = expected.trim().to_ascii_uppercase();
            let passed = !normalized.is_empty() && normalized == expected;
            (
                passed,
                json!({
                    "expected": expected,
                    "actual": answer.trim(),
                    "normalized": normalized,
                }),
            )
        }
        EvaluationChecker::Exact { answer: expected } => {
            let passed = answer.trim() == expected.trim();
            (
                passed,
                json!({
                    "expected": expected,
                    "actual": answer.trim(),
                }),
            )
        }
        EvaluationChecker::Contains { text } => {
            let passed = answer.contains(text);
            (
                passed,
                json!({
                    "expected": text,
                    "actual": answer,
                }),
            )
        }
        EvaluationChecker::Regex { pattern } => {
            let regex = Regex::new(pattern).ok();
            let passed = regex
                .as_ref()
                .map(|re| re.is_match(answer))
                .unwrap_or(false);
            (
                passed,
                json!({
                    "pattern": pattern,
                    "actual": answer,
                    "valid": regex.is_some(),
                }),
            )
        }
        EvaluationChecker::ToolCalled { tool } => {
            let target = normalize_tool_name(tool);
            let mut matched = false;
            let mut called = Vec::new();
            for call in tool_calls {
                called.push(call.name.clone());
                if normalize_tool_name(&call.name) == target {
                    matched = true;
                }
            }
            (
                matched,
                json!({
                    "tool": target,
                    "called": called,
                }),
            )
        }
        EvaluationChecker::ToolArgs { tool, required } => {
            let target = normalize_tool_name(tool);
            let required = apply_placeholders_to_value(required, run_id, user_id);
            let mut matched = false;
            let mut matched_args = Value::Null;
            for call in tool_calls {
                if normalize_tool_name(&call.name) != target {
                    continue;
                }
                if json_contains(&call.args, &required) {
                    matched = true;
                    matched_args = call.args.clone();
                    break;
                }
            }
            (
                matched,
                json!({
                    "tool": target,
                    "required": required,
                    "matched_args": matched_args,
                }),
            )
        }
        EvaluationChecker::FileExists { path } => {
            let resolved = apply_placeholders(path, run_id, user_id);
            match workspace.resolve_path(user_id, &resolved) {
                Ok(target) => {
                    let exists = target.exists();
                    (exists, json!({ "path": resolved, "exists": exists }))
                }
                Err(err) => (false, json!({ "path": resolved, "error": err.to_string() })),
            }
        }
        EvaluationChecker::FileContains { path, text } => {
            let resolved = apply_placeholders(path, run_id, user_id);
            let expected = apply_placeholders(text, run_id, user_id);
            match workspace.resolve_path(user_id, &resolved) {
                Ok(target) => match std::fs::read_to_string(&target) {
                    Ok(content) => {
                        let passed = content.contains(&expected);
                        (
                            passed,
                            json!({ "path": resolved, "expected": expected, "matched": passed }),
                        )
                    }
                    Err(err) => (false, json!({ "path": resolved, "error": err.to_string() })),
                },
                Err(err) => (false, json!({ "path": resolved, "error": err.to_string() })),
            }
        }
        EvaluationChecker::JsonContains { path, required } => {
            let resolved = apply_placeholders(path, run_id, user_id);
            let required = apply_placeholders_to_value(required, run_id, user_id);
            match workspace.resolve_path(user_id, &resolved) {
                Ok(target) => match std::fs::read_to_string(&target) {
                    Ok(content) => match serde_json::from_str::<Value>(&content) {
                        Ok(actual) => {
                            let passed = json_contains(&actual, &required);
                            (
                                passed,
                                json!({
                                    "path": resolved,
                                    "required": required,
                                    "matched": passed,
                                }),
                            )
                        }
                        Err(err) => (false, json!({ "path": resolved, "error": err.to_string() })),
                    },
                    Err(err) => (false, json!({ "path": resolved, "error": err.to_string() })),
                },
                Err(err) => (false, json!({ "path": resolved, "error": err.to_string() })),
            }
        }
    }
}

fn tool_available(name: &str, allowed: &HashSet<String>) -> bool {
    if allowed.contains(name) {
        return true;
    }
    let normalized = normalize_tool_name(name);
    allowed.contains(&normalized)
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

fn resolve_eval_model_name(requested: Option<&str>, config: &Config) -> Option<String> {
    let requested = requested
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    if let Some(name) = requested {
        if config.llm.models.contains_key(name) {
            return Some(name.to_string());
        }
    }
    let default_name = config.llm.default.trim();
    if !default_name.is_empty() && config.llm.models.contains_key(default_name) {
        return Some(default_name.to_string());
    }
    config
        .llm
        .models
        .iter()
        .next()
        .map(|(name, _)| name.clone())
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
                for alias in aliases {
                    output.insert(alias.clone());
                }
            }
            continue;
        }
        if let Some(aliases) = aliases_by_name.get(name) {
            output.insert(name.to_string());
            for alias in aliases {
                output.insert(alias.clone());
            }
            continue;
        }
        output.insert(name.to_string());
    }
    output
}

fn normalize_tool_name(name: &str) -> String {
    let cleaned = name.trim();
    if cleaned.contains('@') {
        return cleaned.to_string();
    }
    resolve_tool_name(cleaned)
}

fn apply_placeholders(text: &str, run_id: &str, user_id: &str) -> String {
    text.replace("{run_id}", run_id)
        .replace("{user_id}", user_id)
}

fn apply_placeholders_to_value(value: &Value, run_id: &str, user_id: &str) -> Value {
    match value {
        Value::String(text) => Value::String(apply_placeholders(text, run_id, user_id)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| apply_placeholders_to_value(item, run_id, user_id))
                .collect(),
        ),
        Value::Object(map) => {
            let mut next = serde_json::Map::new();
            for (key, value) in map {
                next.insert(
                    key.clone(),
                    apply_placeholders_to_value(value, run_id, user_id),
                );
            }
            Value::Object(next)
        }
        other => other.clone(),
    }
}

fn shorten_id(value: &str, max_len: usize) -> String {
    let trimmed = value.trim();
    if max_len == 0 || trimmed.is_empty() {
        return String::new();
    }
    trimmed.chars().take(max_len).collect()
}

fn build_eval_session_id(run_id: &str, case_id: &str) -> String {
    let run_short = shorten_id(run_id, 8);
    let random = Uuid::new_v4().simple().to_string();
    let random_short = shorten_id(&random, 6);
    if run_short.is_empty() {
        return format!("eval_{}_{}", case_id, random_short);
    }
    format!("eval_{run_short}_{case_id}_{random_short}")
}

fn json_contains(actual: &Value, required: &Value) -> bool {
    match required {
        Value::Null => actual.is_null(),
        Value::Bool(expected) => actual.as_bool() == Some(*expected),
        Value::Number(expected) => {
            if let (Some(actual_int), Some(expected_int)) = (actual.as_i64(), expected.as_i64()) {
                actual_int == expected_int
            } else if let (Some(actual_f64), Some(expected_f64)) =
                (actual.as_f64(), expected.as_f64())
            {
                actual_f64 == expected_f64
            } else {
                false
            }
        }
        Value::String(expected) => actual.as_str() == Some(expected.as_str()),
        Value::Array(expected_items) => match actual.as_array() {
            Some(actual_items) => {
                if actual_items.len() < expected_items.len() {
                    return false;
                }
                for (index, expected_item) in expected_items.iter().enumerate() {
                    if !json_contains(&actual_items[index], expected_item) {
                        return false;
                    }
                }
                true
            }
            None => false,
        },
        Value::Object(expected_map) => match actual.as_object() {
            Some(actual_map) => {
                for (key, expected_value) in expected_map {
                    let Some(actual_value) = actual_map.get(key) else {
                        return false;
                    };
                    if !json_contains(actual_value, expected_value) {
                        return false;
                    }
                }
                true
            }
            None => false,
        },
    }
}

fn tool_call_block_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<tool_call\b[^>]*>(?P<payload>.*?)</tool_call\s*>").ok())
        .as_ref()
}

fn tool_block_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<tool\b[^>]*>(?P<payload>.*?)</tool\s*>").ok())
        .as_ref()
}

fn tool_open_tag_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<(tool_call|tool)\b[^>]*>").ok())
        .as_ref()
}

fn tool_close_tag_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)</(tool_call|tool)\s*>").ok())
        .as_ref()
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

fn dimension_label(dimension: EvaluationDimension) -> &'static str {
    match dimension {
        EvaluationDimension::Tool => "tool",
        EvaluationDimension::Logic => "logic",
        EvaluationDimension::Common => "common",
        EvaluationDimension::Complex => "complex",
    }
}

fn dimension_weight(weights: &DimensionWeights, dimension: EvaluationDimension) -> f64 {
    match dimension {
        EvaluationDimension::Tool => weights.tool,
        EvaluationDimension::Logic => weights.logic,
        EvaluationDimension::Common => weights.common,
        EvaluationDimension::Complex => weights.complex,
    }
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}
