use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::command_utils;
use crate::core::python_runtime;
use crate::core::tool_args::recover_tool_args_value as recover_tool_args_value_lossy;
use crate::i18n;
use crate::services::tools::command_options::{
    apply_time_budget_secs, parse_command_budget, parse_dry_run,
};
use crate::services::tools::command_output_guard::{
    derive_capture_policies, render_command_output, CommandOutputCapture, CommandOutputCaptureMeta,
    CommandOutputCollector, CommandOutputPolicy, STDERR_CAPTURE_POLICY, STDOUT_CAPTURE_POLICY,
};
use crate::services::tools::tool_error::{
    build_execute_command_failure_data, build_execute_command_failure_message, with_error_meta,
    ToolErrorMeta,
};

const DEFAULT_WORKSPACE_ROOT: &str = "/workspaces";
const DEFAULT_COMMAND_TIMEOUT_S: f64 = 30.0;
const PTC_TIMEOUT_S: u64 = 60;
const PTC_DIR_NAME: &str = "ptc_temp";
const RULES_CACHE_CAPACITY: usize = 512;
const RULES_CACHE_TTL: Duration = Duration::from_secs(600);
const STREAM_READ_CHUNK_SIZE: usize = 4096;
const STREAM_DRAIN_TIMEOUT_MS: u64 = 2000;

#[derive(Debug, Deserialize)]
struct SandboxToolRequest {
    user_id: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    language: String,
    tool: String,
    #[serde(default)]
    args: Value,
    workspace_root: String,
    #[serde(default)]
    allow_paths: Vec<String>,
    #[serde(default)]
    deny_globs: Vec<String>,
    #[serde(default)]
    allow_commands: Vec<String>,
    #[serde(default = "default_container_root")]
    container_root: String,
    #[serde(default)]
    network: String,
    #[serde(default)]
    readonly_rootfs: bool,
    #[serde(default)]
    idle_ttl_s: u64,
    #[serde(default)]
    resources: SandboxResources,
}

#[derive(Debug, Default, Deserialize)]
struct SandboxResources {
    cpu: f32,
    memory_mb: u64,
    pids: u64,
}

#[derive(Debug, Serialize)]
struct SandboxToolResponse {
    ok: bool,
    data: Value,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
    #[serde(default)]
    debug_events: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct SandboxReleaseRequest {
    user_id: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    language: String,
}

#[derive(Debug, Serialize)]
struct SandboxReleaseResponse {
    ok: bool,
    #[serde(default)]
    message: String,
}

struct SandboxContext {
    workspace_root: PathBuf,
    allow_roots: Arc<Vec<PathBuf>>,
    deny_globs: Arc<Vec<Regex>>,
    allow_commands: Arc<HashSet<String>>,
}

struct ToolResult {
    ok: bool,
    data: Value,
    error: String,
}

#[derive(Clone)]
struct CachedSandboxRules {
    allow_roots: Arc<Vec<PathBuf>>,
    deny_globs: Arc<Vec<Regex>>,
    allow_commands: Arc<HashSet<String>>,
}

struct RulesCacheEntry {
    rules: CachedSandboxRules,
    last_used: Instant,
}

struct SandboxRulesCache {
    entries: HashMap<u64, RulesCacheEntry>,
    order: VecDeque<u64>,
}

impl SandboxRulesCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&mut self, key: u64) -> Option<CachedSandboxRules> {
        let now = Instant::now();
        if let Some(entry) = self.entries.get(&key) {
            if now.duration_since(entry.last_used) > RULES_CACHE_TTL {
                self.entries.remove(&key);
                self.remove_from_order(key);
                return None;
            }
        }
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = now;
            let rules = entry.rules.clone();
            self.touch(key);
            return Some(rules);
        }
        None
    }

    fn insert(&mut self, key: u64, rules: CachedSandboxRules) {
        let now = Instant::now();
        self.entries.insert(
            key,
            RulesCacheEntry {
                rules,
                last_used: now,
            },
        );
        self.touch(key);
        self.evict_expired(now);
        self.evict_overflow();
    }

    fn touch(&mut self, key: u64) {
        self.remove_from_order(key);
        self.order.push_back(key);
    }

    fn remove_from_order(&mut self, key: u64) {
        if let Some(pos) = self.order.iter().position(|item| *item == key) {
            self.order.remove(pos);
        }
    }

    fn evict_expired(&mut self, now: Instant) {
        loop {
            let Some(&key) = self.order.front() else {
                break;
            };
            let expired = self
                .entries
                .get(&key)
                .map(|entry| now.duration_since(entry.last_used) > RULES_CACHE_TTL)
                .unwrap_or(true);
            if !expired {
                break;
            }
            self.order.pop_front();
            self.entries.remove(&key);
        }
    }

    fn evict_overflow(&mut self) {
        while self.entries.len() > RULES_CACHE_CAPACITY {
            if let Some(key) = self.order.pop_front() {
                self.entries.remove(&key);
            } else {
                break;
            }
        }
    }
}

