mod args;
mod attachments;
mod cloud_command;
mod command_session_display;
mod empty_state_animation;
mod error_display;
mod exec_events;
mod input_guard;
mod locale;
mod patch_diff;
mod path_display;
mod render;
mod runtime;
mod slash_command;
mod tool_display;
mod tool_presentation;
mod tui;
mod user_config;
mod workspace_context;

use anyhow::{anyhow, Context, Result};
use args::{
    ApprovalModeArg, Cli, ColorArg, Command, CompletionCommand, ConfigCommand, ConfigSubcommand,
    DoctorCommand, ExecCommand, GlobalArgs, McpAddCommand, McpCommand, McpGetCommand,
    McpListCommand, McpLoginCommand, McpNameCommand, McpSubcommand, ResumeCommand,
    SkillNameCommand, SkillsCommand, SkillsListCommand, SkillsSubcommand, SkillsUploadCommand,
    StressThreadCommand, ToolCallModeArg, ToolCommand, ToolRunCommand, ToolSubcommand,
};
use chrono::{Local, TimeZone};
use clap::CommandFactory;
use clap::Parser;
use clap_complete::generate;
use futures::{future::BoxFuture, StreamExt};
use render::{FinalEvent, StreamRenderer};
use runtime::{CliRuntime, TurnNotificationConfig, TurnNotificationWhen};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing_subscriber::EnvFilter;
use wunder_server::approval::{
    new_channel as new_approval_channel, ApprovalRequestRx, ApprovalResponse,
};
use wunder_server::config::{Config, LlmModelConfig};
use wunder_server::llm::{is_openai_compatible_provider, probe_openai_context_window};
use wunder_server::path_utils::is_within_root;
use wunder_server::schemas::{AttachmentPayload, WunderRequest};
use wunder_server::skills::{load_skills, SkillSpec};
use wunder_server::storage::ChatSessionRecord;
use wunder_server::tools::{
    build_tool_roots, collect_available_tool_names, execute_tool, ToolContext,
};
use wunder_server::user_tools::UserMcpServer;
use zip::ZipArchive;

const CLI_MIN_MAX_ROUNDS: u32 = 8;
const CLI_CONTEXT_PROBE_TIMEOUT_S: u64 = 15;
const CLI_DEFAULT_SESSION_TITLE: &str = "\u{65B0}\u{4F1A}\u{8BDD}";
const CLI_DEFAULT_SESSION_STATUS: &str = "active";
/// One agent turn materialises a deep future chain (orchestrator, tools,
/// durable writers). Debug builds keep every temporary, so the default process
/// stack is not enough; the whole CLI body runs on a thread with this budget.
const CLI_MAIN_STACK_BYTES: usize = 32 * 1024 * 1024;
/// Tool execution runs on runtime workers, which default to 2 MiB. Four
/// workers with this budget stay friendly to 32-bit address space.
const CLI_WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;
const CLI_WORKER_THREADS: usize = 4;

fn main() -> Result<()> {
    wunder_server::rustls_provider::install_process_default_provider();
    init_tracing();
    let worker = std::thread::Builder::new()
        .name("wunder-cli".to_string())
        .stack_size(CLI_MAIN_STACK_BYTES)
        .spawn(run_cli)
        .context("spawn cli worker failed")?;
    match worker.join() {
        Ok(result) => result,
        Err(_) => Err(anyhow!("cli worker thread panicked")),
    }
}

fn run_cli() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(CLI_WORKER_THREADS)
        .thread_stack_size(CLI_WORKER_STACK_BYTES)
        .enable_all()
        .build()
        .context("build tokio runtime failed")?;
    runtime.block_on(async move {
        let cli = Cli::parse();
        let runtime = CliRuntime::init(&cli.global).await?;

        match cli.command {
            Some(command) => dispatch_command(&runtime, &cli.global, command).await,
            None => Box::pin(run_default(&runtime, &cli.global, cli.prompt)).await,
        }
    })
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    // Diagnostics must never share stdout with the reply: `exec` promises that
    // stdout carries the answer alone.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

fn dispatch_command<'a>(
    runtime: &'a CliRuntime,
    global: &'a GlobalArgs,
    command: Command,
) -> BoxFuture<'a, Result<()>> {
    match command {
        Command::Exec(cmd) => Box::pin(handle_exec(runtime, global, cmd)),
        Command::Resume(cmd) => Box::pin(handle_resume(runtime, global, cmd)),
        Command::Tool(cmd) => Box::pin(handle_tool(runtime, global, cmd)),
        Command::Mcp(cmd) => Box::pin(handle_mcp(runtime, global, cmd)),
        Command::Skills(cmd) => Box::pin(handle_skills(runtime, global, cmd)),
        Command::Config(cmd) => Box::pin(handle_config(runtime, global, cmd)),
        Command::StressThread(cmd) => Box::pin(handle_stress_thread(runtime, global, cmd)),
        Command::Doctor(cmd) => Box::pin(handle_doctor(runtime, global, cmd)),
        Command::Cloud(cmd) => Box::pin(cloud_command::handle_cloud(runtime, global, cmd)),
        Command::Completion(cmd) => Box::pin(handle_completion(cmd)),
    }
}

async fn handle_completion(command: CompletionCommand) -> Result<()> {
    let mut cmd = Cli::command();
    generate(command.shell, &mut cmd, "wunder-cli", &mut io::stdout());
    Ok(())
}

/// 生成线程渲染压测线程：同步批量写入本地库，进度按固定间隔打到 stderr。
async fn handle_stress_thread(
    runtime: &CliRuntime,
    _global: &GlobalArgs,
    command: StressThreadCommand,
) -> Result<()> {
    wunder_server::validate_stress_params(command.user_rounds, command.model_rounds)
        .map_err(|message| anyhow!("{message}"))?;
    let config = runtime.state.config_store.get().await;
    let db_path = config.storage.db_path.clone();
    drop(config);
    let user_id = runtime.user_id.clone();
    let title = command
        .title
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "渲染压测 {}×{}",
                command.user_rounds, command.model_rounds
            )
        });
    let spec = wunder_server::storage::StressThreadSpec {
        session_id: uuid::Uuid::new_v4().to_string(),
        title: title.clone(),
        user_rounds: command.user_rounds,
        model_rounds_per_turn: command.model_rounds,
    };
    let report_every = (command.user_rounds / 50).clamp(1, 100);
    let started = std::time::Instant::now();
    let stats = tokio::task::spawn_blocking(move || {
        let storage = wunder_server::storage::SqliteStorage::new(db_path);
        storage.generate_stress_thread(&user_id, &spec, |done, items| {
            if done % report_every == 0 {
                eprint!("\r  已生成 {done} 用户轮次 / {items} 条消息…");
                let _ = io::stderr().flush();
            }
        })
    })
    .await
    .map_err(|error| anyhow!("生成线程崩溃: {error}"))?;
    let elapsed = started.elapsed();
    eprintln!();
    let stats = stats.map_err(|error| anyhow!("生成失败: {error}"))?;
    println!("会话已生成：{title}");
    println!("  会话 ID: {}", stats.session_id);
    println!("  用户轮次: {}", stats.user_turns);
    println!("  消息条目: {}", stats.items);
    println!("  工具调用: {}", stats.tool_calls);
    println!("  耗时: {:.1}s", elapsed.as_secs_f64());
    Ok(())
}

async fn run_default(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    prompt: Option<String>,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let first_prompt = match prompt {
        Some(prompt) => Some(resolve_prompt_text(Some(prompt), language.as_str())?),
        None => None,
    };

    if should_run_tui(global) {
        return tui::run_main(runtime, global, first_prompt, None).await;
    }

    // Non-TTY (pipes, CI, redirected stdio) degrades to one-shot execution,
    // mirroring `wunder-cli exec` rather than starting an invisible REPL.
    let prompt = match first_prompt {
        Some(prompt) => prompt,
        None => resolve_prompt_text(None, language.as_str())?,
    };
    let outcome = Box::pin(run_agent_once(
        runtime,
        global,
        &prompt,
        &stored_exec_session(global),
        None,
        None,
        None,
    ))
    .await?;
    apply_exec_exit_policy(&outcome, language.as_str());
    Ok(())
}

async fn handle_exec(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: ExecCommand,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let prompt = resolve_prompt_text(command.prompt, language.as_str())?;
    let attachments = prepare_global_attachment_payloads(runtime, global).await?;
    let session_id = stored_exec_session(global);
    // The orchestrator future is large in debug builds; box it instead of
    // building the whole chain on the thread stack.
    let outcome = Box::pin(run_agent_once(
        runtime,
        global,
        &prompt,
        &session_id,
        attachments,
        command.last_message_file.as_deref(),
        Some(command.color),
    ))
    .await?;
    // A round that never completed is a fatal outcome for scripts; the JSONL
    // stream has already been flushed, so the code is the only signal left.
    apply_exec_exit_policy(&outcome, language.as_str());
    Ok(())
}

fn stored_exec_session(global: &GlobalArgs) -> String {
    global
        .session
        .clone()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string())
}

async fn handle_resume(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    mut command: ResumeCommand,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    if command.last && command.prompt.is_none() {
        // Clap cannot express this positional behavior directly.
        command.prompt = command.session_id.take();
    }

    let session_id = if command.last {
        let workspace = if command.all {
            None
        } else {
            Some(runtime.workspace_id())
        };
        let sessions = list_recent_sessions_in(runtime, 1, None, workspace).await?;
        sessions
            .first()
            .map(|item| item.session_id.clone())
            .ok_or_else(|| {
                anyhow!(locale::tr(
                    language.as_str(),
                    if command.all {
                        "未找到历史会话，请先开始一次对话"
                    } else {
                        "当前工作区没有历史会话；用 resume --all <id> 可跨工作区恢复"
                    },
                    if command.all {
                        "no recorded session found, start a conversation first"
                    } else {
                        "no thread recorded in this workspace; use resume --all <id> to cross workspaces"
                    },
                ))
            })?
    } else if let Some(session_id) = command
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        session_id.to_string()
    } else {
        return Err(anyhow!(locale::tr(
            language.as_str(),
            "请指定会话 ID，或使用 --last 恢复最近的会话",
            "pass a session id, or use --last to resume the most recent session",
        )));
    };

    let first_prompt = match command.prompt {
        Some(prompt) => Some(resolve_prompt_text(Some(prompt), language.as_str())?),
        None => None,
    };
    if should_run_tui(global) {
        return tui::run_main(runtime, global, first_prompt, Some(session_id)).await;
    }

    match first_prompt {
        Some(prompt) => {
            let attachments = prepare_global_attachment_payloads(runtime, global).await?;
            let outcome = Box::pin(run_agent_once(
                runtime,
                global,
                &prompt,
                &session_id,
                attachments,
                None,
                None,
            ))
            .await?;
            apply_exec_exit_policy(&outcome, language.as_str());
            Ok(())
        }
        None => Err(anyhow!(locale::tr(
            language.as_str(),
            "非交互模式恢复会话需要提供提问内容；请在终端中运行以进入 TUI",
            "resuming without a prompt needs a TTY; run in a terminal to open the TUI",
        ))),
    }
}

fn should_run_tui(global: &GlobalArgs) -> bool {
    if global.json {
        return false;
    }
    io::stdin().is_terminal() && io::stdout().is_terminal() && io::stderr().is_terminal()
}

async fn query_recent_sessions(
    runtime: &CliRuntime,
    limit: i64,
    search: Option<&str>,
    workspace_id: Option<&str>,
) -> Result<Vec<wunder_server::ThreadSnapshot>> {
    let catalog = wunder_server::ThreadCatalogService::new((*runtime.state).clone());
    let page = catalog
        .list(wunder_server::ThreadListQuery {
            user_id: runtime.user_id.clone(),
            limit,
            search: search.map(str::to_string),
            workspace_id: workspace_id.map(str::to_string),
            ..Default::default()
        })
        .await?;
    // The catalog snapshot is the thread directory record; the TUI must not flatten the
    // typed status back into a string, because that is where a second state machine starts.
    Ok(page.items)
}

/// Threads of one workspace — the resume default. `None` lists every workspace.
pub(crate) async fn list_recent_sessions_in(
    runtime: &CliRuntime,
    limit: usize,
    search: Option<&str>,
    workspace_id: Option<&str>,
) -> Result<Vec<wunder_server::ThreadSnapshot>> {
    let limit = limit.clamp(1, 200) as i64;
    query_recent_sessions(runtime, limit, search, workspace_id).await
}

pub(crate) async fn list_recent_sessions(
    runtime: &CliRuntime,
    limit: usize,
) -> Result<Vec<wunder_server::ThreadSnapshot>> {
    list_recent_sessions_searched(runtime, limit, None).await
}

pub(crate) async fn list_recent_sessions_searched(
    runtime: &CliRuntime,
    limit: usize,
    search: Option<&str>,
) -> Result<Vec<wunder_server::ThreadSnapshot>> {
    let limit = limit.clamp(1, 200) as i64;
    query_recent_sessions(runtime, limit, search, None).await
}

pub(crate) async fn session_exists(runtime: &CliRuntime, session_id: &str) -> Result<bool> {
    let target_session = session_id.trim().to_string();
    if target_session.is_empty() {
        return Ok(false);
    }

    let user_store = runtime.state.user_store.clone();
    let user_id = runtime.user_id.clone();
    let session_for_query = target_session.clone();
    let exists = tokio::task::spawn_blocking(move || {
        user_store
            .get_chat_session(&user_id, &session_for_query)
            .map(|record| record.is_some())
    })
    .await
    .map_err(|err| anyhow!("session query cancelled: {err}"))??;
    if exists {
        return Ok(true);
    }

    ensure_cli_session_record(runtime, &target_session, None).await
}

pub(crate) async fn load_session_history_entries(
    runtime: &CliRuntime,
    session_id: &str,
    limit: i64,
) -> Result<Vec<Value>> {
    runtime
        .state
        .workspace
        .load_history_async(&runtime.user_id, session_id, limit)
        .await
}

fn format_session_time(ts: f64) -> String {
    if !ts.is_finite() || ts <= 0.0 {
        return "-".to_string();
    }
    let secs = ts.floor() as i64;
    let nanos = ((ts - secs as f64).max(0.0) * 1_000_000_000.0).round() as u32;
    Local
        .timestamp_opt(secs, nanos.min(999_999_999))
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "-".to_string())
}

async fn ensure_cli_session_record(
    runtime: &CliRuntime,
    session_id: &str,
    prompt_hint: Option<&str>,
) -> Result<bool> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Ok(false);
    }

    let title_hint = prompt_hint.and_then(build_session_title);
    if title_hint.is_none() {
        let has_history = runtime
            .state
            .workspace
            .load_history_async(&runtime.user_id, session_id, 1)
            .await
            .map(|items| !items.is_empty())
            .unwrap_or(false);
        if !has_history {
            return Ok(false);
        }
    }

    let user_store = runtime.state.user_store.clone();
    let user_id = runtime.user_id.clone();
    let workspace_id = runtime.workspace_id().to_string();
    let session_id = session_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<bool> {
        let now = current_ts();
        let mut record = user_store
            .get_chat_session(&user_id, &session_id)?
            .unwrap_or_else(|| ChatSessionRecord {
                session_id: session_id.clone(),
                user_id: user_id.clone(),
                title: title_hint
                    .clone()
                    .unwrap_or_else(|| CLI_DEFAULT_SESSION_TITLE.to_string()),
                status: CLI_DEFAULT_SESSION_STATUS.to_string(),
                created_at: now,
                updated_at: now,
                last_message_at: now,
                agent_id: None,
                workspace_id: Some(workspace_id.clone()),
                tool_overrides: Vec::new(),
                parent_session_id: None,
                parent_message_id: None,
                spawn_label: None,
                spawned_by: None,
            });

        if should_auto_title(record.title.as_str()) {
            if let Some(title) = title_hint.as_ref() {
                record.title = title.clone();
            }
        }
        // Threads always belong to the launch workspace; a legacy row adopted
        // by migration already carries one and keeps it.
        if record.workspace_id.is_none() {
            record.workspace_id = Some(workspace_id);
        }

        record.updated_at = now;
        record.last_message_at = now;
        user_store.upsert_chat_session(&record)?;
        Ok(true)
    })
    .await
    .map_err(|err| anyhow!("session metadata task cancelled: {err}"))?
}

fn should_auto_title(title: &str) -> bool {
    let cleaned = title.trim();
    cleaned.is_empty()
        || cleaned == "\u{65B0}\u{4F1A}\u{8BDD}"
        || cleaned == "\u{672A}\u{547D}\u{540D}\u{4F1A}\u{8BDD}"
}

