use crate::a2a_store::A2aStore;
use crate::config::{is_debug_log_level, Config, KnowledgeBaseConfig};
use crate::lsp::LspManager;
use crate::orchestrator::Orchestrator;
use crate::skills::SkillRegistry;
use crate::state::AppState;
use crate::storage::UserAccountRecord;
use crate::tools::{build_tool_roots, execute_tool, ToolContext, ToolRoots};
use crate::user_access::{build_user_tool_context, compute_allowed_tool_names};
use crate::user_tools::UserToolBindings;
use crate::vector_knowledge;
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
    state: Arc<AppState>,
    config: Arc<Config>,
    orchestrator: Arc<Orchestrator>,
    workspace: Arc<WorkspaceManager>,
    lsp_manager: Arc<LspManager>,
    skills: Arc<SkillRegistry>,
    user_tool_bindings: Arc<UserToolBindings>,
    a2a_store: Arc<A2aStore>,
    http: Arc<reqwest::Client>,
    tool_roots: ToolRoots,
    user_id: String,
    workspace_id: String,
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
    let tool_roots = build_tool_roots(&config, &skills_snapshot, Some(&user_tool_bindings));

    let context = PerformanceContext {
        state: state.clone(),
        config: Arc::new(config),
        orchestrator: state.orchestrator.clone(),
        workspace: state.workspace.clone(),
        lsp_manager: state.lsp_manager.clone(),
        skills: Arc::new(skills_snapshot),
        user_tool_bindings: Arc::new(user_tool_bindings),
        a2a_store: Arc::new(A2aStore::new()),
        http: Arc::new(reqwest::Client::new()),
        tool_roots,
        user_id: PERF_USER_ID.to_string(),
        workspace_id: state.workspace.scoped_user_id(PERF_USER_ID, None),
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
        "tool_access",
        measure_twice(|| measure_tool_access(concurrency, &context)).await,
    ));
    metrics.push(build_metric(
        "vector_flow",
        measure_twice(|| measure_vector_flow(concurrency, &context)).await,
    ));
    metrics.push(build_metric(
        "log_write",
        measure_twice(|| measure_log_write(concurrency, &context)).await,
    ));

    cleanup_perf_dir(&context).await;
    cleanup_perf_vector_dir(&context).await;

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

fn build_perf_user_record(user_id: &str) -> UserAccountRecord {
    UserAccountRecord {
        user_id: user_id.to_string(),
        username: user_id.to_string(),
        email: None,
        password_hash: String::new(),
        roles: Vec::new(),
        status: "active".to_string(),
        access_level: "A".to_string(),
        unit_id: None,
        daily_quota: 0,
        daily_quota_used: 0,
        daily_quota_date: None,
        is_demo: false,
        created_at: 0.0,
        updated_at: 0.0,
        last_login_at: None,
    }
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
                false,
                &context.workspace.scoped_user_id(&context.user_id, None),
                None,
                None,
            )
            .await;
        Ok(started.elapsed().as_secs_f64() * 1000.0)
    })
    .await
}

async fn measure_tool_access(concurrency: usize, context: &PerformanceContext) -> MetricSummary {
    let user_id = context.user_id.clone();
    let state = context.state.clone();
    let user = build_perf_user_record(&user_id);
    run_concurrent(concurrency, move |_| {
        let user = user.clone();
        let state = state.clone();
        let user_id = user_id.clone();
        async move {
            let started = Instant::now();
            let user_context = build_user_tool_context(state.as_ref(), &user_id).await;
            let allowed = compute_allowed_tool_names(&user, &user_context);
            let mut names = allowed.into_iter().collect::<Vec<_>>();
            names.sort();
            Ok(started.elapsed().as_secs_f64() * 1000.0)
        }
    })
    .await
}

