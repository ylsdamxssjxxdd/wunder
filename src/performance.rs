use crate::a2a_store::A2aStore;
use crate::config::Config;
use crate::lsp::LspManager;
use crate::orchestrator::Orchestrator;
use crate::skills::SkillRegistry;
use crate::state::AppState;
use crate::tools::{execute_tool, ToolContext};
use crate::user_tools::UserToolBindings;
use crate::workspace::WorkspaceManager;
use chrono::Local;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

const PERF_USER_ID: &str = "performance_admin";
const PERF_ROOT_DIR: &str = ".wunder_perf";
const DEFAULT_COMMAND: &str = "echo wunder_perf";
const COMMAND_TIMEOUT_S: u64 = 5;

#[derive(Debug, Deserialize)]
pub struct PerformanceSampleRequest {
    pub concurrency: usize,
    #[serde(default)]
    pub command: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PerformanceMetricSample {
    pub key: String,
    pub avg_ms: Option<f64>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PerformanceSampleResponse {
    pub concurrency: usize,
    pub metrics: Vec<PerformanceMetricSample>,
}

struct PerformanceContext {
    config: Arc<Config>,
    orchestrator: Arc<Orchestrator>,
    workspace: Arc<WorkspaceManager>,
    lsp_manager: Arc<LspManager>,
    skills: Arc<SkillRegistry>,
    user_tool_bindings: Arc<UserToolBindings>,
    a2a_store: Arc<A2aStore>,
    http: Arc<reqwest::Client>,
    user_id: String,
    run_id: String,
}

struct MetricSummary {
    avg_ms: Option<f64>,
    ok: bool,
    error: Option<String>,
}

pub async fn run_sample(
    state: Arc<AppState>,
    request: PerformanceSampleRequest,
) -> Result<PerformanceSampleResponse, String> {
    let concurrency = request.concurrency;
    if concurrency == 0 {
        return Err("并发数必须大于 0".to_string());
    }
    let config = state.config_store.get().await;
    let max_allowed = config.server.max_active_sessions.max(1);
    if concurrency > max_allowed {
        return Err(format!("并发数不能超过 {max_allowed}"));
    }
    let skills_snapshot = state.skills.read().await.clone();
    let user_tool_bindings =
        state
            .user_tool_manager
            .build_bindings(&config, &skills_snapshot, PERF_USER_ID);

    let context = PerformanceContext {
        config: Arc::new(config),
        orchestrator: state.orchestrator.clone(),
        workspace: state.workspace.clone(),
        lsp_manager: state.lsp_manager.clone(),
        skills: Arc::new(skills_snapshot),
        user_tool_bindings: Arc::new(user_tool_bindings),
        a2a_store: Arc::new(A2aStore::new()),
        http: Arc::new(reqwest::Client::new()),
        user_id: PERF_USER_ID.to_string(),
        run_id: Uuid::new_v4().simple().to_string(),
    };

    let command = request
        .command
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_COMMAND.to_string());

    let mut metrics = Vec::new();
    metrics.push(build_metric(
        "prompt_build",
        measure_twice(|| measure_prompt_build(concurrency, &context)).await,
    ));
    metrics.push(build_metric(
        "file_ops",
        measure_twice(|| measure_file_ops(concurrency, &context)).await,
    ));
    metrics.push(build_metric(
        "command_exec",
        measure_twice(|| measure_command_exec(concurrency, &context, &command)).await,
    ));
    metrics.push(build_metric(
        "log_write",
        measure_twice(|| measure_log_write(concurrency, &context)).await,
    ));

    cleanup_perf_dir(&context).await;

    Ok(PerformanceSampleResponse {
        concurrency,
        metrics,
    })
}

fn build_metric(key: &str, summary: MetricSummary) -> PerformanceMetricSample {
    PerformanceMetricSample {
        key: key.to_string(),
        avg_ms: summary.avg_ms,
        ok: summary.ok,
        error: summary.error,
    }
}

async fn measure_twice<F, Fut>(mut op: F) -> MetricSummary
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = MetricSummary>,
{
    let first = op().await;
    let second = op().await;
    merge_summaries(first, second)
}

fn merge_summaries(first: MetricSummary, second: MetricSummary) -> MetricSummary {
    let mut values = Vec::new();
    if let Some(value) = first.avg_ms {
        values.push(value);
    }
    if let Some(value) = second.avg_ms {
        values.push(value);
    }
    let avg_ms = if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    };
    let ok = first.ok && second.ok;
    let error = if ok {
        None
    } else {
        first.error.or(second.error)
    };
    MetricSummary { avg_ms, ok, error }
}

async fn measure_prompt_build(concurrency: usize, context: &PerformanceContext) -> MetricSummary {
    run_concurrent(concurrency, |index| async move {
        let _ = index;
        let started = Instant::now();
        let _prompt = context
            .orchestrator
            .build_system_prompt(
                context.config.as_ref(),
                &Vec::new(),
                context.skills.as_ref(),
                Some(context.user_tool_bindings.as_ref()),
                &context.user_id,
                None,
            )
            .await;
        Ok(started.elapsed().as_secs_f64() * 1000.0)
    })
    .await
}