fn build_session_title(content: &str) -> Option<String> {
    let cleaned = content.trim().replace('\n', " ");
    if cleaned.is_empty() {
        return None;
    }

    let mut output = cleaned;
    if output.chars().count() > 20 {
        output = output.chars().take(20).collect::<String>();
        output.push_str("...");
    }
    Some(output)
}

fn current_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn context_left_percent(used_tokens: i64, max_context: Option<u32>) -> Option<u32> {
    let total = u64::from(max_context?.max(1));
    let used = used_tokens.max(0) as u64;
    let left = total.saturating_sub(used);
    Some(((left as f64 / total as f64) * 100.0).round() as u32)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SessionStatsSnapshot {
    pub context_used_tokens: i64,
    pub context_peak_tokens: i64,
    pub model_calls: u64,
    pub tool_calls: u64,
    pub tool_results: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
}

pub(crate) async fn load_session_stats(
    runtime: &CliRuntime,
    session_id: &str,
) -> SessionStatsSnapshot {
    let storage = runtime.state.storage.clone();
    let owner = runtime.user_id.clone();
    let session_id_for_load = session_id.to_string();
    let mut stats = tokio::task::spawn_blocking(move || -> Result<SessionStatsSnapshot> {
        let mut output = SessionStatsSnapshot::default();
        let turns = storage.list_thread_turns(&owner, &session_id_for_load, None, 500)?;
        for turn in turns {
            let turn_id = turn
                .get("turn_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(detail) =
                storage.get_thread_turn(&owner, &session_id_for_load, turn_id, -1, 500, true)?
            else {
                continue;
            };
            for item in detail
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let payload = item.get("payload").unwrap_or(item);
                let event_name = payload
                    .get("event_type")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("kind").and_then(Value::as_str))
                    .unwrap_or_default();
                match event_name {
                    "context_usage" => {
                        if let Some(tokens) = payload.get("context_tokens").and_then(Value::as_i64)
                        {
                            output.context_used_tokens = tokens.max(0);
                            output.context_peak_tokens =
                                output.context_peak_tokens.max(tokens.max(0));
                        }
                    }
                    "llm_request" | "model_call" => {
                        output.model_calls = output.model_calls.saturating_add(1)
                    }
                    "tool_call" => output.tool_calls = output.tool_calls.saturating_add(1),
                    "tool_result" => output.tool_results = output.tool_results.saturating_add(1),
                    "token_usage" => {
                        output.total_input_tokens = output.total_input_tokens.saturating_add(
                            payload
                                .get("input_tokens")
                                .and_then(Value::as_u64)
                                .unwrap_or(0),
                        );
                        output.total_output_tokens = output.total_output_tokens.saturating_add(
                            payload
                                .get("output_tokens")
                                .and_then(Value::as_u64)
                                .unwrap_or(0),
                        );
                        output.total_tokens = output.total_tokens.saturating_add(
                            payload
                                .get("total_tokens")
                                .and_then(Value::as_u64)
                                .unwrap_or(0),
                        );
                    }
                    _ => {}
                }
            }
        }
        Ok(output)
    })
    .await
    .ok()
    .and_then(Result::ok)
    .unwrap_or_default();
    let workspace_tokens = runtime
        .state
        .workspace
        .load_session_context_tokens_async(&runtime.user_id, session_id)
        .await
        .max(0);
    stats.context_peak_tokens = stats.context_peak_tokens.max(workspace_tokens);
    if stats.context_used_tokens <= 0 {
        stats.context_used_tokens = workspace_tokens;
    }
    stats
}

pub(crate) async fn collect_apps_lines(runtime: &CliRuntime, language: &str) -> Vec<String> {
    let is_zh = locale::is_zh_language(language);
    let payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);

    let mut lines = vec![locale::tr(language, "应用连接概览", "apps").to_string()];
    let mut active_count = 0usize;
    let mut total_count = 0usize;

    if !payload.mcp_servers.is_empty() {
        lines.push(locale::tr(language, "- 用户 MCP", "- user mcp").to_string());
        let mut servers = payload.mcp_servers;
        servers.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        for server in servers {
            if server.name.trim().is_empty() {
                continue;
            }
            total_count = total_count.saturating_add(1);
            if server.enabled {
                active_count = active_count.saturating_add(1);
            }
            let status = if server.enabled {
                locale::tr(language, "启用", "enabled")
            } else {
                locale::tr(language, "禁用", "disabled")
            };
            let endpoint = server.endpoint.trim();
            lines.push(format!(
                "  - {} [{}] {}",
                server.name.trim(),
                status,
                if endpoint.is_empty() { "-" } else { endpoint }
            ));
        }
    }

    if total_count == 0 {
        lines.push(
            locale::tr(
                language,
                "暂无可用应用连接（可先配置 MCP）",
                "no app connectors configured yet (configure MCP first)",
            )
            .to_string(),
        );
        return lines;
    }

    lines.insert(
        1,
        if is_zh {
            format!("- 总计: {total_count}（已启用 {active_count}）")
        } else {
            format!("- total: {total_count} (enabled {active_count})")
        },
    );
    lines
}

fn apps_usage_line(language: &str) -> String {
    locale::tr(
        language,
        "用法: /apps [list|info <name>|connect <name> <endpoint> [transport]|install <name> <endpoint> [transport]|enable <name>|disable <name>|disconnect <name>|auth <name> <bearer-token|token|api-key> <secret>|logout <name>|remove <name>|test <name>]",
        "usage: /apps [list|info <name>|connect <name> <endpoint> [transport]|install <name> <endpoint> [transport]|enable <name>|disable <name>|disconnect <name>|auth <name> <bearer-token|token|api-key> <secret>|logout <name>|remove <name>|test <name>]",
    )
}

fn app_auth_key_from_alias(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "bearer-token" | "bearer_token" | "bearer" => Some("bearer_token"),
        "token" => Some("token"),
        "api-key" | "api_key" | "apikey" => Some("api_key"),
        _ => None,
    }
}

fn find_mcp_server_mut<'a>(
    servers: &'a mut [UserMcpServer],
    name: &str,
) -> Option<&'a mut UserMcpServer> {
    servers
        .iter_mut()
        .find(|server| server.name.trim().eq_ignore_ascii_case(name.trim()))
}

fn find_mcp_server<'a>(servers: &'a [UserMcpServer], name: &str) -> Option<&'a UserMcpServer> {
    servers
        .iter()
        .find(|server| server.name.trim().eq_ignore_ascii_case(name.trim()))
}

fn resolve_mcp_auth_header(server: &UserMcpServer) -> Option<(String, String)> {
    let Value::Object(map) = server.auth.as_ref()? else {
        return None;
    };

    if let Some(value) = map
        .get("bearer_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(("Authorization".to_string(), format!("Bearer {value}")));
    }
    if let Some(value) = map
        .get("token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(("Authorization".to_string(), format!("Bearer {value}")));
    }
    map.get("api_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| ("x-api-key".to_string(), value.to_string()))
}

fn format_user_mcp_info_lines(server: &UserMcpServer, language: &str) -> Vec<String> {
    let is_zh = locale::is_zh_language(language);
    let state = if server.enabled {
        locale::tr(language, "启用", "enabled")
    } else {
        locale::tr(language, "禁用", "disabled")
    };
    let auth = detect_mcp_auth_key(server)
        .map(|key| {
            let label = mcp_auth_key_label(key, is_zh);
            if is_zh {
                format!("已配置 ({label})")
            } else {
                format!("configured ({label})")
            }
        })
        .unwrap_or_else(|| locale::tr(language, "未配置", "not configured"));
    let endpoint = server.endpoint.trim();
    let transport = server.transport.trim();
    let mut lines = Vec::new();
    if is_zh {
        lines.push(format!("应用详情: {}", server.name.trim()));
        lines.push("- 来源: 用户 MCP 连接器".to_string());
        lines.push(format!("- 状态: {state}"));
        lines.push(format!(
            "- endpoint: {}",
            if endpoint.is_empty() { "-" } else { endpoint }
        ));
        lines.push(format!(
            "- transport: {}",
            if transport.is_empty() { "-" } else { transport }
        ));
        lines.push(format!("- 鉴权: {auth}"));
        lines.push(format!("- allow_tools: {}", server.allow_tools.len()));
        lines.push(format!("- shared_tools: {}", server.shared_tools.len()));
        lines.push(format!("- headers: {}", server.headers.len()));
        lines.push(format!("- tool_specs: {}", server.tool_specs.len()));
        if !server.display_name.trim().is_empty() {
            lines.push(format!("- 显示名: {}", server.display_name.trim()));
        }
        if !server.description.trim().is_empty() {
            lines.push(format!("- 描述: {}", server.description.trim()));
        }
    } else {
        lines.push(format!("app info: {}", server.name.trim()));
        lines.push("- source: user mcp connector".to_string());
        lines.push(format!("- state: {state}"));
        lines.push(format!(
            "- endpoint: {}",
            if endpoint.is_empty() { "-" } else { endpoint }
        ));
        lines.push(format!(
            "- transport: {}",
            if transport.is_empty() { "-" } else { transport }
        ));
        lines.push(format!("- auth: {auth}"));
        lines.push(format!("- allow_tools: {}", server.allow_tools.len()));
        lines.push(format!("- shared_tools: {}", server.shared_tools.len()));
        lines.push(format!("- headers: {}", server.headers.len()));
        lines.push(format!("- tool_specs: {}", server.tool_specs.len()));
        if !server.display_name.trim().is_empty() {
            lines.push(format!("- display_name: {}", server.display_name.trim()));
        }
        if !server.description.trim().is_empty() {
            lines.push(format!("- description: {}", server.description.trim()));
        }
    }
    lines
}

async fn collect_app_info_lines(runtime: &CliRuntime, language: &str, target: &str) -> Vec<String> {
    let is_zh = locale::is_zh_language(language);
    let name = target.trim();
    if name.is_empty() {
        return vec![
            if is_zh {
                "[错误] 应用名称不能为空".to_string()
            } else {
                "[error] app name is required".to_string()
            },
            apps_usage_line(language),
        ];
    }

    let payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    if let Some(server) = find_mcp_server(&payload.mcp_servers, name) {
        return format_user_mcp_info_lines(server, language);
    }

    vec![if is_zh {
        format!("未找到应用连接器: {name}")
    } else {
        format!("app connector not found: {name}")
    }]
}

pub(crate) async fn execute_apps_command(
    runtime: &CliRuntime,
    language: &str,
    args: &str,
) -> Result<Vec<String>> {
    let is_zh = locale::is_zh_language(language);
    let cleaned = args.trim();
    if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("list") {
        return Ok(collect_apps_lines(runtime, language).await);
    }
    if cleaned.eq_ignore_ascii_case("help") {
        return Ok(vec![apps_usage_line(language)]);
    }

    let values = match shell_words::split(cleaned) {
        Ok(items) if !items.is_empty() => items,
        Ok(_) => return Ok(collect_apps_lines(runtime, language).await),
        Err(err) => {
            return Ok(vec![
                if is_zh {
                    format!("[错误] 解析 /apps 参数失败: {err}")
                } else {
                    format!("[error] parse /apps args failed: {err}")
                },
                apps_usage_line(language),
            ]);
        }
    };

    let action = values[0].trim().to_ascii_lowercase();
    match action.as_str() {
        "list" => Ok(collect_apps_lines(runtime, language).await),
        "info" => {
            if values.len() != 2 {
                return Ok(vec![
                    if is_zh {
                        "[错误] /apps info 参数数量不正确".to_string()
                    } else {
                        "[error] invalid /apps info arguments".to_string()
                    },
                    apps_usage_line(language),
                ]);
            }
            Ok(collect_app_info_lines(runtime, language, values[1].trim()).await)
        }
        "connect" | "install" => {
            if values.len() < 3 || values.len() > 4 {
                return Ok(vec![
                    if is_zh {
                        "[错误] /apps connect|install 参数数量不正确".to_string()
                    } else {
                        "[error] invalid /apps connect|install arguments".to_string()
                    },
                    apps_usage_line(language),
                ]);
            }
            let name = values[1].trim();
            let endpoint = values[2].trim();
            let transport = values
                .get(3)
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .unwrap_or("streamable-http");
            if name.is_empty() || endpoint.is_empty() {
                return Ok(vec![
                    if is_zh {
                        "[错误] 名称或 endpoint 不能为空".to_string()
                    } else {
                        "[error] name and endpoint are required".to_string()
                    },
                    apps_usage_line(language),
                ]);
            }

            let mut payload = runtime
                .state
                .user_tool_store
                .load_user_tools(&runtime.user_id);
            let mut created = true;
            if let Some(server) = find_mcp_server_mut(&mut payload.mcp_servers, name) {
                server.endpoint = endpoint.to_string();
                server.transport = transport.to_string();
                server.enabled = true;
                created = false;
            } else {
                payload.mcp_servers.push(UserMcpServer {
                    name: name.to_string(),
                    endpoint: endpoint.to_string(),
                    allow_tools: Vec::new(),
                    packaged: false,
                    shared_tools: Vec::new(),
                    enabled: true,
                    transport: transport.to_string(),
                    description: String::new(),
                    display_name: String::new(),
                    headers: Default::default(),
                    auth: None,
                    tool_specs: Vec::new(),
                });
            }
            runtime
                .state
                .user_tool_store
                .update_mcp_servers(&runtime.user_id, payload.mcp_servers)?;
            let verb = if action == "install" {
                locale::tr(language, "安装", "installed")
            } else {
                locale::tr(language, "连接", "connected")
            };
            Ok(vec![
                if is_zh {
                    if created {
                        format!("应用已{verb}: {name}")
                    } else {
                        format!("应用已更新并启用: {name}")
                    }
                } else if created {
                    format!("app {verb}: {name}")
                } else {
                    format!("app updated and enabled: {name}")
                },
                format!(
                    "{} {endpoint}",
                    locale::tr(language, "endpoint:", "endpoint:")
                ),
                format!(
                    "{} {transport}",
                    locale::tr(language, "transport:", "transport:")
                ),
            ])
        }
        "enable" | "disable" | "disconnect" => {
            if values.len() != 2 {
                return Ok(vec![
                    if is_zh {
                        "[错误] /apps enable|disable|disconnect 参数数量不正确".to_string()
                    } else {
                        "[error] invalid /apps enable|disable|disconnect arguments".to_string()
                    },
                    apps_usage_line(language),
                ]);
            }
            let target = values[1].trim();
            if target.is_empty() {
                return Ok(vec![
                    if is_zh {
                        "[错误] 应用名称不能为空".to_string()
                    } else {
                        "[error] app name is required".to_string()
                    },
                    apps_usage_line(language),
                ]);
            }
            let enabled = action == "enable";
            let mut payload = runtime
                .state
                .user_tool_store
                .load_user_tools(&runtime.user_id);
            let Some(server) = find_mcp_server_mut(&mut payload.mcp_servers, target) else {
                return Ok(vec![if is_zh {
                    format!("未找到应用连接器: {target}")
                } else {
                    format!("app connector not found: {target}")
                }]);
            };
            server.enabled = enabled;
            runtime
                .state
                .user_tool_store
                .update_mcp_servers(&runtime.user_id, payload.mcp_servers)?;
            let state = if enabled {
                locale::tr(language, "启用", "enabled")
            } else {
                locale::tr(language, "禁用", "disabled")
            };
            Ok(vec![if is_zh {
                format!("应用已{state}: {target}")
            } else {
                format!("app {state}: {target}")
            }])
        }
        "remove" => {
            if values.len() != 2 {
                return Ok(vec![
                    if is_zh {
                        "[错误] /apps remove 参数数量不正确".to_string()
                    } else {
                        "[error] invalid /apps remove arguments".to_string()
                    },
                    apps_usage_line(language),
                ]);
            }
            let target = values[1].trim();
            let mut payload = runtime
                .state
                .user_tool_store
                .load_user_tools(&runtime.user_id);
            let before = payload.mcp_servers.len();
            payload
                .mcp_servers
                .retain(|server| !server.name.trim().eq_ignore_ascii_case(target));
            if before == payload.mcp_servers.len() {
                return Ok(vec![if is_zh {
                    format!("未找到应用连接器: {target}")
                } else {
                    format!("app connector not found: {target}")
                }]);
            }
            runtime
                .state
                .user_tool_store
                .update_mcp_servers(&runtime.user_id, payload.mcp_servers)?;
            Ok(vec![if is_zh {
                format!("应用已移除: {target}")
            } else {
                format!("app removed: {target}")
            }])
        }
        "auth" => {
            if values.len() != 4 {
                return Ok(vec![
                    if is_zh {
                        "[错误] /apps auth 参数数量不正确".to_string()
                    } else {
                        "[error] invalid /apps auth arguments".to_string()
                    },
                    apps_usage_line(language),
                ]);
            }
            let target = values[1].trim();
            let Some(auth_key) = app_auth_key_from_alias(values[2].as_str()) else {
                return Ok(vec![
                    if is_zh {
                        format!(
                            "[错误] 非法鉴权类型: {}（支持 bearer-token/token/api-key）",
                            values[2].trim()
                        )
                    } else {
                        format!(
                            "[error] invalid auth kind: {} (expected bearer-token/token/api-key)",
                            values[2].trim()
                        )
                    },
                    apps_usage_line(language),
                ]);
            };
            let secret = values[3].trim();
            if target.is_empty() || secret.is_empty() {
                return Ok(vec![
                    if is_zh {
                        "[错误] 应用名称和鉴权值不能为空".to_string()
                    } else {
                        "[error] app name and secret are required".to_string()
                    },
                    apps_usage_line(language),
                ]);
            }
            let mut payload = runtime
                .state
                .user_tool_store
                .load_user_tools(&runtime.user_id);
            let Some(server) = find_mcp_server_mut(&mut payload.mcp_servers, target) else {
                return Ok(vec![if is_zh {
                    format!("未找到应用连接器: {target}")
                } else {
                    format!("app connector not found: {target}")
                }]);
            };
            server.auth = Some(json!({ auth_key: secret }));
            runtime
                .state
                .user_tool_store
                .update_mcp_servers(&runtime.user_id, payload.mcp_servers)?;
            let auth_label = mcp_auth_key_label(auth_key, is_zh);
            Ok(vec![if is_zh {
                format!("应用鉴权已更新: {target} ({auth_label})")
            } else {
                format!("app auth updated: {target} ({auth_label})")
            }])
        }
        "logout" => {
            if values.len() != 2 {
                return Ok(vec![
                    if is_zh {
                        "[错误] /apps logout 参数数量不正确".to_string()
                    } else {
                        "[error] invalid /apps logout arguments".to_string()
                    },
                    apps_usage_line(language),
                ]);
            }
            let target = values[1].trim();
            let mut payload = runtime
                .state
                .user_tool_store
                .load_user_tools(&runtime.user_id);
            let Some(server) = find_mcp_server_mut(&mut payload.mcp_servers, target) else {
                return Ok(vec![if is_zh {
                    format!("未找到应用连接器: {target}")
                } else {
                    format!("app connector not found: {target}")
                }]);
            };
            server.auth = None;
            runtime
                .state
                .user_tool_store
                .update_mcp_servers(&runtime.user_id, payload.mcp_servers)?;
            Ok(vec![if is_zh {
                format!("应用鉴权已清除: {target}")
            } else {
                format!("app auth cleared: {target}")
            }])
        }
        "test" => {
            if values.len() != 2 {
                return Ok(vec![
                    if is_zh {
                        "[错误] /apps test 参数数量不正确".to_string()
                    } else {
                        "[error] invalid /apps test arguments".to_string()
                    },
                    apps_usage_line(language),
                ]);
            }
            let target = values[1].trim();
            let payload = runtime
                .state
                .user_tool_store
                .load_user_tools(&runtime.user_id);
            let Some(server) = payload
                .mcp_servers
                .into_iter()
                .find(|server| server.name.trim().eq_ignore_ascii_case(target))
            else {
                return Ok(vec![if is_zh {
                    format!("未找到应用连接器: {target}")
                } else {
                    format!("app connector not found: {target}")
                }]);
            };
            if server.endpoint.trim().is_empty() {
                return Ok(vec![if is_zh {
                    format!("应用 endpoint 为空: {target}")
                } else {
                    format!("app endpoint is empty: {target}")
                }]);
            }

            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(6))
                .build()?;
            let auth_header = resolve_mcp_auth_header(&server);
            let transport = server.transport.trim().to_ascii_lowercase();

            let mut request = client.get(server.endpoint.trim());
            if let Some((name, value)) = auth_header.as_ref() {
                request = request.header(name, value);
            }
            match request.send().await {
                Ok(response) => {
                    let code = response.status();
                    if code == reqwest::StatusCode::METHOD_NOT_ALLOWED
                        && (transport.contains("streamable")
                            || transport.contains("http")
                            || transport.is_empty())
                    {
                        let mut post = client
                            .post(server.endpoint.trim())
                            .header("content-type", "application/json")
                            .body(r#"{"jsonrpc":"2.0","id":"health","method":"ping","params":{}}"#);
                        if let Some((name, value)) = auth_header.as_ref() {
                            post = post.header(name, value);
                        }
                        match post.send().await {
                            Ok(post_response) => {
                                let post_code = post_response.status();
                                Ok(vec![
                                    if post_code.is_success() {
                                        if is_zh {
                                            format!(
                                                "应用连通性测试通过: {target} ({post_code}, probe=GET->POST)"
                                            )
                                        } else {
                                            format!(
                                                "app connectivity ok: {target} ({post_code}, probe=GET->POST)"
                                            )
                                        }
                                    } else if is_zh {
                                        format!("应用可达（GET=405，POST={post_code}）: {target}")
                                    } else {
                                        format!(
                                            "app reachable (GET=405, POST={post_code}): {target}"
                                        )
                                    },
                                    format!(
                                        "{} {}",
                                        locale::tr(language, "endpoint:", "endpoint:"),
                                        server.endpoint
                                    ),
                                    format!(
                                        "{} {}",
                                        locale::tr(language, "transport:", "transport:"),
                                        if transport.is_empty() {
                                            "-"
                                        } else {
                                            &transport
                                        }
                                    ),
                                ])
                            }
                            Err(post_err) => Ok(vec![
                                if is_zh {
                                    format!(
                                        "[错误] 应用连通性测试失败: {target} (GET=405, POST error: {post_err})"
                                    )
                                } else {
                                    format!(
                                        "[error] app connectivity failed: {target} (GET=405, POST error: {post_err})"
                                    )
                                },
                                format!(
                                    "{} {}",
                                    locale::tr(language, "endpoint:", "endpoint:"),
                                    server.endpoint
                                ),
                            ]),
                        }
                    } else {
                        Ok(vec![
                            if code.is_success() {
                                if is_zh {
                                    format!("应用连通性测试通过: {target} ({code})")
                                } else {
                                    format!("app connectivity ok: {target} ({code})")
                                }
                            } else if is_zh {
                                format!("应用连通性可达但返回非 2xx: {target} ({code})")
                            } else {
                                format!("app reachable but returned non-2xx: {target} ({code})")
                            },
                            format!(
                                "{} {}",
                                locale::tr(language, "endpoint:", "endpoint:"),
                                server.endpoint
                            ),
                            format!(
                                "{} {}",
                                locale::tr(language, "transport:", "transport:"),
                                if transport.is_empty() {
                                    "-"
                                } else {
                                    &transport
                                }
                            ),
                        ])
                    }
                }
                Err(err) => Ok(vec![
                    if is_zh {
                        format!("[错误] 应用连通性测试失败: {target} ({err})")
                    } else {
                        format!("[error] app connectivity failed: {target} ({err})")
                    },
                    format!(
                        "{} {}",
                        locale::tr(language, "endpoint:", "endpoint:"),
                        server.endpoint
                    ),
                    format!(
                        "{} {}",
                        locale::tr(language, "transport:", "transport:"),
                        if transport.is_empty() {
                            "-"
                        } else {
                            &transport
                        }
                    ),
                ]),
            }
        }
        _ => Ok(vec![
            if is_zh {
                format!("[错误] 无效的 /apps 子命令: {}", values[0].trim())
            } else {
                format!("[error] invalid /apps subcommand: {}", values[0].trim())
            },
            apps_usage_line(language),
        ]),
    }
}

pub(crate) async fn rename_session_title(
    runtime: &CliRuntime,
    session_id: &str,
    new_title: &str,
) -> Result<String> {
    let cleaned_session = session_id.trim().to_string();
    let cleaned_title = new_title.trim().to_string();
    if cleaned_session.is_empty() {
        return Err(anyhow!("session id is empty"));
    }
    if cleaned_title.is_empty() {
        return Err(anyhow!("session title is empty"));
    }

    let user_store = runtime.state.user_store.clone();
    let user_id = runtime.user_id.clone();
    let session = cleaned_session.clone();
    let title = cleaned_title.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        user_store.update_chat_session_title(&user_id, &session, &title, current_ts())?;
        Ok(())
    })
    .await
    .map_err(|err| anyhow!("rename session cancelled: {err}"))??;
    Ok(cleaned_title)
}