async fn measure_vector_flow(concurrency: usize, context: &PerformanceContext) -> MetricSummary {
    let base_name = format!("perf_vector_{}", context.run_id);
    let root = match vector_knowledge::resolve_vector_root(None, &base_name, true) {
        Ok(path) => path,
        Err(err) => {
            return MetricSummary {
                avg_ms: None,
                ok: false,
                error: Some(err.to_string()),
            }
        }
    };
    let base = build_vector_perf_base(&base_name, &root);
    let content = build_vector_flow_content(&context.run_id);
    let storage = context.state.storage.clone();
    run_concurrent(concurrency, move |index| {
        let base = base.clone();
        let root = root.clone();
        let content = content.clone();
        let storage = storage.clone();
        async move {
            let doc_name = format!("perf_doc_{}", index);
            let started = Instant::now();
            let meta = vector_knowledge::prepare_document(
                &base,
                None,
                storage.as_ref(),
                &root,
                &doc_name,
                None,
                &content,
                None,
            )
            .await
            .map_err(|err| err.to_string())?;
            let content = vector_knowledge::read_vector_document_content(
                storage.as_ref(),
                None,
                &base.name,
                &root,
                &meta.doc_id,
            )
            .await
            .map_err(|err| err.to_string())?;
            let _ = vector_knowledge::build_chunk_previews(&content, &meta).await;
            Ok(started.elapsed().as_secs_f64() * 1000.0)
        }
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
    let include_payload = is_debug_log_level(&context.config.observability.log_level);
    run_concurrent(concurrency, |index| async move {
        let workspace = context.workspace.clone();
        let user_id = context.user_id.clone();
        let session_id = format!("perf_log_{}_{}", context.run_id, index);
        let mut payload = json!({
            "tool": "performance_log",
            "session_id": session_id,
            "ok": true,
            "error": "",
            "args": { "index": index },
            "data": { "tag": "performance" },
            "timestamp": Local::now().to_rfc3339(),
        });
        if !include_payload {
            if let Value::Object(ref mut map) = payload {
                map.insert("__omit_payload".to_string(), Value::Bool(true));
            }
        }
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
        workspace_id: &context.workspace_id,
        agent_id: None,
        workspace: context.workspace.clone(),
        lsp_manager: context.lsp_manager.clone(),
        config: context.config.as_ref(),
        a2a_store: context.a2a_store.as_ref(),
        skills: context.skills.as_ref(),
        user_tool_manager: None,
        user_tool_bindings: Some(context.user_tool_bindings.as_ref()),
        user_tool_store: None,
        allow_roots: Some(context.tool_roots.allow_roots.clone()),
        read_roots: Some(context.tool_roots.read_roots.clone()),
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
        .resolve_path(&context.workspace_id, path)
        .map_err(|err| err.to_string())?;
    tokio::fs::create_dir_all(&target)
        .await
        .map_err(|err| err.to_string())
}

async fn cleanup_perf_dir(context: &PerformanceContext) {
    let dir = format!("{}/{}", PERF_ROOT_DIR, context.run_id);
    let target = context.workspace.resolve_path(&context.workspace_id, &dir);
    let Ok(target) = target else {
        return;
    };
    let _ = tokio::fs::remove_dir_all(&target).await;
}

async fn cleanup_perf_vector_dir(context: &PerformanceContext) {
    let base_name = format!("perf_vector_{}", context.run_id);
    let owner_key = vector_knowledge::resolve_owner_key(None);
    let _ = context
        .state
        .storage
        .delete_vector_documents_by_base(&owner_key, &base_name);
    let root = vector_knowledge::resolve_vector_root(None, &base_name, false);
    let Ok(root) = root else {
        return;
    };
    let _ = tokio::fs::remove_dir_all(&root).await;
}

fn build_vector_perf_base(base_name: &str, root: &std::path::Path) -> KnowledgeBaseConfig {
    KnowledgeBaseConfig {
        name: base_name.to_string(),
        description: String::new(),
        root: root.to_string_lossy().to_string(),
        enabled: true,
        shared: Some(true),
        base_type: Some("vector".to_string()),
        embedding_model: Some("perf".to_string()),
        chunk_size: None,
        chunk_overlap: None,
        top_k: None,
        score_threshold: None,
    }
}

fn build_vector_flow_content(run_id: &str) -> String {
    let seed = format!("vector perf sample {run_id} lorem ipsum dolor sit amet.\n");
    seed.repeat(120)
}