static SANDBOX_RULES_CACHE: OnceLock<Mutex<SandboxRulesCache>> = OnceLock::new();

fn rules_cache() -> &'static Mutex<SandboxRulesCache> {
    SANDBOX_RULES_CACHE.get_or_init(|| Mutex::new(SandboxRulesCache::new()))
}

pub fn build_router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/sandboxes/execute_tool", post(execute_tool))
        .route("/sandboxes/release", post(release_sandbox))
}

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

async fn execute_tool(Json(request): Json<SandboxToolRequest>) -> impl IntoResponse {
    let language = i18n::resolve_language([request.language.as_str()]);
    i18n::with_language(language, async move {
        let response = handle_execute_tool(request).await;
        (StatusCode::OK, Json(response))
    })
    .await
}

async fn release_sandbox(Json(request): Json<SandboxReleaseRequest>) -> impl IntoResponse {
    let language = i18n::resolve_language([request.language.as_str()]);
    i18n::with_language(language, async move {
        let _ = (&request.user_id, &request.session_id);
        let response = SandboxReleaseResponse {
            ok: true,
            message: i18n::t("sandbox.message.release_not_required"),
        };
        (StatusCode::OK, Json(response))
    })
    .await
}

async fn handle_execute_tool(request: SandboxToolRequest) -> SandboxToolResponse {
    // Touch reserved fields to keep payload compatibility without warnings.
    let _ = (
        &request.user_id,
        &request.session_id,
        &request.network,
        request.readonly_rootfs,
        request.idle_ttl_s,
        request.resources.cpu,
        request.resources.memory_mb,
        request.resources.pids,
    );
    let container_root = normalize_container_root(&request.container_root);
    let workspace_root = match normalize_container_path(&request.workspace_root, &container_root) {
        Ok(path) => path,
        Err(message) => {
            return SandboxToolResponse {
                ok: false,
                data: json!({}),
                error: message,
                debug_events: Vec::new(),
            };
        }
    };

    let rules = resolve_cached_rules(
        &workspace_root,
        &container_root,
        &request.allow_paths,
        &request.deny_globs,
        &request.allow_commands,
    );

    let context = SandboxContext {
        workspace_root,
        allow_roots: rules.allow_roots,
        deny_globs: rules.deny_globs,
        allow_commands: rules.allow_commands,
    };

    let args = if request.args.is_null() {
        json!({})
    } else {
        request.args.clone()
    };

    let result = match request.tool.as_str() {
        "执行命令" => execute_command(&context, &args).await,
        "ptc" => execute_ptc(&context, &args).await,
        _ => ToolResult {
            ok: false,
            data: json!({}),
            error: i18n::t("sandbox.error.unsupported_tool"),
        },
    };

    SandboxToolResponse {
        ok: result.ok,
        data: if result.data.is_object() {
            result.data
        } else {
            json!({ "result": result.data })
        },
        error: result.error,
        debug_events: Vec::new(),
    }
}

fn recover_tool_args_value(args: &Value) -> Value {
    recover_tool_args_value_lossy(args)
}