fn build_compact_summary_from_history(history: &[Value], language: &str) -> String {
    let is_zh = locale::is_zh_language(language);
    let mut lines = Vec::new();
    let mut picked = 0usize;
    for record in history.iter().rev() {
        if picked >= 10 {
            break;
        }
        let role = record.get("role").and_then(Value::as_str).unwrap_or("");
        if role != "user" && role != "assistant" {
            continue;
        }
        let content = history_value_to_text(record.get("content"));
        let cleaned = content.trim();
        if cleaned.is_empty() {
            continue;
        }
        let mut preview = cleaned.to_string();
        if preview.chars().count() > 140 {
            preview = preview.chars().take(140).collect::<String>();
            preview.push_str("...");
        }
        let label = if is_zh {
            if role == "user" {
                "用户"
            } else {
                "助手"
            }
        } else if role == "user" {
            "user"
        } else {
            "assistant"
        };
        lines.push(format!("- {label}: {preview}"));
        picked = picked.saturating_add(1);
    }
    lines.reverse();
    if lines.is_empty() {
        return locale::tr(
            language,
            "未找到可压缩的历史消息。",
            "no eligible history entries found for compaction.",
        );
    }
    if is_zh {
        format!("会话压缩摘要（最近关键信息）：\n{}", lines.join("\n"))
    } else {
        format!(
            "session compaction summary (recent key context):\n{}",
            lines.join("\n")
        )
    }
}

fn history_value_to_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text.clone()),
                Value::Object(map) => map
                    .get("text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .or_else(|| {
                        map.get("content")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    }),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

pub(crate) async fn compact_session_into_branch(
    runtime: &CliRuntime,
    source_session_id: &str,
    language: &str,
) -> Result<(String, String)> {
    let source = source_session_id.trim().to_string();
    if source.is_empty() {
        return Err(anyhow!("session id is empty"));
    }
    let history = load_session_history_entries(runtime, source.as_str(), 0).await?;
    let summary = build_compact_summary_from_history(&history, language);
    let user_store = runtime.state.user_store.clone();
    let user_id = runtime.user_id.clone();
    let new_session_id = uuid::Uuid::new_v4().simple().to_string();
    let source_for_record = source.clone();
    let new_session_for_record = new_session_id.clone();
    let title = if locale::is_zh_language(language) {
        "压缩会话".to_string()
    } else {
        "compact session".to_string()
    };

    tokio::task::spawn_blocking(move || -> Result<()> {
        let now = current_ts();
        let source_record = user_store.get_chat_session(&user_id, &source_for_record)?;
        let record = ChatSessionRecord {
            session_id: new_session_for_record.clone(),
            user_id: user_id.clone(),
            title,
            status: CLI_DEFAULT_SESSION_STATUS.to_string(),
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: source_record.and_then(|record| record.agent_id),
            workspace_id: None,
            tool_overrides: Vec::new(),
            parent_session_id: Some(source_for_record.clone()),
            parent_message_id: None,
            spawn_label: Some("compact".to_string()),
            spawned_by: Some("wunder-cli".to_string()),
        };
        user_store.upsert_chat_session(&record)?;
        Ok(())
    })
    .await
    .map_err(|err| anyhow!("compact session metadata cancelled: {err}"))??;

    let compact_payload = json!({
        "session_id": new_session_id,
        "role": "assistant",
        "content": summary.clone(),
        "timestamp": format_session_time(current_ts()),
        "meta": {
            "kind": "compaction_summary",
            "source_session_id": source,
        }
    });
    runtime
        .state
        .workspace
        .append_chat(&runtime.user_id, &compact_payload)?;
    runtime
        .state
        .workspace
        .save_session_context_tokens_async(&runtime.user_id, &new_session_id, 0)
        .await;
    Ok((new_session_id, summary))
}

fn build_plan_prompt_with_language(language: &str, args: &str) -> String {
    let topic = args.trim();
    if topic.is_empty() {
        return locale::tr(
            language,
            "请先给出一个可执行计划（编号列表），再等待我确认，不要直接执行改动。",
            "Please provide an executable plan first (numbered list), then wait for my confirmation before making changes.",
        );
    }
    if locale::is_zh_language(language) {
        format!("请先围绕以下目标给出可执行计划（编号列表），待我确认后再执行：{topic}")
    } else {
        format!(
            "Please provide an executable plan first (numbered list) for this goal, then wait for confirmation: {topic}"
        )
    }
}

/// Template written by `/init`. The file is snapshotted into a thread's system
/// prompt when that thread receives its first message, so the wording tells the
/// user when edits take effect.
fn init_agents_template_text(language: &str) -> String {
    if locale::is_zh_language(language) {
        return r#"# AGENTS.md

> 本文件在工作区内的新线程收到第一条消息时被快照进该线程的系统提示词；
> 之后修改只对新建线程生效，不会改写已有线程。

## 项目约定

- 先说明计划，再执行改动；高风险操作必须二次确认。
- 优先改动最小范围，保持兼容与可回滚。
- 每次改动后运行对应检查（如 format/check/test）。
- 输出要简洁，给出可验证结果与后续建议。
"#
        .to_string();
    }
    r#"# AGENTS.md

> This file is snapshotted into a thread's system prompt when the thread
> receives its first message inside this workspace. Later edits apply to new
> threads only; an existing thread keeps the snapshot it started with.

## Project Rules

- Explain the plan before edits; require confirmation for high-risk actions.
- Prefer minimal, reversible changes and preserve compatibility.
- Run relevant validation after edits (format/check/test).
- Keep outputs concise with clear verification and next steps.
"#
    .to_string()
}

async fn prepare_global_pending_attachments(
    runtime: &CliRuntime,
    global: &GlobalArgs,
) -> Result<Vec<attachments::PreparedAttachment>> {
    let mut output = Vec::new();
    for raw_path in &global.attachments {
        let prepared =
            attachments::prepare_attachment_from_path(runtime, raw_path.as_str()).await?;
        output.push(prepared);
    }
    Ok(output)
}

async fn prepare_global_attachment_payloads(
    runtime: &CliRuntime,
    global: &GlobalArgs,
) -> Result<Option<Vec<AttachmentPayload>>> {
    let prepared = prepare_global_pending_attachments(runtime, global).await?;
    Ok(attachments::to_request_attachments(&prepared))
}

pub(crate) fn open_external_editor(runtime: &CliRuntime, seed: Option<&str>) -> Result<String> {
    let editor = std::env::var("VISUAL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| {
            if cfg!(target_os = "windows") {
                "notepad".to_string()
            } else {
                "vi".to_string()
            }
        });

    let mut parts = shell_words::split(editor.as_str())
        .with_context(|| format!("parse editor command failed: {editor}"))?;
    if parts.is_empty() {
        return Err(anyhow!("editor command is empty"));
    }
    let program = parts.remove(0);

    let dir = runtime.temp_root.join("editor");
    fs::create_dir_all(&dir)?;
    let draft_path = dir.join(format!("draft_{}.md", uuid::Uuid::new_v4().simple()));
    fs::write(
        &draft_path,
        seed.unwrap_or_default().replace("\r\n", "\n").as_bytes(),
    )?;

    let status = std::process::Command::new(program.as_str())
        .args(parts)
        .arg(draft_path.as_os_str())
        .status()
        .with_context(|| format!("launch editor failed: {editor}"))?;
    if !status.success() {
        return Err(anyhow!("editor exited with status: {status}"));
    }

    let text = fs::read_to_string(&draft_path)
        .with_context(|| format!("read editor draft failed: {}", draft_path.to_string_lossy()))?;
    if let Err(err) = fs::remove_file(&draft_path) {
        tracing::debug!(
            "remove editor draft failed: {}, {}",
            draft_path.display(),
            err
        );
    }
    Ok(text)
}

