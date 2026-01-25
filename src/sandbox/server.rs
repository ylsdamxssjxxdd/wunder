use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::i18n;

const DEFAULT_WORKSPACE_ROOT: &str = "/workspaces";
const DEFAULT_COMMAND_TIMEOUT_S: u64 = 30;
const PTC_TIMEOUT_S: u64 = 60;
const PTC_DIR_NAME: &str = "ptc_temp";

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
    image: String,
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
    allow_roots: Vec<PathBuf>,
    deny_globs: Vec<Regex>,
    allow_commands: HashSet<String>,
}

struct ToolResult {
    ok: bool,
    data: Value,
    error: String,
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
        &request.image,
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

    let allow_roots = build_allow_roots(&workspace_root, &container_root, &request.allow_paths);
    let deny_globs = build_deny_globs(&request.deny_globs);
    let allow_commands = request
        .allow_commands
        .iter()
        .map(|item| item.trim().to_lowercase())
        .filter(|item| !item.is_empty())
        .collect::<HashSet<_>>();

    let context = SandboxContext {
        workspace_root,
        allow_roots,
        deny_globs,
        allow_commands,
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

async fn execute_command(context: &SandboxContext, args: &Value) -> ToolResult {
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if content.is_empty() {
        return ToolResult {
            ok: false,
            data: json!({}),
            error: i18n::t("tool.exec.command_required"),
        };
    }

    let timeout_s = parse_timeout_secs(args.get("timeout_s")).unwrap_or(DEFAULT_COMMAND_TIMEOUT_S);
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
                data: json!({}),
                error,
            };
        }
    };
    if !cwd.exists() {
        return ToolResult {
            ok: false,
            data: json!({}),
            error: i18n::t("tool.exec.workdir_not_found"),
        };
    }
    if !cwd.is_dir() {
        return ToolResult {
            ok: false,
            data: json!({}),
            error: i18n::t("tool.exec.workdir_not_dir"),
        };
    }

    let allow_all = context.allow_commands.contains("*");
    let mut results = Vec::new();

    for raw_line in content.lines() {
        let command = raw_line.trim();
        if command.is_empty() {
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
                    data: json!({}),
                    error: i18n::t("tool.exec.not_allowed"),
                };
            }
        }

        let output = run_shell_command(command, &cwd, timeout_s).await;

        let output = match output {
            Ok(output) => output,
            Err(detail) => {
                return ToolResult {
                    ok: false,
                    data: json!({}),
                    error: i18n::t_with_params(
                        "tool.exec.command_failed",
                        &std::collections::HashMap::from([("detail".to_string(), detail)]),
                    ),
                };
            }
        };

        results.push(json!({
            "command": command,
            "returncode": output.returncode,
            "stdout": output.stdout,
            "stderr": output.stderr,
        }));

        if output.returncode != 0 {
            return ToolResult {
                ok: false,
                data: json!({ "results": results }),
                error: i18n::t("tool.exec.failed"),
            };
        }
    }

    ToolResult {
        ok: true,
        data: json!({ "results": results }),
        error: String::new(),
    }
}

async fn execute_ptc(context: &SandboxContext, args: &Value) -> ToolResult {
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
}

async fn run_shell_command(
    command: &str,
    cwd: &Path,
    timeout_s: u64,
) -> Result<CommandOutput, String> {
    let mut cmd = Command::new("bash");
    cmd.arg("-lc").arg(command).current_dir(cwd);
    cmd.kill_on_drop(true);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    run_command_output(cmd, timeout_s).await
}

async fn run_python_script(
    script_path: &Path,
    workdir: &Path,
    timeout_s: u64,
) -> Result<CommandOutput, String> {
    let mut cmd = Command::new("python3");
    cmd.arg(script_path);
    cmd.current_dir(workdir);
    cmd.kill_on_drop(true);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.env("PYTHONIOENCODING", "utf-8");
    run_command_output(cmd, timeout_s).await
}

async fn run_command_output(mut cmd: Command, timeout_s: u64) -> Result<CommandOutput, String> {
    let child = cmd.spawn().map_err(|err| err.to_string())?;
    let output = if timeout_s > 0 {
        match timeout(Duration::from_secs(timeout_s), child.wait_with_output()).await {
            Ok(result) => result.map_err(|err| err.to_string())?,
            Err(_) => {
                return Err(format!("timeout after {timeout_s}s"));
            }
        }
    } else {
        child
            .wait_with_output()
            .await
            .map_err(|err| err.to_string())?
    };
    Ok(CommandOutput {
        returncode: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
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
        let base = match_allowed_root(&target, &context.allow_roots)
            .ok_or_else(|| i18n::t("tool.fs.absolute_forbidden"))?;
        check_deny_globs(&target, &base, &context.deny_globs)?;
        return Ok((target, base));
    }

    let target = normalize_posix_path(&context.workspace_root.join(rel));
    let base = match_allowed_root(&target, &context.allow_roots)
        .ok_or_else(|| i18n::t("tool.fs.path_out_of_bounds"))?;
    check_deny_globs(&target, &base, &context.deny_globs)?;
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

fn build_allow_roots(
    workspace_root: &Path,
    container_root: &Path,
    allow_paths: &[String],
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.push(workspace_root.to_path_buf());
    for raw in allow_paths {
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

fn parse_timeout_secs(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Number(num)) => num.as_u64(),
        Some(Value::String(text)) => text.trim().parse::<u64>().ok(),
        Some(Value::Bool(flag)) => Some(if *flag { 1 } else { 0 }),
        _ => None,
    }
}

fn default_container_root() -> String {
    DEFAULT_WORKSPACE_ROOT.to_string()
}