async fn execute_command(context: &SandboxContext, args: &Value) -> ToolResult {
    let args = recover_tool_args_value(args);
    let dry_run = parse_dry_run(&args);
    let command_budget = parse_command_budget(&args);
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if content.is_empty() {
        return ToolResult {
            ok: false,
            data: with_error_meta(
                json!({}),
                ToolErrorMeta::new(
                    "TOOL_EXEC_COMMAND_REQUIRED",
                    Some("请在 content 中提供要执行的命令或脚本文本。".to_string()),
                    false,
                    None,
                ),
            ),
            error: i18n::t("tool.exec.command_required"),
        };
    }

    let timeout_s = parse_timeout_secs(args.get("timeout_s")).unwrap_or(DEFAULT_COMMAND_TIMEOUT_S);
    let timeout_s = apply_time_budget_secs(timeout_s, &command_budget);
    let workdir = args
        .get("workdir")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let workdir = if workdir.is_empty() { "." } else { &workdir };
    let cwd = match resolve_path(context, workdir) {
        Ok(path) => path,
        Err(error) => {
            return ToolResult {
                ok: false,
                data: with_error_meta(
                    json!({ "workdir": workdir }),
                    ToolErrorMeta::new(
                        "TOOL_EXEC_WORKDIR_INVALID",
                        Some("请确认 workdir 路径存在且在允许范围内。".to_string()),
                        false,
                        None,
                    ),
                ),
                error,
            };
        }
    };
    if !cwd.exists() {
        return ToolResult {
            ok: false,
            data: with_error_meta(
                json!({ "workdir": workdir }),
                ToolErrorMeta::new(
                    "TOOL_EXEC_WORKDIR_NOT_FOUND",
                    Some("请确认 workdir 路径存在且在允许范围内。".to_string()),
                    false,
                    None,
                ),
            ),
            error: i18n::t("tool.exec.workdir_not_found"),
        };
    }
    if !cwd.is_dir() {
        return ToolResult {
            ok: false,
            data: with_error_meta(
                json!({ "workdir": workdir }),
                ToolErrorMeta::new(
                    "TOOL_EXEC_WORKDIR_NOT_DIR",
                    Some("请将 workdir 指向目录而非文件。".to_string()),
                    false,
                    None,
                ),
            ),
            error: i18n::t("tool.exec.workdir_not_dir"),
        };
    }

    let allow_all = context.allow_commands.contains("*");
    let (stdout_policy, stderr_policy) =
        derive_capture_policies(command_budget.output_budget_bytes);
    let effective_output_budget_bytes = stdout_policy
        .max_bytes()
        .saturating_add(stderr_policy.max_bytes());
    let mut results = Vec::new();
    let mut guarded_total_bytes: usize = 0;
    let mut guarded_omitted_bytes: usize = 0;
    let mut guarded_total_commands: usize = 0;
    let mut guarded_truncated_commands: usize = 0;

    let commands = if allow_all {
        vec![content.clone()]
    } else {
        content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };
    if let Some(max_commands) = command_budget.max_commands {
        if commands.len() > max_commands {
            return ToolResult {
                ok: false,
                data: with_error_meta(
                    json!({
                        "command_count": commands.len(),
                        "max_commands": max_commands,
                    }),
                    ToolErrorMeta::new(
                        "TOOL_EXEC_BUDGET_COMMAND_LIMIT",
                        Some("请减少单次执行命令数量，或提高 max_commands 预算。".to_string()),
                        true,
                        Some(200),
                    ),
                ),
                error: format!(
                    "command count {} exceeds budget limit {}",
                    commands.len(),
                    max_commands
                ),
            };
        }
    }
    if dry_run {
        return ToolResult {
            ok: true,
            data: json!({
                "dry_run": true,
                "workdir": cwd.to_string_lossy().to_string(),
                "command_count": commands.len(),
                "commands": commands,
                "timeout_s": timeout_s,
                "budget": command_budget.to_json(),
                "meta": {
                    "output_guard": {
                        "effective_total_bytes": effective_output_budget_bytes,
                    }
                }
            }),
            error: String::new(),
        };
    }

    for command in commands {
        if command.trim().is_empty() {
            continue;
        }
        if !allow_all {
            let lower = command.to_lowercase();
            if !context
                .allow_commands
                .iter()
                .any(|item| lower.starts_with(item))
            {
                return ToolResult {
                    ok: false,
                    data: with_error_meta(
                        json!({ "command": command }),
                        ToolErrorMeta::new(
                            "TOOL_EXEC_NOT_ALLOWED",
                            Some("命令不在 allow_commands 白名单内。".to_string()),
                            false,
                            None,
                        ),
                    ),
                    error: i18n::t("tool.exec.not_allowed"),
                };
            }
        }

        let output =
            run_shell_command(&command, &cwd, timeout_s, stdout_policy, stderr_policy).await;

        let output = match output {
            Ok(output) => output,
            Err(detail) => {
                return ToolResult {
                    ok: false,
                    data: with_error_meta(
                        json!({
                            "command": command,
                        }),
                        ToolErrorMeta::new(
                            "TOOL_EXEC_COMMAND_FAILED",
                            Some("请检查命令内容、运行环境或可执行文件是否存在。".to_string()),
                            true,
                            Some(200),
                        ),
                    ),
                    error: i18n::t_with_params(
                        "tool.exec.command_failed",
                        &std::collections::HashMap::from([("detail".to_string(), detail)]),
                    ),
                };
            }
        };

        let command_total_bytes = output
            .stdout_capture
            .total_bytes
            .saturating_add(output.stderr_capture.total_bytes);
        let command_omitted_bytes = output
            .stdout_capture
            .omitted_bytes
            .saturating_add(output.stderr_capture.omitted_bytes);
        let command_truncated = output.stdout_capture.truncated || output.stderr_capture.truncated;
        guarded_total_bytes = guarded_total_bytes.saturating_add(command_total_bytes);
        guarded_omitted_bytes = guarded_omitted_bytes.saturating_add(command_omitted_bytes);
        guarded_total_commands = guarded_total_commands.saturating_add(1);
        if command_truncated {
            guarded_truncated_commands = guarded_truncated_commands.saturating_add(1);
        }

        results.push(json!({
            "command": command,
            "returncode": output.returncode,
            "stdout": output.stdout,
            "stderr": output.stderr,
            "output_meta": {
                "truncated": command_truncated,
                "total_bytes": command_total_bytes,
                "omitted_bytes": command_omitted_bytes,
                "stdout": output.stdout_capture.to_json(),
                "stderr": output.stderr_capture.to_json(),
            },
        }));

        if output.timed_out {
            return ToolResult {
                ok: false,
                data: with_error_meta(
                    build_execute_command_failure_data(
                        &results,
                        guarded_total_commands,
                        guarded_truncated_commands > 0,
                        guarded_omitted_bytes,
                        true,
                    ),
                    ToolErrorMeta::new(
                        "TOOL_EXEC_TIMEOUT",
                        Some(
                            "命令执行超时，可拆分脚本或提高 timeout/budget.time_budget_ms 后重试。"
                                .to_string(),
                        ),
                        true,
                        Some(500),
                    ),
                ),
                error: build_execute_command_failure_message(&results, true),
            };
        }

        if output.returncode != 0 {
            return ToolResult {
                ok: false,
                data: with_error_meta(
                    build_execute_command_failure_data(
                        &results,
                        guarded_total_commands,
                        guarded_truncated_commands > 0,
                        guarded_omitted_bytes,
                        false,
                    ),
                    ToolErrorMeta::new(
                        "TOOL_EXEC_NON_ZERO_EXIT",
                        Some("命令返回非 0，请先根据 stderr 修正后再重试。".to_string()),
                        false,
                        None,
                    ),
                ),
                error: build_execute_command_failure_message(&results, false),
            };
        }
    }

    ToolResult {
        ok: true,
        data: json!({
            "results": results,
            "meta": {
                "output_guard": {
                    "truncated": guarded_truncated_commands > 0,
                    "commands": guarded_total_commands,
                    "truncated_commands": guarded_truncated_commands,
                    "total_bytes": guarded_total_bytes,
                    "omitted_bytes": guarded_omitted_bytes,
                    "effective_total_bytes": effective_output_budget_bytes,
                }
            },
            "budget": command_budget.to_json()
        }),
        error: String::new(),
    }
}