pub(crate) fn describe_turn_notification(
    config: &TurnNotificationConfig,
    language: &str,
) -> String {
    match config {
        TurnNotificationConfig::Off => locale::tr(language, "关闭", "off"),
        TurnNotificationConfig::Bell { when } => format!(
            "{}{}",
            locale::tr(language, "BEL 铃声", "BEL"),
            describe_notification_when_suffix(when, language)
        ),
        TurnNotificationConfig::Osc9 { when } => format!(
            "{}{}",
            locale::tr(language, "OSC9 终端通知", "OSC9"),
            describe_notification_when_suffix(when, language)
        ),
        TurnNotificationConfig::Command { argv, when } => {
            let rendered = argv.join(" ");
            if rendered.trim().is_empty() {
                locale::tr(language, "自定义命令(空)", "command(empty)")
            } else if locale::is_zh_language(language) {
                format!(
                    "命令: {rendered}{}",
                    describe_notification_when_suffix(when, language)
                )
            } else {
                format!(
                    "command: {rendered}{}",
                    describe_notification_when_suffix(when, language)
                )
            }
        }
    }
}

pub(crate) fn serialize_turn_notification(config: &TurnNotificationConfig) -> Value {
    match config {
        TurnNotificationConfig::Off => json!({ "type": "off" }),
        TurnNotificationConfig::Bell { when } => json!({
            "type": "bell",
            "when": when_to_str(when),
        }),
        TurnNotificationConfig::Osc9 { when } => json!({
            "type": "osc9",
            "when": when_to_str(when),
        }),
        TurnNotificationConfig::Command { argv, when } => json!({
            "type": "command",
            "argv": argv,
            "when": when_to_str(when),
        }),
    }
}

fn when_to_str(when: &TurnNotificationWhen) -> &'static str {
    match when {
        TurnNotificationWhen::Always => "always",
        TurnNotificationWhen::Unfocused => "unfocused",
    }
}

fn describe_notification_when_suffix(when: &TurnNotificationWhen, language: &str) -> String {
    match when {
        TurnNotificationWhen::Always => String::new(),
        TurnNotificationWhen::Unfocused => {
            if locale::is_zh_language(language) {
                "（仅失焦）".to_string()
            } else {
                " (unfocused only)".to_string()
            }
        }
    }
}

fn notification_when(config: &TurnNotificationConfig) -> TurnNotificationWhen {
    match config {
        TurnNotificationConfig::Off => TurnNotificationWhen::Always,
        TurnNotificationConfig::Bell { when } => when.clone(),
        TurnNotificationConfig::Osc9 { when } => when.clone(),
        TurnNotificationConfig::Command { when, .. } => when.clone(),
    }
}

pub(crate) fn parse_approval_mode(raw: &str) -> Option<ApprovalModeArg> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "never" | "full_auto" | "full-auto" | "full" => Some(ApprovalModeArg::Never),
        "on-request" | "on_request" | "auto_edit" | "auto-edit" | "auto" => {
            Some(ApprovalModeArg::OnRequest)
        }
        "suggest" | "suggested" | "untrusted" => Some(ApprovalModeArg::Suggest),
        _ => None,
    }
}

fn sorted_model_names(config: &Config) -> Vec<String> {
    let mut names: Vec<String> = config.llm.models.keys().cloned().collect();
    names.sort();
    names
}

fn resolve_effective_approval_mode(
    config: &Config,
    override_mode: Option<ApprovalModeArg>,
) -> String {
    if let Some(mode) = override_mode {
        return mode.as_str().to_string();
    }
    config
        .security
        .approval_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("full_auto")
        .to_string()
}

async fn handle_tool(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: ToolCommand,
) -> Result<()> {
    match command.command {
        ToolSubcommand::Run(run) => handle_tool_run(runtime, global, run).await,
        ToolSubcommand::List => handle_tool_list(runtime).await,
    }
}

async fn handle_tool_run(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: ToolRunCommand,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let args: Value = serde_json::from_str(command.args.trim()).with_context(|| {
        if locale::is_zh_language(language.as_str()) {
            format!("--args 不是合法 JSON: {}", command.args.trim())
        } else {
            format!("invalid json for --args: {}", command.args.trim())
        }
    })?;
    run_tool_direct(runtime, global, &command.name, args).await
}

async fn handle_tool_list(runtime: &CliRuntime) -> Result<()> {
    let config = runtime.state.config_store.get().await;
    let skills_snapshot = runtime.state.skills.read().await.clone();
    let bindings =
        runtime
            .state
            .user_tool_manager
            .build_bindings(&config, &skills_snapshot, &runtime.user_id);
    let mut names: Vec<String> =
        collect_available_tool_names(&config, &skills_snapshot, Some(&bindings))
            .into_iter()
            .collect();
    names.sort();
    for name in names {
        println!("{name}");
    }
    Ok(())
}

async fn run_tool_direct(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    tool_name: &str,
    args: Value,
) -> Result<()> {
    let session_id = runtime.resolve_session(global.session.as_deref());
    let result = run_tool_outside_turn(runtime, session_id.as_str(), tool_name, args).await?;
    if global.json {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&result)?);
    }
    Ok(())
}

/// Run one tool outside a model turn. `tool run` and the composer's `!` prefix
/// share this path, so a direct command is gated exactly like a tool the model
/// would have called - same roots, same policy, same storage.
pub(crate) async fn run_tool_outside_turn(
    runtime: &CliRuntime,
    session_id: &str,
    tool_name: &str,
    args: Value,
) -> Result<Value> {
    let config = runtime.state.config_store.get().await;
    let skills_snapshot = runtime.state.skills.read().await.clone();
    let bindings =
        runtime
            .state
            .user_tool_manager
            .build_bindings(&config, &skills_snapshot, &runtime.user_id);
    let roots = build_tool_roots(&config, &skills_snapshot, Some(&bindings), &[]);
    let http = reqwest::Client::new();

    let tool_context = ToolContext {
        user_id: &runtime.user_id,
        session_id,
        workspace_id: runtime.workspace_id(),
        agent_id: None,
        user_round: None,
        model_round: None,
        is_admin: false,
        storage: runtime.state.storage.clone(),
        orchestrator: Some(runtime.state.kernel.orchestrator.clone()),
        monitor: Some(runtime.state.monitor.clone()),
        workspace: runtime.state.workspace.clone(),
        lsp_manager: runtime.state.lsp_manager.clone(),
        config: &config,
        skills: &skills_snapshot,
        gateway: Some(runtime.state.control.gateway.clone()),
        user_world: Some(runtime.state.projection.user_world.clone()),
        cron_wake_signal: Some(runtime.state.control.cron.wake_signal()),
        user_tool_manager: Some(runtime.state.user_tool_manager.clone()),
        user_tool_bindings: Some(&bindings),
        user_tool_store: Some(runtime.state.user_tool_manager.store()),
        request_config_overrides: None,
        allow_roots: Some(roots.allow_roots.clone()),
        read_roots: Some(roots.read_roots.clone()),
        command_sessions: Some(runtime.state.control.command_sessions.clone()),
        event_emitter: None,
        http: &http,
    };

    execute_tool(&tool_context, tool_name, &args).await
}

async fn handle_mcp(runtime: &CliRuntime, global: &GlobalArgs, command: McpCommand) -> Result<()> {
    match command.command {
        McpSubcommand::List(cmd) => mcp_list(runtime, global, cmd).await,
        McpSubcommand::Get(cmd) => mcp_get(runtime, global, cmd).await,
        McpSubcommand::Add(cmd) => mcp_add(runtime, global, cmd).await,
        McpSubcommand::Remove(cmd) => mcp_remove(runtime, global, cmd).await,
        McpSubcommand::Enable(cmd) => mcp_toggle(runtime, global, cmd, true).await,
        McpSubcommand::Disable(cmd) => mcp_toggle(runtime, global, cmd, false).await,
        McpSubcommand::Login(cmd) => mcp_login(runtime, global, cmd).await,
        McpSubcommand::Logout(cmd) => mcp_logout(runtime, global, cmd).await,
        McpSubcommand::Test(cmd) => mcp_test(runtime, global, cmd).await,
    }
}

async fn mcp_list(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: McpListCommand,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let mut payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    payload
        .mcp_servers
        .sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    if command.json {
        println!("{}", serde_json::to_string_pretty(&payload.mcp_servers)?);
        return Ok(());
    }
    if payload.mcp_servers.is_empty() {
        println!(
            "{}",
            locale::tr(
                language.as_str(),
                "尚未配置 MCP 服务器。使用 `wunder-cli mcp add` 新增。",
                "No MCP servers configured. Use `wunder-cli mcp add` to add one.",
            )
        );
        return Ok(());
    }
    for server in payload.mcp_servers {
        let state = format_mcp_state(&server, is_zh);
        let auth_state = format_mcp_auth_state(&server, is_zh);
        println!("{} ({state})", server.name);
        println!(
            "{}",
            if is_zh {
                format!("  传输: {}", server.transport)
            } else {
                format!("  transport: {}", server.transport)
            }
        );
        println!(
            "{}",
            if is_zh {
                format!("  地址: {}", server.endpoint)
            } else {
                format!("  endpoint: {}", server.endpoint)
            }
        );
        println!(
            "{}",
            if is_zh {
                format!("  鉴权: {auth_state}")
            } else {
                format!("  auth: {auth_state}")
            }
        );
        if !server.allow_tools.is_empty() {
            println!(
                "{}",
                if is_zh {
                    format!("  允许工具: {}", server.allow_tools.join(", "))
                } else {
                    format!("  allow_tools: {}", server.allow_tools.join(", "))
                }
            );
        }
        println!(
            "{}",
            if is_zh {
                format!("  删除: wunder-cli mcp remove {}", server.name)
            } else {
                format!("  remove: wunder-cli mcp remove {}", server.name)
            }
        );
        if is_zh {
            println!(
                "  登录: wunder-cli mcp login {} --bearer-token <TOKEN>",
                server.name
            );
            println!("  退出: wunder-cli mcp logout {}", server.name);
        } else {
            println!(
                "  login: wunder-cli mcp login {} --bearer-token <TOKEN>",
                server.name
            );
            println!("  logout: wunder-cli mcp logout {}", server.name);
        }
    }
    Ok(())
}

async fn mcp_get(runtime: &CliRuntime, global: &GlobalArgs, command: McpGetCommand) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    let server = payload
        .mcp_servers
        .into_iter()
        .find(|server| server.name.trim() == command.name.trim())
        .ok_or_else(|| {
            anyhow!(if is_zh {
                format!("未找到 MCP 服务器: {}", command.name.trim())
            } else {
                format!("mcp server not found: {}", command.name.trim())
            })
        })?;

    if command.json {
        println!("{}", serde_json::to_string_pretty(&server)?);
        return Ok(());
    }

    println!("{}", server.name);
    println!(
        "{}",
        if is_zh {
            format!("  状态: {}", format_mcp_state(&server, true))
        } else {
            format!("  status: {}", format_mcp_state(&server, false))
        }
    );
    println!(
        "{}",
        if is_zh {
            format!("  传输: {}", server.transport)
        } else {
            format!("  transport: {}", server.transport)
        }
    );
    println!(
        "{}",
        if is_zh {
            format!("  地址: {}", server.endpoint)
        } else {
            format!("  endpoint: {}", server.endpoint)
        }
    );
    println!(
        "{}",
        if is_zh {
            format!("  鉴权: {}", format_mcp_auth_state(&server, true))
        } else {
            format!("  auth: {}", format_mcp_auth_state(&server, false))
        }
    );
    let description = if server.description.trim().is_empty() {
        "-"
    } else {
        server.description.as_str()
    };
    println!(
        "{}",
        if is_zh {
            format!("  描述: {description}")
        } else {
            format!("  description: {description}")
        }
    );
    let display_name = if server.display_name.trim().is_empty() {
        "-"
    } else {
        server.display_name.as_str()
    };
    println!(
        "{}",
        if is_zh {
            format!("  显示名: {display_name}")
        } else {
            format!("  display_name: {display_name}")
        }
    );
    if !server.allow_tools.is_empty() {
        println!(
            "{}",
            if is_zh {
                format!("  允许工具: {}", server.allow_tools.join(", "))
            } else {
                format!("  allow_tools: {}", server.allow_tools.join(", "))
            }
        );
    }
    if !server.shared_tools.is_empty() {
        println!(
            "{}",
            if is_zh {
                format!("  共享工具: {}", server.shared_tools.join(", "))
            } else {
                format!("  shared_tools: {}", server.shared_tools.join(", "))
            }
        );
    }
    println!(
        "{}",
        if is_zh {
            format!("  删除: wunder-cli mcp remove {}", server.name)
        } else {
            format!("  remove: wunder-cli mcp remove {}", server.name)
        }
    );
    if is_zh {
        println!(
            "  登录: wunder-cli mcp login {} --bearer-token <TOKEN>",
            server.name
        );
        println!("  退出: wunder-cli mcp logout {}", server.name);
    } else {
        println!(
            "  login: wunder-cli mcp login {} --bearer-token <TOKEN>",
            server.name
        );
        println!("  logout: wunder-cli mcp logout {}", server.name);
    }
    Ok(())
}

async fn mcp_add(runtime: &CliRuntime, global: &GlobalArgs, command: McpAddCommand) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let mut payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    payload
        .mcp_servers
        .retain(|server| server.name.trim() != command.name.trim());
    payload.mcp_servers.push(UserMcpServer {
        name: command.name.trim().to_string(),
        endpoint: command.endpoint.trim().to_string(),
        allow_tools: normalize_name_list(command.allow_tools),
        packaged: false,
        shared_tools: Vec::new(),
        enabled: command.enabled,
        transport: command.transport.trim().to_string(),
        description: command.description.unwrap_or_default(),
        display_name: command.display_name.unwrap_or_default(),
        headers: Default::default(),
        auth: None,
        tool_specs: Vec::new(),
    });
    runtime
        .state
        .user_tool_store
        .update_mcp_servers(&runtime.user_id, payload.mcp_servers)?;
    if locale::is_zh_language(language.as_str()) {
        println!("已添加 MCP 服务器: {}", command.name.trim());
    } else {
        println!("mcp server added: {}", command.name.trim());
    }
    Ok(())
}

async fn mcp_remove(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: McpNameCommand,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let mut payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    let before = payload.mcp_servers.len();
    payload
        .mcp_servers
        .retain(|server| server.name.trim() != command.name.trim());
    let after = payload.mcp_servers.len();
    runtime
        .state
        .user_tool_store
        .update_mcp_servers(&runtime.user_id, payload.mcp_servers)?;
    if before == after {
        if is_zh {
            println!("未找到 MCP 服务器: {}", command.name.trim());
        } else {
            println!("mcp server not found: {}", command.name.trim());
        }
    } else if is_zh {
        println!("已移除 MCP 服务器: {}", command.name.trim());
    } else {
        println!("mcp server removed: {}", command.name.trim());
    }
    Ok(())
}

async fn mcp_toggle(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: McpNameCommand,
    enabled: bool,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let mut payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    let mut changed = false;
    for server in &mut payload.mcp_servers {
        if server.name.trim() == command.name.trim() {
            server.enabled = enabled;
            changed = true;
        }
    }
    runtime
        .state
        .user_tool_store
        .update_mcp_servers(&runtime.user_id, payload.mcp_servers)?;
    if changed {
        let state = if enabled {
            locale::tr(language.as_str(), "启用", "enabled")
        } else {
            locale::tr(language.as_str(), "禁用", "disabled")
        };
        if is_zh {
            println!("MCP 服务器已{state}: {}", command.name.trim());
        } else {
            println!("mcp server {state}: {}", command.name.trim());
        }
    } else if is_zh {
        println!("未找到 MCP 服务器: {}", command.name.trim());
    } else {
        println!("mcp server not found: {}", command.name.trim());
    }
    Ok(())
}

async fn mcp_login(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: McpLoginCommand,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let auth_payload = resolve_mcp_login_auth(command, language.as_str())?;
    let mut payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    let mut found = false;
    for server in &mut payload.mcp_servers {
        if server.name.trim() == auth_payload.server_name.as_str() {
            server.auth = Some(json!({
                auth_payload.auth_key: auth_payload.auth_value
            }));
            found = true;
            break;
        }
    }

    if !found {
        if is_zh {
            println!("未找到 MCP 服务器: {}", auth_payload.server_name);
        } else {
            println!("mcp server not found: {}", auth_payload.server_name);
        }
        return Ok(());
    }

    runtime
        .state
        .user_tool_store
        .update_mcp_servers(&runtime.user_id, payload.mcp_servers)?;
    let auth_name = mcp_auth_key_label(auth_payload.auth_key, is_zh);
    if is_zh {
        println!(
            "已更新 MCP 鉴权凭据: {} ({auth_name})",
            auth_payload.server_name
        );
    } else {
        println!(
            "mcp auth updated: {} ({auth_name})",
            auth_payload.server_name
        );
    }
    Ok(())
}