async fn measure_file_ops(concurrency: usize, context: &PerformanceContext) -> MetricSummary {
    run_concurrent(concurrency, |index| async move {
        let dir = format!("{}/{}/{}", PERF_ROOT_DIR, context.run_id, index);
        let file_path = format!("{dir}/sample.txt");
        let session_id = format!("perf_file_{}_{}", context.run_id, index);
        let tool_context = build_tool_context(context, &session_id);
        prepare_dir(context, &dir).await?;

        let content = format!("performance sample {}\nneedle\n", context.run_id);
        let started = Instant::now();
        run_tool(
            &tool_context,
            "列出文件",
            json!({ "path": dir.clone(), "max_depth": 1 }),
        )
        .await?;
        run_tool(
            &tool_context,
            "写入文件",
            json!({ "path": file_path.clone(), "content": content }),
        )
        .await?;
        run_tool(
            &tool_context,
            "读取文件",
            json!({ "files": [{ "path": file_path.clone() }] }),
        )
        .await?;
        run_tool(
            &tool_context,
            "搜索内容",
            json!({
                "query": "needle",
                "path": dir,
                "file_pattern": "*.txt",
                "max_depth": 1,
                "max_files": 10
            }),
        )
        .await?;
        run_tool(
            &tool_context,
            "替换文本",
            json!({
                "path": file_path,
                "old_string": "needle",
                "new_string": "needle_replaced",
                "expected_replacements": 1
            }),
        )
        .await?;
        Ok(started.elapsed().as_secs_f64() * 1000.0)
    })
    .await
}

async fn measure_command_exec(
    concurrency: usize,
    context: &PerformanceContext,
    command: &str,
) -> MetricSummary {
    let command = command.to_string();
    run_concurrent(concurrency, move |index| {
        let command = command.clone();
        async move {
            let session_id = format!("perf_cmd_{}_{}", context.run_id, index);
            let tool_context = build_tool_context(context, &session_id);
            let started = Instant::now();
            run_tool(
                &tool_context,
                "执行命令",
                json!({ "content": command, "timeout_s": COMMAND_TIMEOUT_S }),
            )
            .await?;
            Ok(started.elapsed().as_secs_f64() * 1000.0)
        }
    })
    .await
}

async fn measure_log_write(concurrency: usize, context: &PerformanceContext) -> MetricSummary {
    run_concurrent(concurrency, |index| async move {
        let workspace = context.workspace.clone();
        let user_id = context.user_id.clone();
        let session_id = format!("perf_log_{}_{}", context.run_id, index);
        let payload = json!({
            "tool": "performance_log",
            "session_id": session_id,
            "ok": true,
            "error": "",
            "args": { "index": index },
            "data": { "tag": "performance" },
            "timestamp": Local::now().to_rfc3339(),
        });
        let started = Instant::now();
        tokio::task::spawn_blocking(move || workspace.append_tool_log(&user_id, &payload))
            .await
            .map_err(|err| err.to_string())?
            .map_err(|err| err.to_string())?;
        Ok(started.elapsed().as_secs_f64() * 1000.0)
    })
    .await
}

async fn run_concurrent<F, Fut>(concurrency: usize, op: F) -> MetricSummary
where
    F: Fn(usize) -> Fut,
    Fut: std::future::Future<Output = Result<f64, String>>,
{
    let mut tasks = Vec::with_capacity(concurrency);
    for index in 0..concurrency {
        tasks.push(op(index));
    }
    let results = join_all(tasks).await;
    let mut durations = Vec::new();
    let mut error = None;
    for result in results {
        match result {
            Ok(value) => durations.push(value),
            Err(message) => {
                if error.is_none() {
                    error = Some(message);
                }
            }
        }
    }
    let avg_ms = if durations.is_empty() {
        None
    } else {
        Some(durations.iter().sum::<f64>() / durations.len() as f64)
    };
    MetricSummary {
        avg_ms,
        ok: error.is_none(),
        error,
    }
}

fn build_tool_context<'a>(context: &'a PerformanceContext, session_id: &'a str) -> ToolContext<'a> {
    ToolContext {
        user_id: &context.user_id,
        session_id,
        workspace: context.workspace.clone(),
        lsp_manager: context.lsp_manager.clone(),
        config: context.config.as_ref(),
        a2a_store: context.a2a_store.as_ref(),
        skills: context.skills.as_ref(),
        user_tool_manager: None,
        user_tool_bindings: Some(context.user_tool_bindings.as_ref()),
        user_tool_store: None,
        event_emitter: None,
        http: context.http.as_ref(),
    }
}

async fn run_tool(context: &ToolContext<'_>, name: &str, args: Value) -> Result<(), String> {
    let result = execute_tool(context, name, &args)
        .await
        .map_err(|err| err.to_string())?;
    let ok = result.get("ok").and_then(Value::as_bool);
    if ok == Some(false) {
        let message = result
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("tool failed");
        return Err(message.to_string());
    }
    Ok(())
}

async fn prepare_dir(context: &PerformanceContext, path: &str) -> Result<(), String> {
    let target = context
        .workspace
        .resolve_path(&context.user_id, path)
        .map_err(|err| err.to_string())?;
    tokio::fs::create_dir_all(&target)
        .await
        .map_err(|err| err.to_string())
}

async fn cleanup_perf_dir(context: &PerformanceContext) {
    let dir = format!("{}/{}", PERF_ROOT_DIR, context.run_id);
    let target = context.workspace.resolve_path(&context.user_id, &dir);
    let Ok(target) = target else {
        return;
    };
    let _ = tokio::fs::remove_dir_all(&target).await;
}