async fn execute_ptc(context: &SandboxContext, args: &Value) -> ToolResult {
    let args = recover_tool_args_value(args);
    let filename = args
        .get("filename")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let workdir = args
        .get("workdir")
        .and_then(Value::as_str)
        .unwrap_or(".")
        .trim()
        .to_string();
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");

    if filename.is_empty() {
        return ToolResult {
            ok: false,
            data: json!({}),
            error: i18n::t("tool.ptc.filename_required"),
        };
    }
    if content.trim().is_empty() {
        return ToolResult {
            ok: false,
            data: json!({}),
            error: i18n::t("tool.ptc.content_required"),
        };
    }

    let mut script_name = PathBuf::from(&filename);
    if script_name.file_name().and_then(|name| name.to_str()) != Some(&filename) {
        return ToolResult {
            ok: false,
            data: json!({}),
            error: i18n::t("tool.ptc.filename_invalid"),
        };
    }
    if script_name.extension().is_none() {
        script_name.set_extension("py");
    }
    if script_name
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase())
        != Some("py".to_string())
    {
        return ToolResult {
            ok: false,
            data: json!({}),
            error: i18n::t("tool.ptc.ext_invalid"),
        };
    }

    let workdir_path = match resolve_path(context, &workdir) {
        Ok(path) => path,
        Err(error) => {
            return ToolResult {
                ok: false,
                data: json!({}),
                error,
            };
        }
    };
    let ptc_root = match resolve_path(context, PTC_DIR_NAME) {
        Ok(path) => path,
        Err(error) => {
            return ToolResult {
                ok: false,
                data: json!({}),
                error,
            };
        }
    };

    if let Err(err) = tokio::fs::create_dir_all(&workdir_path).await {
        return ToolResult {
            ok: false,
            data: json!({}),
            error: i18n::t_with_params(
                "tool.ptc.exec_error",
                &std::collections::HashMap::from([("detail".to_string(), err.to_string())]),
            ),
        };
    }
    if let Err(err) = tokio::fs::create_dir_all(&ptc_root).await {
        return ToolResult {
            ok: false,
            data: json!({}),
            error: i18n::t_with_params(
                "tool.ptc.exec_error",
                &std::collections::HashMap::from([("detail".to_string(), err.to_string())]),
            ),
        };
    }

    let script_path = ptc_root.join(script_name);
    if let Err(err) = tokio::fs::write(&script_path, content).await {
        return ToolResult {
            ok: false,
            data: json!({}),
            error: i18n::t_with_params(
                "tool.ptc.exec_error",
                &std::collections::HashMap::from([("detail".to_string(), err.to_string())]),
            ),
        };
    }

    let output = run_python_script(&script_path, &workdir_path, PTC_TIMEOUT_S).await;
    let output = match output {
        Ok(output) => output,
        Err(detail) => {
            return ToolResult {
                ok: false,
                data: json!({}),
                error: i18n::t_with_params(
                    "tool.ptc.exec_error",
                    &std::collections::HashMap::from([("detail".to_string(), detail)]),
                ),
            };
        }
    };

    let data = json!({
        "path": script_path.to_string_lossy().to_string(),
        "workdir": workdir_path.to_string_lossy().to_string(),
        "returncode": output.returncode,
        "stdout": output.stdout,
        "stderr": output.stderr,
    });

    if output.returncode != 0 {
        return ToolResult {
            ok: false,
            data,
            error: i18n::t("tool.ptc.exec_failed"),
        };
    }

    ToolResult {
        ok: true,
        data,
        error: String::new(),
    }
}