async fn mcp_logout(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: McpNameCommand,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let mut payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    let mut found = false;
    for server in &mut payload.mcp_servers {
        if server.name.trim() == command.name.trim() {
            server.auth = None;
            found = true;
            break;
        }
    }

    if !found {
        if is_zh {
            println!("未找到 MCP 服务器: {}", command.name.trim());
        } else {
            println!("mcp server not found: {}", command.name.trim());
        }
        return Ok(());
    }

    runtime
        .state
        .user_tool_store
        .update_mcp_servers(&runtime.user_id, payload.mcp_servers)?;
    if is_zh {
        println!("已清除 MCP 鉴权凭据: {}", command.name.trim());
    } else {
        println!("mcp auth cleared: {}", command.name.trim());
    }
    Ok(())
}

async fn mcp_test(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: McpNameCommand,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let target = command.name.trim();
    if target.is_empty() {
        return Err(anyhow!(locale::tr(
            language.as_str(),
            "MCP 名称不能为空",
            "mcp name is required",
        )));
    }
    let args = format!("test {target}");
    for line in execute_apps_command(runtime, language.as_str(), args.as_str()).await? {
        println!("{line}");
    }
    Ok(())
}

fn format_mcp_state(server: &UserMcpServer, is_zh: bool) -> &'static str {
    if is_zh {
        if server.enabled {
            "启用"
        } else {
            "禁用"
        }
    } else if server.enabled {
        "enabled"
    } else {
        "disabled"
    }
}

fn format_mcp_auth_state(server: &UserMcpServer, is_zh: bool) -> String {
    if let Some(key) = detect_mcp_auth_key(server) {
        let label = mcp_auth_key_label(key, is_zh);
        if is_zh {
            format!("已登录（{label}）")
        } else {
            format!("logged in ({label})")
        }
    } else if is_zh {
        "未登录".to_string()
    } else {
        "not logged in".to_string()
    }
}

fn detect_mcp_auth_key(server: &UserMcpServer) -> Option<&'static str> {
    let Some(Value::Object(map)) = server.auth.as_ref() else {
        return None;
    };
    ["bearer_token", "token", "api_key"]
        .into_iter()
        .find(|key| {
            map.get(*key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some()
        })
}

fn mcp_auth_key_label(key: &str, is_zh: bool) -> &'static str {
    match key {
        "bearer_token" => {
            if is_zh {
                "Bearer Token"
            } else {
                "bearer token"
            }
        }
        "token" => "token",
        "api_key" => {
            if is_zh {
                "API Key"
            } else {
                "api key"
            }
        }
        _ => {
            if is_zh {
                "未知"
            } else {
                "unknown"
            }
        }
    }
}

#[derive(Debug)]
struct McpLoginAuthPayload {
    server_name: String,
    auth_key: &'static str,
    auth_value: String,
}

fn resolve_mcp_login_auth(command: McpLoginCommand, language: &str) -> Result<McpLoginAuthPayload> {
    let name = command.name.trim().to_string();
    if name.is_empty() {
        return Err(anyhow!(locale::tr(
            language,
            "MCP 服务器名称不能为空",
            "mcp server name is required",
        )));
    }
    let mut candidates = Vec::new();
    if let Some(value) = normalized_secret(command.bearer_token) {
        candidates.push(("bearer_token", value));
    }
    if let Some(value) = normalized_secret(command.token) {
        candidates.push(("token", value));
    }
    if let Some(value) = normalized_secret(command.api_key) {
        candidates.push(("api_key", value));
    }
    if candidates.len() != 1 {
        return Err(anyhow!(locale::tr(
            language,
            "请且仅请提供一种鉴权参数：--bearer-token / --token / --api-key",
            "please provide exactly one auth option: --bearer-token / --token / --api-key",
        )));
    }
    let (auth_key, auth_value) = candidates.remove(0);
    Ok(McpLoginAuthPayload {
        server_name: name,
        auth_key,
        auth_value,
    })
}

fn normalized_secret(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

async fn handle_skills(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: SkillsCommand,
) -> Result<()> {
    match command.command {
        SkillsSubcommand::List(cmd) => skills_list(runtime, global, cmd).await,
        SkillsSubcommand::Enable(cmd) => skills_toggle(runtime, global, cmd, true).await,
        SkillsSubcommand::Disable(cmd) => skills_toggle(runtime, global, cmd, false).await,
        SkillsSubcommand::Upload(cmd) => skills_upload(runtime, global, cmd).await,
        SkillsSubcommand::Remove(cmd) => skills_remove(runtime, global, cmd).await,
        SkillsSubcommand::Root => skills_root(runtime, global),
    }
}

async fn skills_list(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: SkillsListCommand,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    let enabled_set: HashSet<String> = payload.skills.enabled.into_iter().collect();

    let (skill_root, specs) = load_user_skill_specs(runtime).await;
    if command.json {
        let items = specs
            .iter()
            .map(|spec| {
                json!({
                    "name": spec.name,
                    "path": spec.path,
                    "enabled": enabled_set.contains(&spec.name),
                })
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "root": skill_root,
                "count": items.len(),
                "skills": items,
            }))?
        );
        return Ok(());
    }

    if is_zh {
        println!("技能目录: {}", skill_root.to_string_lossy());
    } else {
        println!("skill root: {}", skill_root.to_string_lossy());
    }
    if specs.is_empty() {
        if is_zh {
            println!("在 {} 未找到技能", skill_root.to_string_lossy());
        } else {
            println!("no skills found in {}", skill_root.to_string_lossy());
        }
        return Ok(());
    }
    for spec in specs {
        let enabled = if enabled_set.contains(&spec.name) {
            if is_zh {
                "启用"
            } else {
                "enabled"
            }
        } else if is_zh {
            "禁用"
        } else {
            "disabled"
        };
        println!("{} [{}] {}", spec.name, enabled, spec.path);
    }
    Ok(())
}

async fn skills_toggle(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: SkillNameCommand,
    enable: bool,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let target = command.name.trim().to_string();
    if target.is_empty() {
        if is_zh {
            println!("技能名称不能为空");
        } else {
            println!("skill name cannot be empty");
        }
        return Ok(());
    }

    let (_, specs) = load_user_skill_specs(runtime).await;
    let available: HashSet<String> = specs.into_iter().map(|spec| spec.name).collect();
    if enable && !available.contains(&target) {
        if is_zh {
            println!("未找到技能: {target}");
        } else {
            println!("skill not found: {target}");
        }
        return Ok(());
    }

    let payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    let mut enabled = payload.skills.enabled;
    enabled.retain(|name| name.trim() != target.as_str());
    if enable {
        enabled.push(target.clone());
    }
    let enabled = normalize_name_list(enabled);
    runtime.state.user_tool_store.update_skills(
        &runtime.user_id,
        enabled,
        payload.skills.shared,
    )?;
    runtime
        .state
        .user_tool_manager
        .clear_skill_cache(Some(&runtime.user_id));
    if enable {
        if is_zh {
            println!("技能已启用: {target}");
        } else {
            println!("skill enabled: {target}");
        }
    } else if is_zh {
        println!("技能已禁用: {target}");
    } else {
        println!("skill disabled: {target}");
    }
    Ok(())
}

fn skills_root(runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let root = runtime
        .state
        .user_tool_store
        .get_skill_root(&runtime.user_id);
    if is_zh {
        println!("技能目录: {}", root.to_string_lossy());
    } else {
        println!("skill root: {}", root.to_string_lossy());
    }
    io::stdout().flush()?;
    Ok(())
}

async fn skills_upload(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: SkillsUploadCommand,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let source = resolve_cli_input_path(runtime.launch_dir.as_path(), command.source.as_path());
    if !source.exists() {
        if is_zh {
            println!("上传源不存在: {}", source.to_string_lossy());
        } else {
            println!("upload source not found: {}", source.to_string_lossy());
        }
        return Ok(());
    }

    let (skill_root, before_specs) = load_user_skill_specs(runtime).await;
    fs::create_dir_all(&skill_root)?;
    let before_path_map = collect_skill_path_map(&before_specs);

    let files_written = if source.is_dir()
        || source
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
    {
        import_skill_directory(source.as_path(), skill_root.as_path(), command.replace)?
    } else {
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if extension == "zip" || extension == "skill" {
            extract_skill_archive(source.as_path(), skill_root.as_path(), command.replace)?
        } else {
            if is_zh {
                println!("仅支持 .zip/.skill 或包含 SKILL.md 的目录");
            } else {
                println!(
                    "only .zip/.skill archives or directories containing SKILL.md are supported"
                );
            }
            return Ok(());
        }
    };

    runtime
        .state
        .user_tool_manager
        .clear_skill_cache(Some(&runtime.user_id));

    let (_, after_specs) = load_user_skill_specs(runtime).await;
    let after_path_map = collect_skill_path_map(&after_specs);
    let mut imported_names = after_path_map
        .iter()
        .filter(|(path, _)| !before_path_map.contains_key(*path))
        .map(|(_, name)| name.clone())
        .collect::<Vec<_>>();
    imported_names.sort();
    imported_names.dedup();

    if !imported_names.is_empty() {
        let payload = runtime
            .state
            .user_tool_store
            .load_user_tools(&runtime.user_id);
        let mut enabled = payload.skills.enabled;
        enabled.extend(imported_names.clone());
        let enabled = normalize_name_list(enabled);
        runtime.state.user_tool_store.update_skills(
            &runtime.user_id,
            enabled,
            payload.skills.shared,
        )?;
    }

    if is_zh {
        println!(
            "技能上传完成，写入文件 {files_written} 个，新增技能 {} 个",
            imported_names.len()
        );
    } else {
        println!(
            "skill upload completed, wrote {files_written} files, discovered {} new skills",
            imported_names.len()
        );
    }
    if !imported_names.is_empty() {
        println!("{}", imported_names.join(", "));
    }
    Ok(())
}

async fn skills_remove(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: SkillNameCommand,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let target = command.name.trim();
    if target.is_empty() {
        if is_zh {
            println!("技能名称不能为空");
        } else {
            println!("skill name cannot be empty");
        }
        return Ok(());
    }

    let (skill_root, specs) = load_user_skill_specs(runtime).await;
    let Some(spec) = specs.into_iter().find(|item| item.name == target) else {
        if is_zh {
            println!("未找到技能: {target}");
        } else {
            println!("skill not found: {target}");
        }
        return Ok(());
    };

    let skill_file = PathBuf::from(spec.path);
    let Some(skill_dir) = skill_file.parent() else {
        return Err(anyhow!(
            "invalid skill path: {}",
            skill_file.to_string_lossy()
        ));
    };
    if !is_within_root(skill_root.as_path(), skill_dir) {
        return Err(anyhow!(
            "skill path out of root: {}",
            skill_dir.to_string_lossy()
        ));
    }

    fs::remove_dir_all(skill_dir).with_context(|| {
        format!(
            "remove skill directory failed: {}",
            skill_dir.to_string_lossy()
        )
    })?;
    runtime
        .state
        .user_tool_manager
        .clear_skill_cache(Some(&runtime.user_id));

    let payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    let mut enabled = payload.skills.enabled;
    enabled.retain(|name| name.trim() != target);
    runtime.state.user_tool_store.update_skills(
        &runtime.user_id,
        normalize_name_list(enabled),
        payload.skills.shared,
    )?;

    if is_zh {
        println!("技能已删除: {target}");
    } else {
        println!("skill removed: {target}");
    }
    Ok(())
}

async fn load_user_skill_specs(runtime: &CliRuntime) -> (PathBuf, Vec<SkillSpec>) {
    let config = runtime.state.config_store.get().await;
    let skill_root = runtime
        .state
        .user_tool_store
        .get_skill_root(&runtime.user_id);
    let mut scan_config = config.clone();
    scan_config.skills.paths = vec![skill_root.to_string_lossy().to_string()];
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    let mut specs = registry.list_specs();
    specs.sort_by(|a, b| a.name.cmp(&b.name));
    (skill_root, specs)
}

fn collect_skill_path_map(specs: &[SkillSpec]) -> HashMap<String, String> {
    specs
        .iter()
        .map(|spec| (canonical_skill_path(spec.path.as_str()), spec.name.clone()))
        .collect()
}

fn canonical_skill_path(raw: &str) -> String {
    let path = PathBuf::from(raw);
    let resolved = path.canonicalize().unwrap_or(path);
    resolved.to_string_lossy().to_ascii_lowercase()
}

fn resolve_cli_input_path(base: &Path, source: &Path) -> PathBuf {
    if source.is_absolute() {
        source.to_path_buf()
    } else {
        base.join(source)
    }
}

fn import_skill_directory(source: &Path, skill_root: &Path, replace: bool) -> Result<usize> {
    let source_dir = if source.is_file() {
        let is_skill_markdown = source
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"));
        if !is_skill_markdown {
            return Err(anyhow!("source file must be SKILL.md"));
        }
        source
            .parent()
            .ok_or_else(|| anyhow!("SKILL.md has no parent directory"))?
    } else {
        source
    };
    if !source_dir.join("SKILL.md").is_file() {
        return Err(anyhow!("source directory must contain SKILL.md"));
    }

    let skill_name = source_dir
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow!("cannot infer skill directory name"))?;
    let target_dir = skill_root.join(skill_name);
    if !is_within_root(skill_root, &target_dir) {
        return Err(anyhow!("target skill path out of bounds"));
    }

    let source_norm = source_dir
        .canonicalize()
        .unwrap_or_else(|_| source_dir.to_path_buf());
    let target_norm = target_dir
        .canonicalize()
        .unwrap_or_else(|_| target_dir.clone());
    if source_norm == target_norm {
        return Ok(0);
    }

    if target_dir.exists() {
        if !replace {
            return Err(anyhow!(
                "target skill already exists: {} (use --replace to overwrite)",
                target_dir.to_string_lossy()
            ));
        }
        fs::remove_dir_all(&target_dir)?;
    }
    copy_dir_recursive(source_dir, &target_dir)
}

fn extract_skill_archive(archive_path: &Path, skill_root: &Path, replace: bool) -> Result<usize> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("open archive failed: {}", archive_path.to_string_lossy()))?;
    let mut archive = ZipArchive::new(file).context("invalid zip archive")?;
    let mut files_written = 0usize;

    let mut has_root_files = false;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).context("read zip entry failed")?;
        if entry.is_dir() {
            continue;
        }
        let normalized = entry.name().replace('\\', "/");
        if !normalized.contains('/') {
            has_root_files = true;
            break;
        }
    }

    let package_stem = archive_path
        .file_stem()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("imported_skill");

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).context("read zip entry failed")?;
        if entry.is_dir() {
            continue;
        }
        let mut relative = normalize_archive_entry_path(entry.name())?;
        if has_root_files {
            relative = PathBuf::from(package_stem).join(relative);
        }

        let dest = skill_root.join(&relative);
        if !is_within_root(skill_root, &dest) {
            return Err(anyhow!(
                "zip entry out of skill root: {}",
                relative.to_string_lossy()
            ));
        }
        if dest.exists() && !replace {
            return Err(anyhow!(
                "target file already exists: {} (use --replace to overwrite)",
                dest.to_string_lossy()
            ));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut buffer = Vec::new();
        entry.read_to_end(&mut buffer)?;
        fs::write(&dest, buffer)?;
        files_written = files_written.saturating_add(1);
    }
    Ok(files_written)
}

fn normalize_archive_entry_path(raw: &str) -> Result<PathBuf> {
    let cleaned = raw.replace('\\', "/");
    let trimmed = cleaned.trim_matches('/');
    if trimmed.is_empty() {
        return Err(anyhow!("empty zip entry path"));
    }
    let relative = PathBuf::from(trimmed);
    for component in relative.components() {
        if matches!(
            component,
            std::path::Component::Prefix(_)
                | std::path::Component::RootDir
                | std::path::Component::ParentDir
        ) {
            return Err(anyhow!(
                "zip entry contains illegal path segment: {trimmed}"
            ));
        }
    }
    Ok(relative)
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<usize> {
    let mut files_written = 0usize;
    for entry in walkdir::WalkDir::new(source)
        .into_iter()
        .filter_map(|item| item.ok())
    {
        let path = entry.path();
        let relative = path.strip_prefix(source).unwrap_or(path);
        let dest = target.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dest)?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(path, &dest)?;
        files_written = files_written.saturating_add(1);
    }
    Ok(files_written)
}

async fn handle_config(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: ConfigCommand,
) -> Result<()> {
    match command.command {
        ConfigSubcommand::Show => config_show(runtime, global).await,
        ConfigSubcommand::Path => config_path(runtime),
        ConfigSubcommand::Validate => config_validate(runtime, global),
    }
}