struct CommandOutput {
    returncode: i32,
    stdout: String,
    stderr: String,
    timed_out: bool,
    stdout_capture: CommandOutputCaptureMeta,
    stderr_capture: CommandOutputCaptureMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandErrorKind {
    SpawnNotFound,
    SpawnFailed,
    WaitFailed,
}

#[derive(Debug)]
struct CommandError {
    kind: CommandErrorKind,
    detail: String,
}

impl CommandError {
    fn from_spawn(err: std::io::Error) -> Self {
        let kind = if command_utils::is_not_found_error(&err) {
            CommandErrorKind::SpawnNotFound
        } else {
            CommandErrorKind::SpawnFailed
        };
        CommandError {
            kind,
            detail: err.to_string(),
        }
    }

    fn from_wait(err: std::io::Error) -> Self {
        CommandError {
            kind: CommandErrorKind::WaitFailed,
            detail: err.to_string(),
        }
    }
}

async fn run_shell_command(
    command: &str,
    cwd: &Path,
    timeout_s: f64,
    stdout_policy: CommandOutputPolicy,
    stderr_policy: CommandOutputPolicy,
) -> Result<CommandOutput, String> {
    let runtime = python_runtime::resolve_python_runtime();

    if let Some(mut cmd) = runtime
        .as_ref()
        .and_then(|runtime| {
            command_utils::build_direct_command_with_python_override(command, cwd, &runtime.bin)
        })
        .or_else(|| command_utils::build_direct_command(command, cwd))
    {
        if let Some(runtime) = runtime.as_ref() {
            python_runtime::apply_python_env(&mut cmd, runtime);
        }
        match run_command_output(cmd, timeout_s, stdout_policy, stderr_policy).await {
            Ok(output) => return Ok(output),
            Err(err) if err.kind == CommandErrorKind::SpawnNotFound => {}
            Err(err) => return Err(err.detail),
        }
    }

    let mut cmd = command_utils::build_shell_command(command, cwd);
    if let Some(runtime) = runtime.as_ref() {
        python_runtime::apply_python_env(&mut cmd, runtime);
    }
    run_command_output(cmd, timeout_s, stdout_policy, stderr_policy)
        .await
        .map_err(|err| err.detail)
}

async fn run_python_script(
    script_path: &Path,
    workdir: &Path,
    timeout_s: u64,
) -> Result<CommandOutput, String> {
    let runtime = python_runtime::resolve_python_runtime();
    let python_bin = runtime
        .as_ref()
        .map(|value| value.bin.to_string_lossy().to_string())
        .unwrap_or_else(|| "python3".to_string());
    let mut cmd = Command::new(python_bin);
    cmd.arg(script_path);
    cmd.current_dir(workdir);
    cmd.env("PYTHONIOENCODING", "utf-8");
    if let Some(runtime) = runtime.as_ref() {
        python_runtime::apply_python_env(&mut cmd, runtime);
    }
    command_utils::apply_platform_spawn_options(&mut cmd);
    run_command_output(
        cmd,
        timeout_s as f64,
        STDOUT_CAPTURE_POLICY,
        STDERR_CAPTURE_POLICY,
    )
    .await
    .map_err(|err| err.detail)
}

async fn run_command_output(
    mut cmd: Command,
    timeout_s: f64,
    stdout_policy: CommandOutputPolicy,
    stderr_policy: CommandOutputPolicy,
) -> Result<CommandOutput, CommandError> {
    cmd.kill_on_drop(true);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(CommandError::from_spawn)?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_task = stdout.map(|stream| tokio::spawn(read_stream_capture(stream, stdout_policy)));
    let stderr_task = stderr.map(|stream| tokio::spawn(read_stream_capture(stream, stderr_policy)));

    let mut timed_out = false;
    let status = if timeout_s > 0.0 {
        match timeout(Duration::from_secs_f64(timeout_s), child.wait()).await {
            Ok(result) => Some(result.map_err(CommandError::from_wait)?),
            Err(_) => {
                timed_out = true;
                let _ = child.kill().await;
                let _ = child.wait().await;
                None
            }
        }
    } else {
        Some(child.wait().await.map_err(CommandError::from_wait)?)
    };
    let stdout_capture = join_capture_task(stdout_task, stdout_policy).await?;
    let stderr_capture = join_capture_task(stderr_task, stderr_policy).await?;
    let stdout = render_command_output(&stdout_capture, decode_command_output);
    let stderr = render_command_output(&stderr_capture, decode_command_output);
    let returncode = status.and_then(|item| item.code()).unwrap_or(-1);

    Ok(CommandOutput {
        returncode,
        stdout,
        stderr,
        timed_out,
        stdout_capture: stdout_capture.meta,
        stderr_capture: stderr_capture.meta,
    })
}

async fn join_capture_task(
    handle: Option<tokio::task::JoinHandle<Result<CommandOutputCapture, CommandError>>>,
    policy: CommandOutputPolicy,
) -> Result<CommandOutputCapture, CommandError> {
    let Some(mut handle) = handle else {
        return Ok(CommandOutputCollector::new(policy).finish());
    };
    match timeout(Duration::from_millis(STREAM_DRAIN_TIMEOUT_MS), &mut handle).await {
        Ok(result) => match result {
            Ok(output) => output,
            Err(err) => Err(CommandError {
                kind: CommandErrorKind::WaitFailed,
                detail: err.to_string(),
            }),
        },
        Err(_) => {
            handle.abort();
            Ok(CommandOutputCollector::new(policy).finish())
        }
    }
}

async fn read_stream_capture<R>(
    mut reader: R,
    policy: CommandOutputPolicy,
) -> Result<CommandOutputCapture, CommandError>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut collector = CommandOutputCollector::new(policy);
    let mut chunk = vec![0u8; STREAM_READ_CHUNK_SIZE];
    loop {
        let read = reader
            .read(&mut chunk)
            .await
            .map_err(CommandError::from_wait)?;
        if read == 0 {
            break;
        }
        collector.push_chunk(&chunk[..read]);
    }
    Ok(collector.finish())
}

fn decode_command_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

fn resolve_path(context: &SandboxContext, raw_path: &str) -> Result<PathBuf, String> {
    let (target, _) = resolve_path_with_base(context, raw_path)?;
    Ok(target)
}

fn resolve_path_with_base(
    context: &SandboxContext,
    raw_path: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let trimmed = normalize_slashes(raw_path.trim());
    let rel = PathBuf::from(&trimmed);
    if rel.is_absolute() {
        let target = normalize_posix_path(&rel);
        let base = match_allowed_root(&target, context.allow_roots.as_ref())
            .ok_or_else(|| i18n::t("tool.fs.absolute_forbidden"))?;
        check_deny_globs(&target, &base, context.deny_globs.as_ref())?;
        return Ok((target, base));
    }

    let target = normalize_posix_path(&context.workspace_root.join(rel));
    let base = match_allowed_root(&target, context.allow_roots.as_ref())
        .ok_or_else(|| i18n::t("tool.fs.path_out_of_bounds"))?;
    check_deny_globs(&target, &base, context.deny_globs.as_ref())?;
    Ok((target, base))
}

fn match_allowed_root(target: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    for root in roots {
        if target == root || target.starts_with(root) {
            return Some(root.clone());
        }
    }
    None
}

fn check_deny_globs(target: &Path, base: &Path, deny_globs: &[Regex]) -> Result<(), String> {
    let relative = target.strip_prefix(base).unwrap_or(target);
    let relative = relative.to_string_lossy().replace('\\', "/");
    for matcher in deny_globs {
        if matcher.is_match(&relative) {
            return Err(i18n::t("tool.fs.path_forbidden"));
        }
    }
    Ok(())
}