/// The user config file is the only configuration a person edits; the engine
/// YAML next to it is generated and rebuilt on every start.
fn config_path(runtime: &CliRuntime) -> Result<()> {
    let report = &runtime.user_config;
    let payload = json!({
        "user": report.user_path,
        "profile": report.profile_path,
        "project": report.project_path,
        "profile_dir": runtime.wunder_home.join(user_config::PROFILE_DIR_NAME),
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn config_validate(runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    // Re-read with strict mode so validation catches typos the lenient startup
    // path deliberately ignores.
    let report = user_config::load_report(
        runtime.wunder_home.as_path(),
        runtime.workspace_root(),
        global.profile.as_deref(),
        true,
    );
    match report {
        Ok(report) => {
            let layers: Vec<String> = report
                .layer_paths()
                .into_iter()
                .map(|(name, path)| format!("{name}: {}", path.display()))
                .collect();
            if is_zh {
                println!("配置有效（{} 层）", layers.len());
            } else {
                println!("config valid ({} layer(s))", layers.len());
            }
            for line in layers {
                println!("- {line}");
            }
            Ok(())
        }
        Err(err) => {
            if is_zh {
                eprintln!("[错误] 配置无效: {err}");
            } else {
                eprintln!("[error] invalid config: {err}");
            }
            std::process::exit(1);
        }
    }
}

pub(crate) async fn apply_cli_model_config(
    runtime: &CliRuntime,
    base_url: &str,
    api_key: &str,
    model_name: &str,
    manual_max_context: Option<u32>,
    language: &str,
) -> Result<(String, Option<u32>)> {
    let base_url = base_url.trim().to_string();
    let api_key = api_key.trim().to_string();
    let model_name = model_name.trim().to_string();
    if base_url.is_empty() || api_key.is_empty() || model_name.is_empty() {
        return Err(anyhow!(locale::tr(
            language,
            "base_url、api_key 和 model 不能为空",
            "base_url, api_key and model are required",
        )));
    }

    let provider = infer_provider_from_base_url(&base_url);
    let resolved_max_context = resolve_model_max_context_value(
        &provider,
        &base_url,
        &api_key,
        &model_name,
        manual_max_context,
    )
    .await;

    let model_for_update = model_name.clone();
    let provider_for_update = provider.clone();
    let base_url_for_update = base_url.clone();
    let api_key_for_update = api_key.clone();

    runtime
        .state
        .config_store
        .update(move |config| {
            let entry = config
                .llm
                .models
                .entry(model_for_update.clone())
                .or_insert_with(|| {
                    build_cli_llm_model_config(
                        provider_for_update.as_str(),
                        base_url_for_update.as_str(),
                        api_key_for_update.as_str(),
                        model_for_update.as_str(),
                    )
                });
            entry.enable = Some(true);
            entry.provider = Some(provider_for_update.clone());
            entry.base_url = Some(base_url_for_update.clone());
            entry.api_key = Some(api_key_for_update.clone());
            entry.model = Some(model_for_update.clone());
            entry.max_rounds = Some(
                entry
                    .max_rounds
                    .unwrap_or(CLI_MIN_MAX_ROUNDS)
                    .max(CLI_MIN_MAX_ROUNDS),
            );
            if let Some(value) = resolved_max_context {
                entry.max_context = Some(value.max(1));
            }
            if entry
                .model_type
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            {
                entry.model_type = Some("llm".to_string());
            }
            config.llm.default = model_for_update.clone();
        })
        .await?;

    // config.toml is the user-owned source of truth: the engine YAML above is
    // regenerated on every start, so the endpoint must also land in the file
    // the next start projects from.
    crate::user_config::save_user_value(
        runtime.wunder_home.as_path(),
        "model",
        model_name.as_str(),
    )
    .context("save model into config.toml failed")?;
    crate::user_config::save_user_provider(
        runtime.wunder_home.as_path(),
        base_url.as_str(),
        api_key.as_str(),
        resolved_max_context,
    )
    .context("save [provider] into config.toml failed")?;

    Ok((provider, resolved_max_context))
}

async fn config_show(runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    let config = runtime.state.config_store.get().await;
    let model = runtime.resolve_model_name(global.model.as_deref()).await;
    let model_entry = model.as_ref().and_then(|name| config.llm.models.get(name));
    let tool_call_mode = runtime::effective_tool_call_mode(model_entry).to_string();
    let max_rounds = model_entry
        .and_then(|model| model.max_rounds)
        .unwrap_or(CLI_MIN_MAX_ROUNDS)
        .max(CLI_MIN_MAX_ROUNDS);
    let max_context = Some(runtime::effective_max_context(model_entry));
    let approval_mode = resolve_effective_approval_mode(&config, global.approval_mode);
    let approval_mode_source = if global.approval_mode.is_some() {
        "cli"
    } else {
        runtime
            .user_config
            .sources
            .get("approval_policy")
            .map(String::as_str)
            .unwrap_or("default")
    };
    // The user-facing words: `-s/--sandbox` wins over config.toml, and the
    // engine's approval mode is reported back as the policy word it came from.
    let sandbox_mode = global
        .sandbox
        .map(|mode| mode.as_str().to_string())
        .or_else(|| runtime.user_config.values.sandbox_mode.clone())
        .unwrap_or_else(|| "workspace-write".to_string());
    let sandbox_mode_source = if global.sandbox.is_some() {
        "cli"
    } else {
        runtime
            .user_config
            .sources
            .get("sandbox_mode")
            .map(String::as_str)
            .unwrap_or("default")
    };
    let approval_policy =
        crate::args::ApprovalModeArg::from_engine_mode(approval_mode.as_str()).policy_word();
    let session_id = runtime.resolve_session(global.session.as_deref());
    let stats = load_session_stats(runtime, &session_id).await;

    let payload = json!({
        "launch_dir": runtime.launch_dir,
        "temp_root": runtime.temp_root,
        "user_id": runtime.user_id,
        "workspace": {
            "workspace_id": runtime.workspace_id(),
            "name": runtime.workspace.name,
            "root_path": runtime.workspace_root(),
        },
        "storage_backend": config.storage.backend,
        "db_path": config.storage.db_path,
        "project_doc": {
            "enabled": config.project_doc.enabled,
            "max_bytes": config.project_doc.max_bytes,
        },
        "model": model,
        "tool_call_mode": tool_call_mode,
        "sandbox_mode": sandbox_mode,
        "sandbox_mode_source": sandbox_mode_source,
        "approval_policy": approval_policy,
        "approval_mode": approval_mode,
        "approval_mode_source": approval_mode_source,
        "max_rounds": max_rounds,
        "max_context": max_context,
        "context_used": stats.context_used_tokens.max(0),
        "context_left_percent": context_left_percent(stats.context_used_tokens, max_context),
        "config_path": std::env::var("WUNDER_CONFIG_PATH").unwrap_or_default(),
        "user_config": {
            "layers": runtime
                .user_config
                .layer_paths()
                .into_iter()
                .map(|(name, path)| json!({"layer": name, "path": path}))
                .collect::<Vec<_>>(),
            "sources": runtime.user_config.sources,
            "model_reasoning_effort": runtime.user_config.values.model_reasoning_effort,
            "sandbox_mode": runtime.user_config.values.sandbox_mode,
            "notify": runtime.user_config.values.notify,
            "notify_when": runtime.user_config.values.notify_when,
            "language": runtime.user_config.values.language,
        },
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn parse_optional_max_context_value_localized(raw: &str, language: &str) -> Result<Option<u32>> {
    let cleaned = raw.trim();
    if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }
    let value = cleaned.parse::<u32>().map_err(|_| {
        anyhow!(locale::tr(
            language,
            "max_context 必须是正整数",
            "max_context must be a positive integer",
        ))
    })?;
    if value == 0 {
        return Err(anyhow!(locale::tr(
            language,
            "max_context 必须大于 0",
            "max_context must be greater than 0",
        )));
    }
    Ok(Some(value))
}

fn parse_optional_max_context_value(raw: &str) -> Result<Option<u32>> {
    parse_optional_max_context_value_localized(raw, "en-US")
}

pub(crate) async fn resolve_model_max_context_value(
    provider: &str,
    base_url: &str,
    api_key: &str,
    model_name: &str,
    manual_value: Option<u32>,
) -> Option<u32> {
    if let Some(value) = manual_value.filter(|value| *value > 0) {
        return Some(value);
    }
    if !is_openai_compatible_provider(provider) {
        return None;
    }
    probe_openai_context_window(base_url, api_key, model_name, CLI_CONTEXT_PROBE_TIMEOUT_S)
        .await
        .ok()
        .flatten()
}

/// codex-shaped approval words on top of the engine's three modes.
/// `never` runs without prompts and lets the workspace boundary be the guard;
/// `on-request` is the engine's "writes free, execution asks" mode.
pub(crate) fn map_approval_policy(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "never" | "full_auto" | "full-auto" => Some("full_auto"),
        "on-request" | "on_request" => Some("auto_edit"),
        "suggest" => Some("suggest"),
        "auto_edit" | "auto-edit" => Some("auto_edit"),
        _ => None,
    }
}

/// Whether the composer's `!` escape may run a command. That path has no
/// approval channel inside its own flow, so every gated policy refuses instead
/// of silently bypassing the gate the model's tool calls go through.
pub(crate) fn direct_shell_is_allowed(engine_mode: Option<&str>) -> bool {
    ApprovalModeArg::from_engine_mode(engine_mode.unwrap_or("full_auto")) == ApprovalModeArg::Never
}

pub(crate) fn infer_provider_from_base_url(base_url: &str) -> String {
    let normalized = base_url.trim().to_ascii_lowercase();
    if normalized.contains("dashscope.aliyuncs.com") {
        "qwen".to_string()
    } else if normalized.contains("api.openai.com") {
        "openai".to_string()
    } else if normalized.contains("openrouter.ai") {
        "openrouter".to_string()
    } else {
        "openai_compatible".to_string()
    }
}

fn build_cli_llm_model_config(
    provider: &str,
    base_url: &str,
    api_key: &str,
    model_name: &str,
) -> LlmModelConfig {
    LlmModelConfig {
        enable: Some(true),
        provider: Some(provider.to_string()),
        api_mode: None,
        base_url: Some(base_url.to_string()),
        api_key: Some(api_key.to_string()),
        model: Some(model_name.to_string()),
        temperature: None,
        timeout_s: None,
        max_rounds: Some(CLI_MIN_MAX_ROUNDS),
        max_context: None,
        max_output: None,
        thinking_token_budget: None,
        support_vision: None,
        support_hearing: None,
        stream: None,
        stream_include_usage: None,
        history_compaction_ratio: None,
        tool_call_mode: None,
        reasoning_effort: None,
        model_type: Some("llm".to_string()),
        stop: None,
        mock_if_unconfigured: None,
        ..Default::default()
    }
}

async fn handle_doctor(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: DoctorCommand,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let config = runtime.state.config_store.get().await;
    let model = runtime.resolve_model_name(global.model.as_deref()).await;
    let prompts_root = std::env::var("WUNDER_PROMPTS_ROOT").unwrap_or_default();
    let prompts_status_path = if prompts_root.trim().is_empty() {
        "<embedded>".to_string()
    } else {
        prompts_root
    };
    let checks = vec![
        (
            "config",
            std::env::var("WUNDER_CONFIG_PATH").unwrap_or_default(),
            true,
        ),
        (
            "i18n_messages",
            std::env::var("WUNDER_I18N_MESSAGES_PATH").unwrap_or_default(),
            true,
        ),
        ("prompts_root", prompts_status_path, false),
        (
            "skill_runner",
            std::env::var("WUNDER_SKILL_RUNNER_PATH").unwrap_or_default(),
            true,
        ),
    ];

    println!(
        "{}",
        locale::tr(language.as_str(), "wunder-cli 诊断", "wunder-cli doctor")
    );
    println!(
        "{}",
        if is_zh {
            format!("- 启动目录: {}", runtime.launch_dir.to_string_lossy())
        } else {
            format!("- launch_dir: {}", runtime.launch_dir.to_string_lossy())
        }
    );
    println!(
        "{}",
        if is_zh {
            format!("- 临时目录: {}", runtime.temp_root.to_string_lossy())
        } else {
            format!("- temp_root: {}", runtime.temp_root.to_string_lossy())
        }
    );
    println!(
        "{}",
        if is_zh {
            format!("- 项目根目录: {}", runtime.repo_root.to_string_lossy())
        } else {
            format!("- project_root: {}", runtime.repo_root.to_string_lossy())
        }
    );
    println!(
        "{}",
        if is_zh {
            format!("- 用户 ID: {}", runtime.user_id)
        } else {
            format!("- user_id: {}", runtime.user_id)
        }
    );
    println!(
        "{}",
        if is_zh {
            format!("- 工作目录: {}", config.workspace.root)
        } else {
            format!("- workspace_root: {}", config.workspace.root)
        }
    );
    println!(
        "{}",
        if is_zh {
            format!("- 数据库路径: {}", config.storage.db_path)
        } else {
            format!("- db_path: {}", config.storage.db_path)
        }
    );
    println!(
        "{}",
        if is_zh {
            format!("- 模型: {}", model.unwrap_or_else(|| "<none>".to_string()))
        } else {
            format!("- model: {}", model.unwrap_or_else(|| "<none>".to_string()))
        }
    );
    println!(
        "{}",
        if is_zh {
            format!(
                "- 审批模式: {}",
                resolve_effective_approval_mode(&config, global.approval_mode)
            )
        } else {
            format!(
                "- approval_mode: {}",
                resolve_effective_approval_mode(&config, global.approval_mode)
            )
        }
    );
    println!(
        "{}",
        if is_zh {
            format!(
                "- 覆盖配置存在: {}",
                runtime.temp_root.join("config/wunder.yaml").exists()
            )
        } else {
            format!(
                "- config_exists: {}",
                runtime.temp_root.join("config/wunder.yaml").exists()
            )
        }
    );

    for (name, path, should_exist) in checks {
        let exists = if path.trim().is_empty() {
            false
        } else {
            std::path::Path::new(path.as_str()).exists()
        };
        let status = if !should_exist || exists {
            locale::tr(language.as_str(), "正常", "ok")
        } else {
            locale::tr(language.as_str(), "缺失", "missing")
        };
        let check_name = if is_zh {
            match name {
                "config" => "配置文件",
                "i18n_messages" => "i18n 消息文件",
                "prompts_root" => "提示词根目录",
                "skill_runner" => "技能运行器",
                _ => name,
            }
        } else {
            name
        };
        println!("- {check_name}: [{status}] {path}");
    }

    if command.verbose {
        let payload = json!({
            "skills_paths": config.skills.paths,
            "allow_paths": config.security.allow_paths,
            "allow_commands": config.security.allow_commands,
            "approval_mode_config": config.security.approval_mode,
            "approval_mode_effective": resolve_effective_approval_mode(&config, global.approval_mode),
            "exec_policy_mode": config.security.exec_policy_mode,
            "config_path": std::env::var("WUNDER_CONFIG_PATH").unwrap_or_default(),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    }
    Ok(())
}

pub(crate) async fn build_wunder_request(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    prompt: &str,
    session_id: &str,
    attachments: Option<Vec<AttachmentPayload>>,
) -> Result<WunderRequest> {
    let config = runtime.state.config_store.get().await;
    let model_name = runtime.resolve_model_name(global.model.as_deref()).await;
    let language = locale::resolve_cli_language(global);
    let request_overrides = build_request_overrides(
        &config,
        model_name.as_deref(),
        global.tool_call_mode,
        global.approval_mode,
    );

    input_guard::validate_request_text_input_size(
        language.as_str(),
        prompt,
        attachments.as_deref(),
    )?;

    ensure_cli_session_record(runtime, session_id, Some(prompt)).await?;

    Ok(WunderRequest {
        user_id: runtime.user_id.clone(),
        question: prompt.trim().to_string(),
        client_message_id: None,
        tool_names: Vec::new(),
        skip_tool_calls: false,
        stream: !global.no_stream,
        session_id: Some(session_id.to_string()),
        // One built-in agent only: the runtime resolves `__default__` itself.
        agent_id: None,
        workspace_container_id: None,
        // The launch directory is a real workspace; the runtime binds tools to
        // its folder through the registered workspace root.
        workspace_id: Some(runtime.workspace_id().to_string()),
        model_name,
        language: global.language.clone(),
        config_overrides: request_overrides,
        // No per-request prompt override: standing instructions come from
        // AGENTS.md (project snapshot + `$WUNDER_HOME/AGENTS.md`), which the
        // engine freezes into the thread on its first turn.
        agent_prompt: None,
        preview_skill: false,
        attachments,
        allow_queue: true,
        enforce_runtime_queue: true,
        is_admin: false,
        approval_tx: None,
    })
}

fn truncate_preview(text: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let cleaned = text.trim();
    if cleaned.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (index, ch) in cleaned.chars().enumerate() {
        if index >= limit {
            break;
        }
        out.push(ch);
    }
    if cleaned.chars().count() > limit {
        out.push('…');
    }
    out
}

pub(crate) fn emit_turn_complete_notification(
    runtime: &CliRuntime,
    session_id: &str,
    final_event: &FinalEvent,
    source: &str,
    terminal_focused: Option<bool>,
) {
    let config = runtime.load_turn_notification_config();
    if matches!(config, TurnNotificationConfig::Off) {
        return;
    }
    if matches!(notification_when(&config), TurnNotificationWhen::Unfocused)
        && terminal_focused.unwrap_or(true)
    {
        return;
    }

    let summary = truncate_preview(&final_event.answer, 180);
    let payload = json!({
        "type": "agent-turn-complete",
        "source": source,
        "session_id": session_id,
        "user_id": runtime.user_id,
        "cwd": runtime.launch_dir,
        "stop_reason": final_event.stop_reason,
        "answer_preview": summary,
        "ts": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_secs_f64())
            .unwrap_or(0.0),
    });
    let payload_text = serde_json::to_string(&payload).unwrap_or_default();

    match config {
        TurnNotificationConfig::Off => {}
        TurnNotificationConfig::Bell { .. } => {
            eprint!("\u{0007}");
            let _ = io::stderr().flush();
        }
        TurnNotificationConfig::Osc9 { .. } => {
            let message = if summary.trim().is_empty() {
                "wunder-cli turn complete".to_string()
            } else {
                summary
            };
            eprint!("\u{1b}]9;{message}\u{1b}\\");
            let _ = io::stderr().flush();
        }
        TurnNotificationConfig::Command { argv, .. } => {
            if argv.is_empty() {
                return;
            }
            let mut command = std::process::Command::new(&argv[0]);
            if argv.len() > 1 {
                command.args(&argv[1..]);
            }
            command
                .arg(payload_text)
                .env("WUNDER_NOTIFY_EVENT", "agent-turn-complete")
                .env("WUNDER_NOTIFY_SOURCE", source)
                .env("WUNDER_NOTIFY_SESSION_ID", session_id)
                .env("WUNDER_NOTIFY_USER_ID", runtime.user_id.as_str())
                .env(
                    "WUNDER_NOTIFY_CWD",
                    runtime.launch_dir.to_string_lossy().as_ref(),
                );
            let _ = command.spawn();
        }
    }
}

/// One non-interactive agent turn: the answer goes to stdout, everything the
/// agent does on the way there (tool calls, command output, approvals) goes to
/// stderr so a pipeline can consume the reply without filtering the trace.
async fn run_agent_once(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    prompt: &str,
    session_id: &str,
    attachments: Option<Vec<AttachmentPayload>>,
    last_message_file: Option<&Path>,
    color: Option<ColorArg>,
) -> Result<AgentTurnOutcome> {
    let language = locale::resolve_cli_language(global);
    let color = resolve_color(color);
    let mut request =
        build_wunder_request(runtime, global, prompt, session_id, attachments).await?;
    if !should_interactive_approvals(global) && !global.json {
        // A gated policy with no approver is a real trap: the turn proceeds but
        // every gated tool call is refused with an engine message about an
        // "allow list". Say what is actually happening before the first call.
        let config = runtime.state.config_store.get().await;
        let mode = resolve_effective_approval_mode(&config, global.approval_mode);
        if mode != "full_auto" {
            eprintln!(
                "{}",
                locale::tr(
                    language.as_str(),
                    &format!(
                        "[提示] 当前审批模式为 {mode}，非交互运行没有批准入口：免批操作继续，需要批准的调用会被拒绝；如需放开请加 --approval-mode never。"
                    ),
                    &format!(
                        "[hint] approval mode is {mode} with no interactive approver: ungated operations continue, gated calls are refused. Pass --approval-mode never to allow them."
                    ),
                )
            );
        }
    }
    let _approval_task = if should_interactive_approvals(global) {
        let (tx, rx) = new_approval_channel();
        request.approval_tx = Some(tx);
        Some(tokio::spawn(handle_stdio_approvals(rx, language)))
    } else {
        None
    };

    if global.no_stream {
        let response = runtime.state.kernel.orchestrator.run(request).await?;
        let final_event = FinalEvent {
            answer: response.answer.clone(),
            usage: response
                .usage
                .map(|usage| serde_json::to_value(usage).unwrap_or(Value::Null)),
            stop_reason: response.stop_reason,
        };
        if global.json {
            // Same contract as the streaming path, emitted as one short burst.
            exec_events::emit_single_turn(
                session_id,
                final_event.answer.as_str(),
                final_event.usage.clone(),
                final_event.stop_reason.as_deref(),
            );
        } else {
            println!("{}", response.answer);
        }
        write_last_message_file(last_message_file, final_event.answer.as_str())?;
        emit_turn_complete_notification(runtime, session_id, &final_event, "exec", None);
        return Ok(AgentTurnOutcome::completed_round(final_event));
    }

    let mut stream = runtime.state.kernel.orchestrator.stream(request).await?;
    let language = locale::resolve_cli_language(global);
    let mut final_event = FinalEvent::default();
    let mut saw_error = false;
    let mut saw_final = false;

    if global.json {
        // `--json` is the machine contract: one codex-shaped object per line.
        let mut writer = exec_events::ExecEventWriter::new(session_id);
        writer.begin();
        while let Some(item) = stream.next().await {
            let event = item.expect("infallible stream event");
            if let Some(final_payload) = writer.handle(&event) {
                final_event = FinalEvent {
                    answer: final_payload
                        .get("answer")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    usage: final_payload.get("usage").cloned(),
                    stop_reason: final_payload
                        .get("stop_reason")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                };
                saw_final = true;
            }
        }
        saw_error = writer.saw_error();
        let outcome = AgentTurnOutcome {
            final_event,
            saw_final,
            saw_error,
        };
        writer.finish(outcome.failure().as_deref());
        write_last_message_file(last_message_file, outcome.final_event.answer.as_str())?;
        emit_turn_complete_notification(runtime, session_id, &outcome.final_event, "exec", None);
        return Ok(outcome);
    }

    let mut renderer = StreamRenderer::new(false, language.as_str())
        .with_progress_on_stderr()
        .with_color(color);
    while let Some(item) = stream.next().await {
        let event = item.expect("infallible stream event");
        if event.event == "error" {
            saw_error = true;
        }
        if let Some(final_payload) = renderer.render_event(&event)? {
            final_event = final_payload;
            saw_final = true;
        }
    }
    renderer.finish();
    write_last_message_file(last_message_file, final_event.answer.as_str())?;
    emit_turn_complete_notification(runtime, session_id, &final_event, "exec", None);
    Ok(AgentTurnOutcome {
        final_event,
        saw_final,
        saw_error,
    })
}

/// A turn only counts as completed when the stream actually reached its final
/// event: an interrupted or failed round ends without one, and scripts must be
/// able to tell those apart from a short answer.
struct AgentTurnOutcome {
    final_event: FinalEvent,
    saw_final: bool,
    saw_error: bool,
}

impl AgentTurnOutcome {
    fn completed_round(final_event: FinalEvent) -> Self {
        Self {
            final_event,
            saw_final: true,
            saw_error: false,
        }
    }

    /// `None` means success; the message explains a non-zero exit.
    fn failure(&self) -> Option<String> {
        if self.saw_error {
            return Some("the turn reported an error".to_string());
        }
        if !self.saw_final {
            return Some("the turn ended before completing (interrupted or cancelled)".to_string());
        }
        match self.final_event.stop_reason.as_deref() {
            Some("tool_failure_guard") | Some("tool_no_progress_guard") => Some(format!(
                "the turn gave up: {}",
                self.final_event.stop_reason.as_deref().unwrap_or_default()
            )),
            Some("max_rounds") => Some("the turn hit the round limit".to_string()),
            _ => None,
        }
    }
}

/// Script-facing exit policy: 0 when the round completed, 1 otherwise.
fn apply_exec_exit_policy(outcome: &AgentTurnOutcome, language: &str) {
    let Some(reason) = outcome.failure() else {
        return;
    };
    eprintln!(
        "{}",
        locale::tr(
            language,
            &format!("[错误] {reason}"),
            &format!("[error] {reason}"),
        )
    );
    std::process::exit(1);
}

fn write_last_message_file(path: Option<&Path>, answer: &str) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "create output directory failed: {}",
                parent.to_string_lossy()
            )
        })?;
    }
    fs::write(path, answer.as_bytes())
        .with_context(|| format!("write last message failed: {}", path.to_string_lossy()))?;
    Ok(())
}