fn resolve_cached_rules(
    workspace_root: &Path,
    container_root: &Path,
    allow_paths: &[String],
    deny_globs: &[String],
    allow_commands: &[String],
) -> CachedSandboxRules {
    let allow_paths = normalize_allow_paths_for_cache(container_root, allow_paths);
    let deny_globs = normalize_deny_globs_for_cache(deny_globs);
    let allow_commands = normalize_allow_commands_for_cache(allow_commands);
    let key = build_rules_cache_key(
        container_root,
        workspace_root,
        &allow_paths,
        &deny_globs,
        &allow_commands,
    );

    if let Some(rules) = {
        let mut cache = rules_cache().lock();
        cache.get(key)
    } {
        return rules;
    }

    let allow_roots = build_allow_roots(workspace_root, container_root, &allow_paths);
    let deny_globs = build_deny_globs(&deny_globs);
    let allow_commands = allow_commands.into_iter().collect::<HashSet<_>>();
    let rules = CachedSandboxRules {
        allow_roots: Arc::new(allow_roots),
        deny_globs: Arc::new(deny_globs),
        allow_commands: Arc::new(allow_commands),
    };

    let mut cache = rules_cache().lock();
    if let Some(existing) = cache.get(key) {
        return existing;
    }
    cache.insert(key, rules.clone());
    rules
}

fn normalize_allow_paths_for_cache(container_root: &Path, allow_paths: &[String]) -> Vec<String> {
    let mut output = Vec::new();
    for raw in allow_paths {
        if is_allow_all_path_token(raw) {
            output.push("/".to_string());
            continue;
        }
        if let Ok(path) = normalize_container_path(raw, container_root) {
            output.push(path.to_string_lossy().to_string());
        }
    }
    output.sort();
    output.dedup();
    output
}

fn normalize_deny_globs_for_cache(patterns: &[String]) -> Vec<String> {
    let mut output = patterns
        .iter()
        .map(|pattern| pattern.trim().to_string())
        .filter(|pattern| !pattern.is_empty())
        .collect::<Vec<_>>();
    output.sort();
    output.dedup();
    output
}

fn normalize_allow_commands_for_cache(commands: &[String]) -> Vec<String> {
    let mut output = commands
        .iter()
        .map(|item| item.trim().to_lowercase())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    output.sort();
    output.dedup();
    output
}

fn build_rules_cache_key(
    container_root: &Path,
    workspace_root: &Path,
    allow_paths: &[String],
    deny_globs: &[String],
    allow_commands: &[String],
) -> u64 {
    let mut hasher = DefaultHasher::new();
    "sandbox_rules_v1".hash(&mut hasher);
    container_root.to_string_lossy().hash(&mut hasher);
    workspace_root.to_string_lossy().hash(&mut hasher);
    hash_list(&mut hasher, allow_paths);
    hash_list(&mut hasher, deny_globs);
    hash_list(&mut hasher, allow_commands);
    hasher.finish()
}

fn hash_list(hasher: &mut DefaultHasher, items: &[String]) {
    items.len().hash(hasher);
    for item in items {
        item.hash(hasher);
    }
}

fn build_allow_roots(
    workspace_root: &Path,
    container_root: &Path,
    allow_paths: &[String],
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.push(workspace_root.to_path_buf());
    for raw in allow_paths {
        if is_allow_all_path_token(raw) {
            let root = PathBuf::from("/");
            if roots.iter().all(|existing| existing != &root) {
                roots.push(root);
            }
            continue;
        }
        if let Ok(path) = normalize_container_path(raw, container_root) {
            if roots.iter().all(|existing| existing != &path) {
                roots.push(path);
            }
        }
    }
    roots
}

fn build_deny_globs(patterns: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|pattern| build_glob_matcher(pattern))
        .collect()
}

fn build_glob_matcher(pattern: &str) -> Option<Regex> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut regex = String::from("^");
    for ch in trimmed.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '.' | '(' | ')' | '[' | ']' | '{' | '}' | '+' | '|' | '^' | '$' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            _ => regex.push(ch),
        }
    }
    regex.push('$');
    Regex::new(&regex).ok()
}

fn normalize_container_root(raw: &str) -> PathBuf {
    let trimmed = normalize_slashes(raw.trim());
    let base = if trimmed.is_empty() {
        PathBuf::from(DEFAULT_WORKSPACE_ROOT)
    } else {
        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            path
        } else {
            PathBuf::from("/").join(path)
        }
    };
    normalize_posix_path(&base)
}

fn normalize_container_path(raw: &str, container_root: &Path) -> Result<PathBuf, String> {
    let text = normalize_slashes(raw.trim());
    if text.is_empty() {
        return Err(i18n::t("sandbox.error.path_required"));
    }
    if looks_like_windows_drive(&text) {
        return Err(i18n::t("sandbox.error.path_out_of_bounds"));
    }
    let mut path = PathBuf::from(text);
    if !path.is_absolute() {
        path = container_root.join(path);
    }
    let normalized = normalize_posix_path(&path);
    if !normalized.starts_with(container_root) {
        return Err(i18n::t("sandbox.error.path_out_of_bounds"));
    }
    Ok(normalized)
}

fn normalize_posix_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            _ => {}
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        normalized
    }
}

fn normalize_slashes(input: &str) -> String {
    input.replace('\\', "/")
}

fn looks_like_windows_drive(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 2 {
        return false;
    }
    bytes[1] == b':' && value.chars().next().map(|ch| ch.is_ascii_alphabetic()) == Some(true)
}

fn is_allow_all_path_token(value: &str) -> bool {
    value.trim() == "*"
}

fn parse_timeout_secs(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(num)) => num.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(Value::Bool(flag)) => Some(if *flag { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn default_container_root() -> String {
    DEFAULT_WORKSPACE_ROOT.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_timeout_secs_accepts_float_number_and_string() {
        let numeric = json!(1.5);
        let text = json!("2.25");
        assert_eq!(parse_timeout_secs(Some(&numeric)), Some(1.5));
        assert_eq!(parse_timeout_secs(Some(&text)), Some(2.25));
    }

    #[test]
    fn parse_timeout_secs_handles_bool_and_invalid_values() {
        let enabled = json!(true);
        let disabled = json!(false);
        let invalid = json!("oops");
        assert_eq!(parse_timeout_secs(Some(&enabled)), Some(1.0));
        assert_eq!(parse_timeout_secs(Some(&disabled)), Some(0.0));
        assert_eq!(parse_timeout_secs(Some(&invalid)), None);
        assert_eq!(parse_timeout_secs(None), None);
    }

    #[test]
    fn wildcard_allow_path_normalizes_to_container_root() {
        let container_root = Path::new("/workspaces");
        let normalized = normalize_allow_paths_for_cache(container_root, &["*".to_string()]);
        let roots = build_allow_roots(
            Path::new("/workspaces/admin__c__1"),
            container_root,
            &["*".to_string()],
        );

        assert_eq!(normalized, vec!["/".to_string()]);
        assert!(roots.iter().any(|root| root == Path::new("/")));
    }
}