/// Resolve `--color` for the progress stream. `auto` follows the same rule the
/// rest of the CLI uses: colour only for an interactive stderr, never when the
/// user opted out with NO_COLOR.
fn resolve_color(mode: Option<ColorArg>) -> bool {
    match mode {
        Some(ColorArg::Always) => true,
        Some(ColorArg::Never) => false,
        Some(ColorArg::Auto) | None => {
            if std::env::var_os("NO_COLOR").is_some() {
                return false;
            }
            // Progress is written to stderr in exec mode and to stdout
            // otherwise; either way the decision follows "is a terminal".
            let stream_is_terminal = if io::stderr().is_terminal() {
                true
            } else {
                io::stdout().is_terminal()
            };
            stream_is_terminal
        }
    }
}

fn should_interactive_approvals(global: &GlobalArgs) -> bool {
    if global.json {
        return false;
    }
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

async fn handle_stdio_approvals(mut rx: ApprovalRequestRx, language: String) {
    let is_zh = locale::is_zh_language(language.as_str());
    while let Some(request) = rx.recv().await {
        let summary = compact_approval_prompt_text(request.summary.as_str(), 180, is_zh);
        println!();
        if is_zh {
            println!("[审批] {summary}");
        } else {
            println!("[approval] {summary}");
        }
        if is_zh {
            println!("- 工具: {}", request.tool);
        } else {
            println!("- tool: {}", request.tool);
        }
        let response = loop {
            if is_zh {
                println!("审批选项:");
                println!("  1) 仅本次批准");
                println!("  2) 本会话批准");
                println!("  3) 拒绝");
                println!("请输入 1/2/3（也可用 y/a/n）:");
            } else {
                println!("Approval options:");
                println!("  1) approve once");
                println!("  2) approve for session");
                println!("  3) deny");
                println!("choose 1/2/3 (or y/a/n):");
            }
            io::stdout().flush().ok();

            let choice = tokio::task::spawn_blocking(|| {
                let mut buffer = String::new();
                std::io::stdin().read_line(&mut buffer).ok();
                buffer
            })
            .await
            .ok()
            .unwrap_or_default();

            let parsed = match choice.trim().to_ascii_lowercase().as_str() {
                "y" | "yes" | "1" => Some(ApprovalResponse::ApproveOnce),
                "a" | "always" | "2" => Some(ApprovalResponse::ApproveSession),
                "n" | "no" | "3" => Some(ApprovalResponse::Deny),
                _ => None,
            };
            if let Some(response) = parsed {
                break response;
            }
            if is_zh {
                println!("[提示] 输入无效，请输入 1/2/3（或 y/a/n）。");
            } else {
                println!("[hint] invalid input, please type 1/2/3 (or y/a/n).");
            }
            io::stdout().flush().ok();
        };
        let _ = request.respond_to.send(response);
    }
}

fn compact_approval_prompt_text(text: &str, max_chars: usize, is_zh: bool) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let compact = normalized
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let compact = compact.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_for_stderr(compact, max_chars, is_zh)
}

fn truncate_for_stderr(text: String, max_chars: usize, is_zh: bool) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text;
    }
    let mut out = String::new();
    for ch in text.chars().take(max_chars) {
        out.push(ch);
    }
    if is_zh {
        out.push_str("...(已截断)");
    } else {
        out.push_str("...(truncated)");
    }
    out
}

fn build_request_overrides(
    config: &Config,
    model_name: Option<&str>,
    tool_call_mode: Option<ToolCallModeArg>,
    approval_mode: Option<ApprovalModeArg>,
) -> Option<Value> {
    let selected_model = resolve_selected_model(config, model_name)?;
    let mut root = serde_json::Map::new();
    let mut model_overrides = serde_json::Map::new();

    if let Some(mode) = tool_call_mode {
        model_overrides.insert("tool_call_mode".to_string(), json!(mode.as_str()));
    }

    let max_rounds = config
        .llm
        .models
        .get(&selected_model)
        .and_then(|entry| entry.max_rounds);
    if max_rounds.unwrap_or(0) < CLI_MIN_MAX_ROUNDS {
        model_overrides.insert(
            "max_rounds".to_string(),
            json!(max_rounds
                .unwrap_or(CLI_MIN_MAX_ROUNDS)
                .max(CLI_MIN_MAX_ROUNDS)),
        );
    }

    if model_overrides.is_empty() {
        // noop
    } else {
        root.insert(
            "llm".to_string(),
            json!({
                "models": {
                    selected_model: model_overrides
                }
            }),
        );
    }

    if let Some(mode) = approval_mode {
        root.insert(
            "security".to_string(),
            json!({ "approval_mode": mode.as_str() }),
        );
    }

    if root.is_empty() {
        None
    } else {
        Some(Value::Object(root))
    }
}

fn resolve_selected_model(config: &Config, model_name: Option<&str>) -> Option<String> {
    model_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            if config.llm.default.trim().is_empty() {
                None
            } else {
                Some(config.llm.default.trim().to_string())
            }
        })
        .or_else(|| config.llm.models.keys().next().cloned())
}

fn resolve_prompt_text(prompt: Option<String>, language: &str) -> Result<String> {
    match prompt {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed == "-" {
                read_stdin_all(language)
            } else if trimmed.is_empty() {
                Err(anyhow!(locale::tr(
                    language,
                    "提问内容为空",
                    "prompt is empty",
                )))
            } else {
                Ok(trimmed.to_string())
            }
        }
        None => read_stdin_all(language),
    }
}

fn read_stdin_all(language: &str) -> Result<String> {
    if io::stdin().is_terminal() {
        return Err(anyhow!(locale::tr(
            language,
            "必须提供提问内容",
            "prompt is required",
        )));
    }
    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;
    let text = buffer.trim();
    if text.is_empty() {
        Err(anyhow!(locale::tr(
            language,
            "stdin 为空",
            "stdin is empty",
        )))
    } else {
        Ok(text.to_string())
    }
}

fn normalize_name_list(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for value in values {
        let cleaned = value.trim();
        if cleaned.is_empty() {
            continue;
        }
        if !seen.insert(cleaned.to_string()) {
            continue;
        }
        output.push(cleaned.to_string());
    }
    output
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiffSlashAction {
    Summary,
    Files,
    Show(String),
    Hunks(String),
    Stage(String),
    Unstage(String),
    Revert(String),
}

pub(crate) fn parse_diff_slash_action(args: &str) -> Result<DiffSlashAction> {
    let cleaned = args.trim();
    if cleaned.is_empty()
        || cleaned.eq_ignore_ascii_case("summary")
        || cleaned.eq_ignore_ascii_case("status")
    {
        return Ok(DiffSlashAction::Summary);
    }
    if cleaned.eq_ignore_ascii_case("files") || cleaned.eq_ignore_ascii_case("tree") {
        return Ok(DiffSlashAction::Files);
    }

    let mut parts = cleaned.splitn(2, char::is_whitespace);
    let action = parts.next().unwrap_or_default().to_ascii_lowercase();
    let value = parts.next().unwrap_or_default().trim().to_string();
    if value.is_empty() {
        if action == "show" || action == "file" {
            return Err(anyhow!("usage: /diff show <index|path>"));
        }
        if action == "hunks" || action == "hunk" {
            return Err(anyhow!("usage: /diff hunks <index|path>"));
        }
        if action == "stage" {
            return Err(anyhow!("usage: /diff stage <index|path>"));
        }
        if action == "unstage" {
            return Err(anyhow!("usage: /diff unstage <index|path>"));
        }
        if action == "revert" || action == "discard" {
            return Err(anyhow!("usage: /diff revert <index|path>"));
        }
    }

    match action.as_str() {
        "show" | "file" => Ok(DiffSlashAction::Show(value)),
        "hunks" | "hunk" => Ok(DiffSlashAction::Hunks(value)),
        "stage" => Ok(DiffSlashAction::Stage(value)),
        "unstage" => Ok(DiffSlashAction::Unstage(value)),
        "revert" | "discard" => Ok(DiffSlashAction::Revert(value)),
        _ => Ok(DiffSlashAction::Show(cleaned.to_string())),
    }
}

pub(crate) fn git_changed_files_with_status(
    workspace_root: &std::path::Path,
) -> Result<Vec<(String, String)>> {
    let Some(status_output) = run_git(workspace_root, ["status", "--porcelain"]) else {
        return Err(anyhow!("git status --porcelain failed"));
    };
    let mut rows = Vec::new();
    for row in status_output.lines() {
        if row.trim().is_empty() {
            continue;
        }
        let status = row.chars().take(2).collect::<String>();
        let path = row.get(3..).unwrap_or("").trim();
        if path.is_empty() {
            continue;
        }
        let path = path.replace('\\', "/");
        let path = path
            .split(" -> ")
            .last()
            .map(str::trim)
            .unwrap_or(path.as_str())
            .to_string();
        rows.push((status, path));
    }
    Ok(rows)
}

pub(crate) fn git_changed_files(workspace_root: &std::path::Path) -> Result<Vec<String>> {
    Ok(git_changed_files_with_status(workspace_root)?
        .into_iter()
        .map(|(_, path)| path)
        .collect())
}

pub(crate) fn resolve_diff_target(workspace_root: &std::path::Path, value: &str) -> Result<String> {
    let cleaned = value.trim();
    if cleaned.is_empty() {
        return Err(anyhow!("diff target is empty"));
    }
    let changed = git_changed_files(workspace_root)?;
    if let Ok(index) = cleaned.parse::<usize>() {
        if index == 0 || index > changed.len() {
            return Err(anyhow!("diff index out of range: {index}"));
        }
        return Ok(changed[index - 1].clone());
    }
    if changed
        .iter()
        .any(|path| path.eq_ignore_ascii_case(cleaned))
    {
        return Ok(cleaned.replace('\\', "/"));
    }
    if let Some(path) = changed.iter().find(|path| {
        path.to_ascii_lowercase()
            .contains(cleaned.to_ascii_lowercase().as_str())
    }) {
        return Ok(path.clone());
    }
    Ok(cleaned.replace('\\', "/"))
}

pub(crate) fn diff_file_lines_with_language(
    workspace_root: &std::path::Path,
    target: &str,
    language: &str,
) -> Vec<String> {
    let mut lines = Vec::new();
    let resolved = match resolve_diff_target(workspace_root, target) {
        Ok(path) => path,
        Err(err) => {
            lines.push(locale::tr(language, "文件 diff", "file diff"));
            lines.push(format!("[error] {err}"));
            return lines;
        }
    };
    lines.push(locale::tr(language, "文件 diff", "file diff"));
    lines.push(format!("- target: {resolved}"));
    let output = run_git(workspace_root, ["diff", "--", resolved.as_str()])
        .or_else(|| {
            run_git(
                workspace_root,
                ["diff", "--cached", "--", resolved.as_str()],
            )
        })
        .unwrap_or_default();
    if output.trim().is_empty() {
        lines.push(locale::tr(
            language,
            "- 当前目标没有可显示的 diff",
            "- no diff output for target",
        ));
        return lines;
    }
    lines.extend(
        truncate_chars(output.as_str(), 16_000)
            .lines()
            .map(ToString::to_string),
    );
    lines
}

pub(crate) fn diff_hunk_lines_with_language(
    workspace_root: &std::path::Path,
    target: &str,
    language: &str,
) -> Vec<String> {
    let mut lines = Vec::new();
    let resolved = match resolve_diff_target(workspace_root, target) {
        Ok(path) => path,
        Err(err) => {
            lines.push(locale::tr(language, "diff hunk 列表", "diff hunk list"));
            lines.push(format!("[error] {err}"));
            return lines;
        }
    };
    let diff = run_git(workspace_root, ["diff", "--", resolved.as_str()])
        .or_else(|| {
            run_git(
                workspace_root,
                ["diff", "--cached", "--", resolved.as_str()],
            )
        })
        .unwrap_or_default();
    lines.push(locale::tr(language, "diff hunk 列表", "diff hunk list"));
    lines.push(format!("- target: {resolved}"));
    let mut index = 0usize;
    for row in diff.lines() {
        if row.starts_with("@@") {
            index = index.saturating_add(1);
            lines.push(format!("{index:>2}. {row}"));
        }
    }
    if index == 0 {
        lines.push(locale::tr(
            language,
            "- 没有可见 hunk（可能文件只在 staged 或无变更）",
            "- no visible hunks found",
        ));
    }
    lines
}

pub(crate) fn diff_files_lines_with_language(
    workspace_root: &std::path::Path,
    language: &str,
) -> Vec<String> {
    let mut lines = vec![locale::tr(language, "变更文件树", "changed files")];
    let rows = match git_changed_files_with_status(workspace_root) {
        Ok(rows) => rows,
        Err(err) => {
            lines.push(format!("[error] {err}"));
            return lines;
        }
    };
    if rows.is_empty() {
        lines.push(locale::tr(
            language,
            "- 当前没有检测到变更",
            "- no changed files detected",
        ));
        return lines;
    }
    for (index, (status, path)) in rows.into_iter().enumerate() {
        lines.push(format!("{:>2}. [{status}] {path}", index + 1));
    }
    lines
}

pub(crate) fn run_git_file_action(
    workspace_root: &std::path::Path,
    target: &str,
    action: &str,
) -> Result<()> {
    let resolved = resolve_diff_target(workspace_root, target)?;
    let mut command = std::process::Command::new("git");
    command.current_dir(workspace_root);
    match action {
        "stage" => {
            command.args(["add", "--", resolved.as_str()]);
        }
        "unstage" => {
            command.args(["restore", "--staged", "--", resolved.as_str()]);
        }
        "revert" => {
            command.args(["restore", "--", resolved.as_str()]);
        }
        _ => return Err(anyhow!("unknown diff file action: {action}")),
    }
    let output = command.output().context("execute git action failed")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            return Err(anyhow!("git action failed for {resolved}"));
        }
        return Err(anyhow!("{stderr}"));
    }
    Ok(())
}

pub(crate) fn git_diff_summary_lines_with_language(
    workspace_root: &std::path::Path,
    language: &str,
) -> Result<Vec<String>> {
    let is_zh = locale::is_zh_language(language);
    if !workspace_root.join(".git").exists() {
        return Ok(vec![
            locale::tr(language, "变更摘要", "diff"),
            locale::tr(
                language,
                "[提示] 当前工作区不是 git 仓库",
                "[info] current workspace is not a git repository",
            ),
        ]);
    }

    let mut lines = Vec::new();
    lines.push(locale::tr(language, "变更摘要", "diff"));

    let Some(status) = run_git(workspace_root, ["status", "--porcelain"]) else {
        lines.push(locale::tr(
            language,
            "[错误] 未检测到 git（无法执行 `git status`）",
            "[error] git is not available (cannot run `git status`)",
        ));
        return Ok(lines);
    };
    if status.trim().is_empty() {
        lines.push(locale::tr(language, "- 状态: 干净", "- status: clean"));
        return Ok(lines);
    }

    let changed = status.lines().count();
    if is_zh {
        lines.push(format!("- 状态: {changed} 个路径有变更"));
    } else {
        lines.push(format!("- status: {changed} paths changed"));
    }
    for row in status.lines().take(80) {
        lines.push(format!("  {row}"));
    }
    if changed > 80 {
        if is_zh {
            lines.push(format!("  ...（还有 {} 项）", changed - 80));
        } else {
            lines.push(format!("  ... ({} more)", changed - 80));
        }
    }

    let stat = run_git(workspace_root, ["diff", "--stat"]).unwrap_or_default();
    if !stat.trim().is_empty() {
        lines.push(locale::tr(language, "- diff --stat：", "- diff --stat:"));
        for row in stat.lines().take(80) {
            lines.push(format!("  {row}"));
        }
        if stat.lines().count() > 80 {
            lines.push(locale::tr(language, "  ...（已截断）", "  ... (truncated)"));
        }
    }

    Ok(lines)
}

pub(crate) fn build_review_prompt_with_language(
    workspace_root: &std::path::Path,
    focus: &str,
    language: &str,
) -> Result<String> {
    if !workspace_root.join(".git").exists() {
        return Err(anyhow!(locale::tr(
            language,
            "当前工作区不是 git 仓库，/review 依赖 git diff",
            "current workspace is not a git repository, /review requires git diff",
        )));
    }

    let focus = focus.trim();
    let focus_line = if focus.is_empty() {
        String::new()
    } else {
        format!("Focus: {focus}\n")
    };

    let status = run_git(workspace_root, ["status", "--porcelain"]).ok_or_else(|| {
        anyhow!(locale::tr(
            language,
            "未检测到 git（无法执行 `git status`）",
            "git is not available (cannot run `git status`)",
        ))
    })?;
    let cached = run_git(workspace_root, ["diff", "--cached"]).unwrap_or_default();
    let unstaged = run_git(workspace_root, ["diff"]).unwrap_or_default();

    const MAX_DIFF_CHARS: usize = 120_000;
    let mut diff_body = String::new();
    if !cached.trim().is_empty() {
        diff_body.push_str("## git diff --cached\n");
        diff_body.push_str(&cached);
        if !diff_body.ends_with('\n') {
            diff_body.push('\n');
        }
        diff_body.push('\n');
    }
    if !unstaged.trim().is_empty() {
        diff_body.push_str("## git diff\n");
        diff_body.push_str(&unstaged);
        if !diff_body.ends_with('\n') {
            diff_body.push('\n');
        }
    }
    if diff_body.trim().is_empty() {
        diff_body = "<no diff>".to_string();
    }
    let diff_trimmed = truncate_chars(&diff_body, MAX_DIFF_CHARS);

    Ok(format!(
        r#"你是一名严格的代码审查员。请基于下面的 git 变更做 review（像 codex 一样）：
- 先列出问题（按严重程度排序）：bug/安全/行为回归/边界条件/并发/错误处理/性能/可维护性
- 再列出可选优化与可读性建议
- 最后给出建议的验证步骤（命令/测试用例）
- 输出要简洁、可执行；避免泛泛而谈

{focus_line}## git status --porcelain
{status}

{diff_trimmed}
"#
    ))
}

fn run_git<I, S>(workspace_root: &std::path::Path, args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(workspace_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars().take(max_chars) {
        out.push(ch);
    }
    out.push_str("\n...(truncated)\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_overrides_sets_default_max_rounds_when_missing() {
        let mut config = Config::default();
        let model_name = "demo";
        config.llm.default = model_name.to_string();
        let mut model = build_cli_llm_model_config(
            "openai_compatible",
            "https://example.com/v1",
            "test-key",
            model_name,
        );
        model.max_rounds = None;
        config.llm.models.insert(model_name.to_string(), model);

        let overrides =
            build_request_overrides(&config, None, None, None).expect("overrides expected");
        assert_eq!(
            overrides["llm"]["models"][model_name]["max_rounds"],
            json!(8)
        );
    }

    #[test]
    fn build_request_overrides_raises_low_max_rounds() {
        let mut config = Config::default();
        let model_name = "demo";
        config.llm.default = model_name.to_string();
        let mut model = build_cli_llm_model_config(
            "openai_compatible",
            "https://example.com/v1",
            "test-key",
            model_name,
        );
        model.max_rounds = Some(1);
        config.llm.models.insert(model_name.to_string(), model);

        let overrides =
            build_request_overrides(&config, None, None, None).expect("overrides expected");
        assert_eq!(
            overrides["llm"]["models"][model_name]["max_rounds"],
            json!(CLI_MIN_MAX_ROUNDS)
        );
    }

    #[test]
    fn build_request_overrides_keeps_safe_max_rounds_and_applies_mode() {
        let mut config = Config::default();
        let model_name = "demo";
        config.llm.default = model_name.to_string();
        let mut model = build_cli_llm_model_config(
            "openai_compatible",
            "https://example.com/v1",
            "test-key",
            model_name,
        );
        model.max_rounds = Some(12);
        config.llm.models.insert(model_name.to_string(), model);

        let overrides =
            build_request_overrides(&config, None, Some(ToolCallModeArg::FunctionCall), None)
                .expect("overrides expected");
        assert_eq!(
            overrides["llm"]["models"][model_name]["tool_call_mode"],
            json!("function_call")
        );
        assert!(overrides["llm"]["models"][model_name]["max_rounds"].is_null());

        assert!(build_request_overrides(&config, None, None, None).is_none());
    }

    #[test]
    fn parse_optional_max_context_value_supports_auto_and_numbers() {
        assert_eq!(parse_optional_max_context_value(" ").unwrap(), None);
        assert_eq!(parse_optional_max_context_value("auto").unwrap(), None);
        assert_eq!(
            parse_optional_max_context_value("32768").unwrap(),
            Some(32768)
        );
        assert!(parse_optional_max_context_value("0").is_err());
        assert!(parse_optional_max_context_value("not-a-number").is_err());
    }

    #[test]
    fn context_left_percent_handles_bounds() {
        assert_eq!(context_left_percent(0, Some(1000)), Some(100));
        assert_eq!(context_left_percent(250, Some(1000)), Some(75));
        assert_eq!(context_left_percent(1200, Some(1000)), Some(0));
        assert_eq!(context_left_percent(-10, Some(1000)), Some(100));
        assert_eq!(context_left_percent(100, None), None);
    }

    #[test]
    fn a_turn_only_succeeds_when_the_stream_reached_its_final_event() {
        let completed = AgentTurnOutcome::completed_round(FinalEvent {
            answer: "done".to_string(),
            usage: None,
            stop_reason: Some("model_response".to_string()),
        });
        assert_eq!(completed.failure(), None);

        let no_final = AgentTurnOutcome {
            final_event: FinalEvent::default(),
            saw_final: false,
            saw_error: false,
        };
        assert!(
            no_final
                .failure()
                .is_some_and(|reason| reason.contains("before completing")),
            "an interrupted stream must fail the script contract"
        );

        let errored = AgentTurnOutcome {
            final_event: FinalEvent::default(),
            saw_final: true,
            saw_error: true,
        };
        assert!(errored.failure().is_some_and(|r| r.contains("error")));

        let gave_up = AgentTurnOutcome::completed_round(FinalEvent {
            answer: "partial".to_string(),
            usage: None,
            stop_reason: Some("tool_no_progress_guard".to_string()),
        });
        assert!(
            gave_up.failure().is_some_and(|r| r.contains("gave up")),
            "a guard stop is not a completed round"
        );

        let round_limit = AgentTurnOutcome::completed_round(FinalEvent {
            answer: "partial".to_string(),
            usage: None,
            stop_reason: Some("max_rounds".to_string()),
        });
        assert!(round_limit
            .failure()
            .is_some_and(|r| r.contains("round limit")));
    }

    #[test]
    fn init_template_states_when_the_snapshot_applies() {
        let zh = init_agents_template_text("zh-CN");
        assert!(
            zh.contains("快照"),
            "the zh template must say the file is snapshotted per thread"
        );
        assert!(zh.contains("新建线程生效"));
        let en = init_agents_template_text("en-US");
        assert!(en.contains("snapshotted"));
        assert!(
            en.contains("apply to new"),
            "the en template must say edits only reach new threads: {en}"
        );
    }

    #[test]
    fn direct_shell_refuses_every_gated_policy() {
        assert!(direct_shell_is_allowed(Some("full_auto")));
        assert!(direct_shell_is_allowed(None));
        // Both gated modes send execution-class tools to the approver, and the
        // `!` path has no approver, so it must refuse rather than bypass.
        assert!(!direct_shell_is_allowed(Some("auto_edit")));
        assert!(!direct_shell_is_allowed(Some("suggest")));
    }

    #[test]
    fn parse_diff_slash_action_supports_subcommands() {
        assert_eq!(
            parse_diff_slash_action("files").expect("files action"),
            DiffSlashAction::Files
        );
        assert_eq!(
            parse_diff_slash_action("show 2").expect("show action"),
            DiffSlashAction::Show("2".to_string())
        );
        assert_eq!(
            parse_diff_slash_action("stage src/main.rs").expect("stage action"),
            DiffSlashAction::Stage("src/main.rs".to_string())
        );
    }

    #[test]
    fn parse_diff_slash_action_reports_usage_for_missing_target() {
        let err = parse_diff_slash_action("stage").expect_err("stage should require target");
        assert!(err.to_string().contains("usage: /diff stage <index|path>"));
    }
}
