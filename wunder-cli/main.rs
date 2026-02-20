mod args;
mod locale;
mod render;
mod runtime;
mod slash_command;
mod tui;

use anyhow::{anyhow, Context, Result};
use args::{
    ApprovalModeArg, AskCommand, ChatCommand, Cli, Command, CompletionCommand, ConfigCommand,
    ConfigSubcommand, DoctorCommand, ExecCommand, GlobalArgs, McpAddCommand, McpCommand,
    McpGetCommand, McpListCommand, McpNameCommand, McpSubcommand, ResumeCommand,
    SetApprovalModeCommand, SetToolCallModeCommand, SkillNameCommand, SkillsCommand,
    SkillsListCommand, SkillsSubcommand, SkillsUploadCommand, ToolCallModeArg, ToolCommand,
    ToolRunCommand, ToolSubcommand,
};
use chrono::{Local, TimeZone};
use clap::CommandFactory;
use clap::Parser;
use clap_complete::generate;
use futures::StreamExt;
use render::{FinalEvent, StreamRenderer};
use runtime::CliRuntime;
use serde_json::{json, Value};
use slash_command::{ParsedSlashCommand, SlashCommand};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing_subscriber::EnvFilter;
use wunder_server::a2a_store::A2aStore;
use wunder_server::approval::{
    new_channel as new_approval_channel, ApprovalRequestRx, ApprovalResponse,
};
use wunder_server::config::{Config, LlmModelConfig};
use wunder_server::llm::{is_openai_compatible_provider, probe_openai_context_window};
use wunder_server::path_utils::is_within_root;
use wunder_server::schemas::WunderRequest;
use wunder_server::skills::{load_skills, SkillSpec};
use wunder_server::storage::ChatSessionRecord;
use wunder_server::tools::{
    build_tool_roots, collect_available_tool_names, execute_tool, resolve_tool_name, ToolContext,
};
use wunder_server::user_tools::UserMcpServer;
use zip::ZipArchive;

const CLI_MIN_MAX_ROUNDS: u32 = 8;
const CLI_CONTEXT_PROBE_TIMEOUT_S: u64 = 15;
const CONFIG_SLASH_USAGE: &str = "/config [<base_url> <api_key> <model> [max_context|auto]]";
const CLI_DEFAULT_SESSION_TITLE: &str = "\u{65B0}\u{4F1A}\u{8BDD}";

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let runtime = CliRuntime::init(&cli.global).await?;

    match cli.command {
        Some(command) => dispatch_command(&runtime, &cli.global, command).await,
        None => run_default(&runtime, &cli.global, cli.prompt).await,
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

async fn dispatch_command(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: Command,
) -> Result<()> {
    match command {
        Command::Ask(cmd) => handle_ask(runtime, global, cmd).await,
        Command::Chat(cmd) => handle_chat(runtime, global, cmd).await,
        Command::Resume(cmd) => handle_resume(runtime, global, cmd).await,
        Command::Exec(cmd) => handle_exec(runtime, global, cmd).await,
        Command::Tool(cmd) => handle_tool(runtime, global, cmd).await,
        Command::Mcp(cmd) => handle_mcp(runtime, global, cmd).await,
        Command::Skills(cmd) => handle_skills(runtime, global, cmd).await,
        Command::Config(cmd) => handle_config(runtime, global, cmd).await,
        Command::Doctor(cmd) => handle_doctor(runtime, global, cmd).await,
        Command::Completion(cmd) => handle_completion(cmd).await,
    }
}

async fn handle_completion(command: CompletionCommand) -> Result<()> {
    let mut cmd = Cli::command();
    generate(command.shell, &mut cmd, "wunder-cli", &mut io::stdout());
    Ok(())
}

async fn run_default(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    prompt: Option<String>,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    if let Some(prompt) = prompt {
        let prompt = resolve_prompt_text(Some(prompt), language.as_str())?;
        let session_id = global
            .session
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        run_prompt_once(runtime, global, &prompt, &session_id).await?;
        return Ok(());
    }

    if !io::stdin().is_terminal() {
        let prompt = resolve_prompt_text(None, language.as_str())?;
        let session_id = global
            .session
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        run_prompt_once(runtime, global, &prompt, &session_id).await?;
        return Ok(());
    }

    if should_run_tui(global) {
        return tui::run_main(runtime, global, None, None).await;
    }

    run_chat_loop(runtime, global, None, None).await
}

async fn handle_ask(runtime: &CliRuntime, global: &GlobalArgs, command: AskCommand) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let prompt = resolve_prompt_text(command.prompt, language.as_str())?;
    let session_id = global
        .session
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    run_prompt_once(runtime, global, &prompt, &session_id).await?;
    Ok(())
}

async fn handle_chat(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: ChatCommand,
) -> Result<()> {
    run_chat_loop(runtime, global, command.prompt, None).await
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
        runtime.load_saved_session().ok_or_else(|| {
            anyhow!(locale::tr(
                language.as_str(),
                "未找到保存的会话，请先使用 `wunder-cli chat` 开始对话",
                "no saved session found, start with `wunder-cli chat` first",
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
        runtime.resolve_session(global.session.as_deref())
    };

    runtime.save_session(&session_id).ok();

    let first_prompt = match command.prompt {
        Some(prompt) => Some(resolve_prompt_text(Some(prompt), language.as_str())?),
        None => None,
    };
    if should_run_tui(global) {
        return tui::run_main(runtime, global, first_prompt, Some(session_id)).await;
    }

    run_chat_loop(runtime, global, first_prompt, Some(session_id)).await
}

fn should_run_tui(global: &GlobalArgs) -> bool {
    if global.json {
        return false;
    }
    io::stdin().is_terminal() && io::stdout().is_terminal() && io::stderr().is_terminal()
}

async fn run_chat_loop(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    first_prompt: Option<String>,
    session_override: Option<String>,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let mut session_id =
        session_override.unwrap_or_else(|| runtime.resolve_session(global.session.as_deref()));
    runtime.save_session(&session_id).ok();

    let mut first = first_prompt
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    println!(
        "{}",
        locale::tr(
            language.as_str(),
            "wunder-cli 交互模式。输入 /help 查看命令。",
            "wunder-cli interactive mode. type /help for commands.",
        )
    );
    if locale::is_zh_language(language.as_str()) {
        println!("会话: {session_id}");
    } else {
        println!("session: {session_id}");
    }

    loop {
        let input = if let Some(prompt) = first.take() {
            prompt
        } else {
            let line = read_line("wunder> ")?;
            if line.is_empty() {
                println!();
                break;
            }
            line
        };

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('/') {
            if let Some(command) = slash_command::parse_slash_command(trimmed) {
                let should_exit =
                    handle_chat_slash_command(runtime, global, &mut session_id, command).await?;
                if should_exit {
                    break;
                }
                continue;
            }
            if locale::is_zh_language(language.as_str()) {
                println!("[错误] 未知命令: {trimmed}");
                println!("输入 /help 查看可用 slash 命令");
            } else {
                println!("[error] unknown command: {trimmed}");
                println!("type /help to list available slash commands");
            }
            continue;
        }

        run_prompt_once(runtime, global, trimmed, &session_id).await?;
        runtime.save_session(&session_id).ok();
    }

    Ok(())
}

async fn handle_chat_slash_command(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    session_id: &mut String,
    command: ParsedSlashCommand<'_>,
) -> Result<bool> {
    let language = locale::resolve_cli_language(global);
    match command.command {
        SlashCommand::Help => {
            for line in slash_command::help_lines_with_language(language.as_str()) {
                println!("{line}");
            }
            Ok(false)
        }
        SlashCommand::Status => {
            print_runtime_status(runtime, global, session_id.as_str()).await?;
            Ok(false)
        }
        SlashCommand::Session => {
            print_session_stats(runtime, global, session_id.as_str()).await?;
            Ok(false)
        }
        SlashCommand::System => {
            handle_slash_system(runtime, global, session_id.as_str(), command.args).await?;
            Ok(false)
        }
        SlashCommand::Mouse => {
            println!(
                "{}",
                locale::tr(
                    language.as_str(),
                    "/mouse 仅在 TUI 模式可用（TTY 下默认直接运行 `wunder-cli`）",
                    "/mouse is available in TUI mode only (default `wunder-cli` on TTY)",
                )
            );
            Ok(false)
        }
        SlashCommand::Resume => {
            handle_slash_resume(runtime, global, session_id, command.args).await?;
            Ok(false)
        }
        SlashCommand::New => {
            *session_id = uuid::Uuid::new_v4().simple().to_string();
            runtime.save_session(session_id).ok();
            if locale::is_zh_language(language.as_str()) {
                println!("已切换到会话: {session_id}");
            } else {
                println!("switched to session: {session_id}");
            }
            Ok(false)
        }
        SlashCommand::Config => {
            config_setup_from_slash(runtime, global, command.args).await?;
            Ok(false)
        }
        SlashCommand::ConfigShow => {
            config_show(runtime, global).await?;
            Ok(false)
        }
        SlashCommand::Model => {
            handle_slash_model(runtime, global, command.args).await?;
            Ok(false)
        }
        SlashCommand::ToolCallMode => {
            handle_slash_tool_call_mode(runtime, global, command.args).await?;
            Ok(false)
        }
        SlashCommand::Approvals => {
            handle_slash_approvals(runtime, global, command.args).await?;
            Ok(false)
        }
        SlashCommand::Diff => {
            print_git_diff_summary(runtime.launch_dir.as_path(), language.as_str())?;
            Ok(false)
        }
        SlashCommand::Review => {
            let prompt = match build_review_prompt_with_language(
                runtime.launch_dir.as_path(),
                command.args,
                language.as_str(),
            ) {
                Ok(prompt) => prompt,
                Err(err) => {
                    if locale::is_zh_language(language.as_str()) {
                        println!("[错误] {err}");
                    } else {
                        println!("[error] {err}");
                    }
                    return Ok(false);
                }
            };
            run_prompt_once(runtime, global, prompt.as_str(), session_id).await?;
            Ok(false)
        }
        SlashCommand::Mention => {
            let query = command.args.trim();
            if query.is_empty() {
                println!(
                    "{}",
                    locale::tr(
                        language.as_str(),
                        "用法: /mention <query>",
                        "usage: /mention <query>",
                    )
                );
                return Ok(false);
            }
            for path in search_workspace_files(runtime.launch_dir.as_path(), query, 20) {
                println!("{path}");
            }
            Ok(false)
        }
        SlashCommand::Exit | SlashCommand::Quit => Ok(true),
    }
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

#[derive(Debug, Clone)]
pub(crate) struct ResumeSessionSummary {
    pub session_id: String,
    pub title: String,
    pub updated_at: f64,
    pub last_message_at: f64,
}

async fn query_recent_sessions(
    runtime: &CliRuntime,
    limit: i64,
) -> Result<Vec<ResumeSessionSummary>> {
    let user_store = runtime.state.user_store.clone();
    let user_id = runtime.user_id.clone();
    tokio::task::spawn_blocking(move || -> Result<Vec<ResumeSessionSummary>> {
        let (items, _) = user_store.list_chat_sessions(&user_id, None, None, 0, limit)?;
        Ok(items
            .into_iter()
            .map(|record| {
                let title = normalize_session_title(&record);
                ResumeSessionSummary {
                    session_id: record.session_id,
                    title,
                    updated_at: record.updated_at,
                    last_message_at: record.last_message_at,
                }
            })
            .collect())
    })
    .await
    .map_err(|err| anyhow!("list sessions cancelled: {err}"))?
}

pub(crate) async fn list_recent_sessions(
    runtime: &CliRuntime,
    limit: usize,
) -> Result<Vec<ResumeSessionSummary>> {
    let limit = limit.clamp(1, 200) as i64;
    let mut sessions = query_recent_sessions(runtime, limit).await?;
    if !sessions.is_empty() {
        return Ok(sessions);
    }

    if let Some(saved_session) = runtime.load_saved_session() {
        let _ = ensure_cli_session_record(runtime, &saved_session, None).await?;
        sessions = query_recent_sessions(runtime, limit).await?;
    }
    Ok(sessions)
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

fn normalize_session_title(record: &ChatSessionRecord) -> String {
    let title = record.title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    let agent = record
        .agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("-");
    format!("untitled ({agent})")
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
                created_at: now,
                updated_at: now,
                last_message_at: now,
                agent_id: None,
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

fn stream_event_payload(record: &Value) -> &Value {
    let data = record.get("data").unwrap_or(record);
    data.get("data").unwrap_or(data)
}

fn context_left_percent(used_tokens: i64, max_context: Option<u32>) -> Option<u32> {
    let total = u64::from(max_context?.max(1));
    let used = used_tokens.max(0) as u64;
    let left = total.saturating_sub(used);
    Some(((left as f64 / total as f64) * 100.0).round() as u32)
}

pub(crate) async fn load_session_stats(
    runtime: &CliRuntime,
    session_id: &str,
) -> SessionStatsSnapshot {
    let storage = runtime.state.storage.clone();
    let session_id_for_load = session_id.to_string();
    let mut stats = tokio::task::spawn_blocking(move || -> Result<SessionStatsSnapshot> {
        let mut output = SessionStatsSnapshot::default();
        let max_event_id = storage.get_max_stream_event_id(&session_id_for_load)?;
        if max_event_id <= 0 {
            return Ok(output);
        }
        let limit = max_event_id.saturating_add(64).max(1);
        let events = storage.load_stream_events(&session_id_for_load, 0, limit)?;
        for record in events {
            let event_name = record
                .get("event")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let payload = stream_event_payload(&record);
            match event_name {
                "context_usage" => {
                    if let Some(tokens) = payload.get("context_tokens").and_then(Value::as_i64) {
                        output.context_used_tokens = tokens.max(0);
                        output.context_peak_tokens = output.context_peak_tokens.max(tokens.max(0));
                    }
                }
                "llm_request" => {
                    output.model_calls = output.model_calls.saturating_add(1);
                }
                "tool_call" => {
                    output.tool_calls = output.tool_calls.saturating_add(1);
                }
                "tool_result" => {
                    output.tool_results = output.tool_results.saturating_add(1);
                }
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

async fn print_runtime_status(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    session_id: &str,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let config = runtime.state.config_store.get().await;
    let model_name = runtime
        .resolve_model_name(global.model.as_deref())
        .await
        .unwrap_or_else(|| "<none>".to_string());
    let model_entry = config.llm.models.get(&model_name);
    let tool_call_mode = model_entry
        .and_then(|model| model.tool_call_mode.as_deref())
        .unwrap_or("tool_call");
    let max_rounds = model_entry
        .and_then(|model| model.max_rounds)
        .unwrap_or(CLI_MIN_MAX_ROUNDS)
        .max(CLI_MIN_MAX_ROUNDS);
    let max_context = model_entry
        .and_then(|model| model.max_context)
        .filter(|value| *value > 0);
    let stats = load_session_stats(runtime, session_id).await;
    let approval_mode = resolve_effective_approval_mode(&config, global.approval_mode);

    println!("{}", locale::tr(language.as_str(), "状态", "status"));
    if is_zh {
        println!("- 会话: {session_id}");
    } else {
        println!("- session: {session_id}");
    }
    if is_zh {
        println!("- 模型: {model_name}");
        println!("- 工具调用模式: {tool_call_mode}");
        println!("- 审批模式: {approval_mode}");
        println!("- 最大轮次: {max_rounds}");
    } else {
        println!("- model: {model_name}");
        println!("- tool_call_mode: {tool_call_mode}");
        println!("- approval_mode: {approval_mode}");
        println!("- max_rounds: {max_rounds}");
    }
    if let Some(total) = max_context {
        let used = stats.context_used_tokens.max(0) as u64;
        let left = context_left_percent(stats.context_used_tokens, Some(total)).unwrap_or(0);
        if is_zh {
            println!("- 上下文: {used}/{total} (剩余 {left}%)");
        } else {
            println!("- context: {used}/{total} ({left}% left)");
        }
    } else if is_zh {
        println!("- 上下文: {}/未知", stats.context_used_tokens.max(0));
    } else {
        println!("- context: {}/unknown", stats.context_used_tokens.max(0));
    }
    if is_zh {
        println!("- 工作目录: {}", config.workspace.root);
        println!("- 临时目录: {}", runtime.temp_root.to_string_lossy());
        println!("- 数据库路径: {}", config.storage.db_path);
    } else {
        println!("- workspace: {}", config.workspace.root);
        println!("- temp_root: {}", runtime.temp_root.to_string_lossy());
        println!("- db_path: {}", config.storage.db_path);
    }
    Ok(())
}

async fn print_session_stats(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    session_id: &str,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let config = runtime.state.config_store.get().await;
    let model_name = runtime
        .resolve_model_name(global.model.as_deref())
        .await
        .unwrap_or_else(|| "<none>".to_string());
    let model_entry = config.llm.models.get(&model_name);
    let max_context = model_entry
        .and_then(|model| model.max_context)
        .filter(|value| *value > 0);
    let stats = load_session_stats(runtime, session_id).await;

    println!("{}", locale::tr(language.as_str(), "会话", "session"));
    if is_zh {
        println!("- 会话 ID: {session_id}");
    } else {
        println!("- id: {session_id}");
    }
    if is_zh {
        println!("- 模型: {model_name}");
    } else {
        println!("- model: {model_name}");
    }
    if let Some(total) = max_context {
        let used = stats.context_used_tokens.max(0) as u64;
        let left = context_left_percent(stats.context_used_tokens, Some(total)).unwrap_or(0);
        if is_zh {
            println!("- 上下文: {used}/{total} (剩余 {left}%)");
        } else {
            println!("- context: {used}/{total} ({left}% left)");
        }
    } else if is_zh {
        println!("- 上下文: {}/未知", stats.context_used_tokens.max(0));
    } else {
        println!("- context: {}/unknown", stats.context_used_tokens.max(0));
    }
    if is_zh {
        println!("- 模型调用: {}", stats.model_calls);
        println!("- 工具调用: {}", stats.tool_calls);
        println!("- 工具结果: {}", stats.tool_results);
        println!(
            "- token 占用: input={} output={} total={}",
            stats.total_input_tokens, stats.total_output_tokens, stats.total_tokens
        );
    } else {
        println!("- model_calls: {}", stats.model_calls);
        println!("- tool_calls: {}", stats.tool_calls);
        println!("- tool_results: {}", stats.tool_results);
        println!(
            "- token_usage: input={} output={} total={}",
            stats.total_input_tokens, stats.total_output_tokens, stats.total_tokens
        );
    }
    Ok(())
}

async fn handle_slash_resume(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    session_id: &mut String,
    args: &str,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let cleaned = args.trim();
    if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("list") {
        let sessions = list_recent_sessions(runtime, 20).await?;
        if sessions.is_empty() {
            if is_zh {
                println!("[提示] 未找到历史会话");
                println!("提示: 先开始对话，再使用 /resume 切换");
            } else {
                println!("[info] no historical sessions found");
                println!("tip: start chatting first, then use /resume to switch");
            }
            return Ok(());
        }

        println!("{}", locale::tr(language.as_str(), "恢复会话", "resume"));
        for (index, item) in sessions.iter().enumerate() {
            let marker = if item.session_id == *session_id {
                "*"
            } else {
                " "
            };
            let when = format_session_time(item.updated_at.max(item.last_message_at));
            println!(
                "{marker} {:>2}. {}  {}  {}",
                index + 1,
                item.session_id,
                when,
                item.title,
            );
        }
        println!(
            "{}",
            locale::tr(
                language.as_str(),
                "用法: /resume <session_id|index|last>",
                "usage: /resume <session_id|index|last>",
            )
        );
        return Ok(());
    }

    let target = if cleaned.eq_ignore_ascii_case("last") {
        runtime.load_saved_session().ok_or_else(|| {
            anyhow!(locale::tr(
                language.as_str(),
                "未找到保存的会话",
                "no saved session found",
            ))
        })?
    } else if let Ok(index) = cleaned.parse::<usize>() {
        let sessions = list_recent_sessions(runtime, 20).await?;
        let Some(item) = sessions.get(index.saturating_sub(1)) else {
            if is_zh {
                println!("[错误] 会话索引越界: {index}");
            } else {
                println!("[error] session index out of range: {index}");
            }
            return Ok(());
        };
        item.session_id.clone()
    } else {
        cleaned.to_string()
    };

    if target == *session_id {
        if is_zh {
            println!("当前已在会话: {target}");
        } else {
            println!("already using session: {target}");
        }
        return Ok(());
    }

    if !session_exists(runtime, &target).await? {
        if is_zh {
            println!("[错误] 会话不存在: {target}");
            println!("提示: 运行 /resume list 查看可用会话");
        } else {
            println!("[error] session not found: {target}");
            println!("tip: run /resume list to inspect available sessions");
        }
        return Ok(());
    }

    *session_id = target;
    runtime.save_session(session_id).ok();
    let history_count = load_session_history_entries(runtime, session_id, 0)
        .await
        .map(|entries| entries.len())
        .unwrap_or(0);
    if is_zh {
        println!("已恢复会话: {session_id}（已恢复 {history_count} 条消息）");
    } else {
        println!("resumed session: {session_id} ({history_count} messages restored)");
    }
    Ok(())
}

async fn handle_slash_system(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    session_id: &str,
    args: &str,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let cleaned = args.trim();
    if cleaned.eq_ignore_ascii_case("clear") {
        runtime.clear_extra_prompt()?;
        println!(
            "{}",
            locale::tr(
                language.as_str(),
                "额外提示词已清除",
                "extra prompt cleared"
            )
        );
        return Ok(());
    }
    if let Some(rest) = cleaned.strip_prefix("set ") {
        let prompt = rest.trim();
        if prompt.is_empty() {
            if is_zh {
                println!("[错误] 额外提示词为空");
                println!("用法: /system [set <extra_prompt>|clear]");
            } else {
                println!("[error] extra prompt is empty");
                println!("usage: /system [set <extra_prompt>|clear]");
            }
            return Ok(());
        }
        runtime.save_extra_prompt(prompt)?;
        if is_zh {
            println!("额外提示词已保存（{} 字符）", prompt.chars().count());
        } else {
            println!("extra prompt saved ({} chars)", prompt.chars().count());
        }
    } else if !cleaned.is_empty() && !cleaned.eq_ignore_ascii_case("show") {
        if is_zh {
            println!("[错误] 无效的 /system 参数");
            println!("用法: /system [set <extra_prompt>|clear]");
        } else {
            println!("[error] invalid /system args");
            println!("usage: /system [set <extra_prompt>|clear]");
        }
        return Ok(());
    }

    let prompt = build_current_system_prompt(runtime, global).await?;
    let extra = runtime.load_extra_prompt();
    println!("{}", locale::tr(language.as_str(), "系统提示词", "system"));
    println!(
        "{}",
        if is_zh {
            format!("- 会话: {session_id}")
        } else {
            format!("- session: {session_id}")
        }
    );
    let extra_prompt = extra
        .as_ref()
        .map(|value| {
            if is_zh {
                format!("已启用（{} 字符）", value.chars().count())
            } else {
                format!("enabled ({} chars)", value.chars().count())
            }
        })
        .unwrap_or_else(|| locale::tr(language.as_str(), "无", "none"));
    println!(
        "{}",
        if is_zh {
            format!("- 额外提示词: {extra_prompt}")
        } else {
            format!("- extra_prompt: {extra_prompt}")
        }
    );
    println!(
        "{}",
        locale::tr(
            language.as_str(),
            "--- 系统提示词开始 ---",
            "--- system prompt ---"
        )
    );
    println!("{prompt}");
    println!(
        "{}",
        locale::tr(
            language.as_str(),
            "--- 系统提示词结束 ---",
            "--- end system prompt ---",
        )
    );
    Ok(())
}

pub(crate) async fn build_current_system_prompt(
    runtime: &CliRuntime,
    global: &GlobalArgs,
) -> Result<String> {
    let config = runtime.state.config_store.get().await;
    let model_name = runtime.resolve_model_name(global.model.as_deref()).await;
    let request_overrides = build_request_overrides(
        &config,
        model_name.as_deref(),
        global.tool_call_mode,
        global.approval_mode,
    );
    let skills = runtime.state.skills.read().await.clone();
    let user_tool_bindings =
        runtime
            .state
            .user_tool_manager
            .build_bindings(&config, &skills, &runtime.user_id);
    let workspace_id = runtime
        .state
        .workspace
        .scoped_user_id(&runtime.user_id, None);
    Ok(runtime
        .state
        .orchestrator
        .build_system_prompt(
            &config,
            &[],
            &skills,
            Some(&user_tool_bindings),
            &runtime.user_id,
            false,
            &workspace_id,
            request_overrides.as_ref(),
            runtime.load_extra_prompt().as_deref(),
        )
        .await)
}

async fn handle_slash_model(runtime: &CliRuntime, global: &GlobalArgs, args: &str) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let target = args.trim();
    if target.is_empty() {
        show_model_status(runtime, global).await?;
        return Ok(());
    }

    let config = runtime.state.config_store.get().await;
    if !config.llm.models.contains_key(target) {
        if is_zh {
            println!("[错误] 模型不存在: {target}");
        } else {
            println!("[error] model not found: {target}");
        }
        let models = sorted_model_names(&config);
        if models.is_empty() {
            println!(
                "{}",
                locale::tr(
                    language.as_str(),
                    "尚未配置模型，请先运行 /config",
                    "no models configured. run /config first.",
                )
            );
        } else if is_zh {
            println!("可用模型: {}", models.join(", "));
        } else {
            println!("available models: {}", models.join(", "));
        }
        return Ok(());
    }

    let target_name = target.to_string();
    runtime
        .state
        .config_store
        .update(move |config| {
            config.llm.default = target_name.clone();
        })
        .await?;

    if is_zh {
        println!("模型已切换: {target}");
    } else {
        println!("model set: {target}");
    }
    show_model_status(runtime, global).await?;
    Ok(())
}

async fn show_model_status(runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let config = runtime.state.config_store.get().await;
    let active_model = runtime
        .resolve_model_name(global.model.as_deref())
        .await
        .unwrap_or_else(|| "<none>".to_string());
    if is_zh {
        println!("当前模型: {active_model}");
    } else {
        println!("current model: {active_model}");
    }

    let models = sorted_model_names(&config);
    if models.is_empty() {
        println!(
            "{}",
            locale::tr(
                language.as_str(),
                "尚未配置模型，请先运行 /config",
                "no models configured. run /config first.",
            )
        );
        return Ok(());
    }

    println!(
        "{}",
        locale::tr(language.as_str(), "可用模型：", "available models:")
    );
    for name in models {
        let marker = if name == active_model { "*" } else { " " };
        let model_entry = config.llm.models.get(&name);
        let mode = model_entry
            .and_then(|model| model.tool_call_mode.as_deref())
            .unwrap_or("tool_call");
        let max_rounds = model_entry
            .and_then(|model| model.max_rounds)
            .unwrap_or(CLI_MIN_MAX_ROUNDS)
            .max(CLI_MIN_MAX_ROUNDS);
        let max_context = model_entry
            .and_then(|model| model.max_context)
            .filter(|value| *value > 0)
            .map(|value| value.to_string())
            .unwrap_or_else(|| locale::tr(language.as_str(), "未知", "unknown"));
        if is_zh {
            println!("{marker} {name} ({mode}, 最大轮次={max_rounds}, 上下文上限={max_context})");
        } else {
            println!(
                "{marker} {name} ({mode}, max_rounds={max_rounds}, max_context={max_context})"
            );
        }
    }
    Ok(())
}

async fn handle_slash_tool_call_mode(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    args: &str,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let cleaned = args.trim();
    if cleaned.is_empty() {
        let config = runtime.state.config_store.get().await;
        let model_name = runtime
            .resolve_model_name(global.model.as_deref())
            .await
            .unwrap_or_else(|| "<none>".to_string());
        let mode = config
            .llm
            .models
            .get(&model_name)
            .and_then(|model| model.tool_call_mode.as_deref())
            .unwrap_or("tool_call");
        if is_zh {
            println!("工具调用模式: 模型={model_name} 模式={mode}");
        } else {
            println!("tool_call_mode: model={model_name} mode={mode}");
        }
        println!(
            "{}",
            locale::tr(
                language.as_str(),
                "用法: /tool-call-mode <tool_call|function_call> [model]",
                "usage: /tool-call-mode <tool_call|function_call> [model]",
            )
        );
        return Ok(());
    }

    let mut parts = cleaned.split_whitespace();
    let Some(mode_token) = parts.next() else {
        return Ok(());
    };
    let Some(mode) = parse_tool_call_mode(mode_token) else {
        if is_zh {
            println!("[错误] 非法模式: {mode_token}");
            println!("可选模式: tool_call, function_call");
        } else {
            println!("[error] invalid mode: {mode_token}");
            println!("valid modes: tool_call, function_call");
        }
        return Ok(());
    };

    let model = parts.next().map(str::to_string);
    if parts.next().is_some() {
        if is_zh {
            println!("[错误] 参数过多");
            println!("用法: /tool-call-mode <tool_call|function_call> [model]");
        } else {
            println!("[error] too many arguments");
            println!("usage: /tool-call-mode <tool_call|function_call> [model]");
        }
        return Ok(());
    }

    config_set_tool_call_mode(runtime, global, SetToolCallModeCommand { mode, model }).await
}

fn parse_tool_call_mode(raw: &str) -> Option<ToolCallModeArg> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "tool_call" | "tool-call" | "tool" => Some(ToolCallModeArg::ToolCall),
        "function_call" | "function-call" | "function" => Some(ToolCallModeArg::FunctionCall),
        _ => None,
    }
}

async fn handle_slash_approvals(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    args: &str,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let cleaned = args.trim();
    if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("show") {
        let config = runtime.state.config_store.get().await;
        let mode = config
            .security
            .approval_mode
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("full_auto");
        if is_zh {
            println!("审批模式: {mode}");
        } else {
            println!("approval_mode: {mode}");
        }
        println!(
            "{}",
            locale::tr(
                language.as_str(),
                "用法: /approvals [show|suggest|auto_edit|full_auto]",
                "usage: /approvals [show|suggest|auto_edit|full_auto]",
            )
        );
        return Ok(());
    }

    let Some(mode) = parse_approval_mode(cleaned) else {
        if is_zh {
            println!("[错误] 非法审批模式: {cleaned}");
            println!("可选模式: suggest, auto_edit, full_auto");
        } else {
            println!("[error] invalid approval mode: {cleaned}");
            println!("valid modes: suggest, auto_edit, full_auto");
        }
        return Ok(());
    };

    config_set_approval_mode(runtime, global, SetApprovalModeCommand { mode }).await
}

pub(crate) fn parse_approval_mode(raw: &str) -> Option<ApprovalModeArg> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "suggest" | "suggested" => Some(ApprovalModeArg::Suggest),
        "auto_edit" | "auto-edit" | "auto" => Some(ApprovalModeArg::AutoEdit),
        "full_auto" | "full-auto" | "full" => Some(ApprovalModeArg::FullAuto),
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

async fn handle_exec(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: ExecCommand,
) -> Result<()> {
    if command.command.is_empty() {
        let language = locale::resolve_cli_language(global);
        return Err(anyhow!(locale::tr(
            language.as_str(),
            "必须提供命令内容",
            "command is required",
        )));
    }
    let content = command.command.join(" ");
    let args = json!({
        "content": content,
        "workdir": command.workdir.unwrap_or_else(|| ".".to_string()),
        "timeout_s": command.timeout_s,
    });
    run_tool_direct(runtime, global, &resolve_tool_name("execute_command"), args).await
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
    let config = runtime.state.config_store.get().await;
    let skills_snapshot = runtime.state.skills.read().await.clone();
    let bindings =
        runtime
            .state
            .user_tool_manager
            .build_bindings(&config, &skills_snapshot, &runtime.user_id);
    let roots = build_tool_roots(&config, &skills_snapshot, Some(&bindings));
    let session_id = runtime.resolve_session(global.session.as_deref());
    let a2a_store = A2aStore::new();
    let http = reqwest::Client::new();

    let tool_context = ToolContext {
        user_id: &runtime.user_id,
        session_id: &session_id,
        workspace_id: &runtime.user_id,
        agent_id: None,
        is_admin: false,
        storage: runtime.state.storage.clone(),
        orchestrator: Some(runtime.state.orchestrator.clone()),
        monitor: Some(runtime.state.monitor.clone()),
        workspace: runtime.state.workspace.clone(),
        lsp_manager: runtime.state.lsp_manager.clone(),
        config: &config,
        a2a_store: &a2a_store,
        skills: &skills_snapshot,
        gateway: Some(runtime.state.gateway.clone()),
        user_tool_manager: Some(runtime.state.user_tool_manager.clone()),
        user_tool_bindings: Some(&bindings),
        user_tool_store: Some(runtime.state.user_tool_manager.store()),
        request_config_overrides: None,
        allow_roots: Some(roots.allow_roots.clone()),
        read_roots: Some(roots.read_roots.clone()),
        event_emitter: None,
        http: &http,
    };

    let result = execute_tool(&tool_context, tool_name, &args).await?;
    if global.json {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&result)?);
    }
    Ok(())
}

async fn handle_mcp(runtime: &CliRuntime, global: &GlobalArgs, command: McpCommand) -> Result<()> {
    match command.command {
        McpSubcommand::List(cmd) => mcp_list(runtime, global, cmd).await,
        McpSubcommand::Get(cmd) => mcp_get(runtime, global, cmd).await,
        McpSubcommand::Add(cmd) => mcp_add(runtime, global, cmd).await,
        McpSubcommand::Remove(cmd) => mcp_remove(runtime, global, cmd).await,
        McpSubcommand::Enable(cmd) => mcp_toggle(runtime, global, cmd, true).await,
        McpSubcommand::Disable(cmd) => mcp_toggle(runtime, global, cmd, false).await,
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
        ConfigSubcommand::SetToolCallMode(cmd) => {
            config_set_tool_call_mode(runtime, global, cmd).await
        }
        ConfigSubcommand::SetApprovalMode(cmd) => {
            config_set_approval_mode(runtime, global, cmd).await
        }
    }
}

async fn config_setup_from_slash(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    args: &str,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let cleaned = args.trim();
    if cleaned.is_empty() {
        return config_interactive_setup(runtime, global).await;
    }

    let values = match shell_words::split(cleaned) {
        Ok(parts) => parts,
        Err(err) => {
            if locale::is_zh_language(language.as_str()) {
                println!("[错误] /config 参数解析失败: {err}");
                println!("用法: {CONFIG_SLASH_USAGE}");
            } else {
                println!("[error] failed to parse /config args: {err}");
                println!("usage: {CONFIG_SLASH_USAGE}");
            }
            return Ok(());
        }
    };

    if !(values.len() == 3 || values.len() == 4) {
        if locale::is_zh_language(language.as_str()) {
            println!("[错误] /config 参数不合法");
            println!("用法: {CONFIG_SLASH_USAGE}");
        } else {
            println!("[error] invalid /config args");
            println!("usage: {CONFIG_SLASH_USAGE}");
        }
        return Ok(());
    }

    let base_url = values[0].trim();
    let api_key = values[1].trim();
    let model_name = values[2].trim();
    if base_url.is_empty() || api_key.is_empty() || model_name.is_empty() {
        if locale::is_zh_language(language.as_str()) {
            println!("[错误] /config 需要非空的 base_url、api_key 和 model");
            println!("用法: {CONFIG_SLASH_USAGE}");
        } else {
            println!("[error] /config requires non-empty base_url, api_key and model");
            println!("usage: {CONFIG_SLASH_USAGE}");
        }
        return Ok(());
    }

    let manual_max_context = if let Some(raw) = values.get(3) {
        match parse_optional_max_context_value_localized(raw, language.as_str()) {
            Ok(value) => value,
            Err(err) => {
                if locale::is_zh_language(language.as_str()) {
                    println!("[错误] {err}");
                    println!("用法: {CONFIG_SLASH_USAGE}");
                } else {
                    println!("[error] {err}");
                    println!("usage: {CONFIG_SLASH_USAGE}");
                }
                return Ok(());
            }
        }
    } else {
        None
    };

    let (provider, resolved_max_context) = apply_cli_model_config(
        runtime,
        base_url,
        api_key,
        model_name,
        manual_max_context,
        language.as_str(),
    )
    .await?;

    let is_zh = locale::is_zh_language(language.as_str());
    println!(
        "{}",
        locale::tr(language.as_str(), "模型配置完成", "model configured")
    );
    if is_zh {
        println!("- 提供商: {provider}");
        println!("- base_url: {base_url}");
        println!("- 模型: {model_name}");
    } else {
        println!("- provider: {provider}");
        println!("- base_url: {base_url}");
        println!("- model: {model_name}");
    }
    if let Some(value) = resolved_max_context {
        println!("- max_context: {value}");
    } else {
        println!(
            "{}",
            locale::tr(
                language.as_str(),
                "- max_context: 自动探测不可用（或保留现有值）",
                "- max_context: auto probe unavailable (or keep existing)",
            )
        );
    }
    if is_zh {
        println!("- 工具调用模式: tool_call");
    } else {
        println!("- tool_call_mode: tool_call");
    }
    Ok(())
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
            entry.tool_call_mode = Some("tool_call".to_string());
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

    Ok((provider, resolved_max_context))
}

async fn config_show(runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    let config = runtime.state.config_store.get().await;
    let model = runtime.resolve_model_name(global.model.as_deref()).await;
    let model_entry = model.as_ref().and_then(|name| config.llm.models.get(name));
    let tool_call_mode = model_entry
        .and_then(|model| model.tool_call_mode.clone())
        .unwrap_or_else(|| "tool_call".to_string());
    let max_rounds = model_entry
        .and_then(|model| model.max_rounds)
        .unwrap_or(CLI_MIN_MAX_ROUNDS)
        .max(CLI_MIN_MAX_ROUNDS);
    let max_context = model_entry
        .and_then(|model| model.max_context)
        .filter(|value| *value > 0);
    let approval_mode = resolve_effective_approval_mode(&config, global.approval_mode);
    let session_id = runtime.resolve_session(global.session.as_deref());
    let stats = load_session_stats(runtime, &session_id).await;

    let payload = json!({
        "launch_dir": runtime.launch_dir,
        "temp_root": runtime.temp_root,
        "user_id": runtime.user_id,
        "workspace_root": config.workspace.root,
        "storage_backend": config.storage.backend,
        "db_path": config.storage.db_path,
        "model": model,
        "tool_call_mode": tool_call_mode,
        "approval_mode": approval_mode,
        "max_rounds": max_rounds,
        "max_context": max_context,
        "context_used": stats.context_used_tokens.max(0),
        "context_left_percent": context_left_percent(stats.context_used_tokens, max_context),
        "override_path": runtime.temp_root.join("config/wunder.override.yaml"),
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

async fn config_set_tool_call_mode(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: SetToolCallModeCommand,
) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    let is_zh = locale::is_zh_language(language.as_str());
    let current = runtime.state.config_store.get().await;
    let model = if let Some(model) = command.model.clone() {
        let cleaned = model.trim().to_string();
        if !current.llm.models.contains_key(&cleaned) {
            return Err(anyhow!(if is_zh {
                format!("配置中不存在模型: {cleaned}")
            } else {
                format!("model not found in config: {cleaned}")
            }));
        }
        cleaned
    } else {
        runtime
            .resolve_model_name(global.model.as_deref())
            .await
            .ok_or_else(|| {
                anyhow!(locale::tr(
                    language.as_str(),
                    "尚未配置 LLM 模型",
                    "no llm model configured",
                ))
            })?
    };
    let mode = command.mode.as_str().to_string();
    let model_for_update = model.clone();

    runtime
        .state
        .config_store
        .update(move |config| {
            if config.llm.default.trim().is_empty() {
                config.llm.default = model_for_update.clone();
            }
            if let Some(entry) = config.llm.models.get_mut(&model_for_update) {
                entry.tool_call_mode = Some(mode.clone());
            }
        })
        .await?;

    if is_zh {
        println!(
            "工具调用模式已设置: 模型={model} 模式={}",
            command.mode.as_str()
        );
    } else {
        println!(
            "tool_call_mode set: model={model} mode={}",
            command.mode.as_str()
        );
    }
    Ok(())
}

async fn config_set_approval_mode(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: SetApprovalModeCommand,
) -> Result<()> {
    let mode = command.mode.as_str().to_string();
    runtime
        .state
        .config_store
        .update(move |config| {
            config.security.approval_mode = Some(mode.clone());
        })
        .await?;
    let language = locale::resolve_cli_language(global);
    if locale::is_zh_language(language.as_str()) {
        println!("审批模式已设置: {}", command.mode.as_str());
    } else {
        println!("approval_mode set: {}", command.mode.as_str());
    }
    Ok(())
}

async fn config_interactive_setup(runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    let language = locale::resolve_cli_language(global);
    if let Some(model) = runtime.resolve_model_name(global.model.as_deref()).await {
        if locale::is_zh_language(language.as_str()) {
            println!("当前模型: {model}");
        } else {
            println!("current model: {model}");
        }
    }
    println!(
        "{}",
        locale::tr(
            language.as_str(),
            "配置 LLM 模型（必填项直接回车可取消）",
            "configure llm model (press Enter on required field to cancel)",
        )
    );

    let Some(base_url) =
        prompt_config_value(locale::tr(language.as_str(), "base_url：", "base_url: ").as_str())?
    else {
        println!(
            "{}",
            locale::tr(language.as_str(), "配置已取消", "config cancelled")
        );
        return Ok(());
    };
    let Some(api_key) =
        prompt_config_value(locale::tr(language.as_str(), "api_key：", "api_key: ").as_str())?
    else {
        println!(
            "{}",
            locale::tr(language.as_str(), "配置已取消", "config cancelled")
        );
        return Ok(());
    };
    let Some(model_name) =
        prompt_config_value(locale::tr(language.as_str(), "model：", "model: ").as_str())?
    else {
        println!(
            "{}",
            locale::tr(language.as_str(), "配置已取消", "config cancelled")
        );
        return Ok(());
    };
    let manual_max_context = parse_optional_max_context_value_localized(
        read_line(
            locale::tr(
                language.as_str(),
                "max_context（可选，回车自动探测）：",
                "max_context (optional, Enter for auto probe): ",
            )
            .as_str(),
        )?
        .as_str(),
        language.as_str(),
    )?;

    let (provider, resolved_max_context) = apply_cli_model_config(
        runtime,
        &base_url,
        &api_key,
        &model_name,
        manual_max_context,
        language.as_str(),
    )
    .await?;

    println!(
        "{}",
        locale::tr(language.as_str(), "模型配置完成", "model configured")
    );
    if locale::is_zh_language(language.as_str()) {
        println!("- 提供商: {provider}");
        println!("- base_url: {base_url}");
        println!("- 模型: {model_name}");
    } else {
        println!("- provider: {provider}");
        println!("- base_url: {base_url}");
        println!("- model: {model_name}");
    }
    if let Some(value) = resolved_max_context {
        println!("- max_context: {value}");
    } else {
        println!(
            "{}",
            locale::tr(
                language.as_str(),
                "- max_context: 自动探测不可用（或保留现有值）",
                "- max_context: auto probe unavailable (or keep existing)",
            )
        );
    }
    if locale::is_zh_language(language.as_str()) {
        println!("- 工具调用模式: tool_call");
    } else {
        println!("- tool_call_mode: tool_call");
    }
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

fn prompt_config_value(prompt: &str) -> Result<Option<String>> {
    let value = read_line(prompt)?;
    let cleaned = value.trim();
    if cleaned.is_empty() {
        return Ok(None);
    }
    Ok(Some(cleaned.to_string()))
}

fn infer_provider_from_base_url(base_url: &str) -> String {
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
        base_url: Some(base_url.to_string()),
        api_key: Some(api_key.to_string()),
        model: Some(model_name.to_string()),
        temperature: None,
        timeout_s: None,
        retry: None,
        max_rounds: Some(CLI_MIN_MAX_ROUNDS),
        max_context: None,
        max_output: None,
        support_vision: None,
        stream: None,
        stream_include_usage: None,
        history_compaction_ratio: None,
        history_compaction_reset: None,
        tool_call_mode: Some("tool_call".to_string()),
        model_type: Some("llm".to_string()),
        stop: None,
        mock_if_unconfigured: None,
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
            "base_config",
            std::env::var("WUNDER_CONFIG_PATH").unwrap_or_default(),
            true,
        ),
        (
            "override_config",
            std::env::var("WUNDER_CONFIG_OVERRIDE_PATH").unwrap_or_default(),
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
                runtime
                    .temp_root
                    .join("config/wunder.override.yaml")
                    .exists()
            )
        } else {
            format!(
                "- override_config_exists: {}",
                runtime
                    .temp_root
                    .join("config/wunder.override.yaml")
                    .exists()
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
                "base_config" => "基础配置",
                "override_config" => "覆盖配置",
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
            "base_config_path": std::env::var("WUNDER_CONFIG_PATH").unwrap_or_default(),
            "override_config_path": std::env::var("WUNDER_CONFIG_OVERRIDE_PATH").unwrap_or_default(),
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
) -> Result<WunderRequest> {
    let config = runtime.state.config_store.get().await;
    let model_name = runtime.resolve_model_name(global.model.as_deref()).await;
    let request_overrides = build_request_overrides(
        &config,
        model_name.as_deref(),
        global.tool_call_mode,
        global.approval_mode,
    );

    ensure_cli_session_record(runtime, session_id, Some(prompt)).await?;

    Ok(WunderRequest {
        user_id: runtime.user_id.clone(),
        question: prompt.trim().to_string(),
        tool_names: Vec::new(),
        skip_tool_calls: false,
        stream: !global.no_stream,
        debug_payload: false,
        session_id: Some(session_id.to_string()),
        agent_id: None,
        model_name,
        language: global.language.clone(),
        config_overrides: request_overrides,
        agent_prompt: runtime.load_extra_prompt(),
        attachments: None,
        allow_queue: true,
        is_admin: false,
        approval_tx: None,
    })
}

async fn run_prompt_once(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    prompt: &str,
    session_id: &str,
) -> Result<FinalEvent> {
    let language = locale::resolve_cli_language(global);
    let mut request = build_wunder_request(runtime, global, prompt, session_id).await?;
    let _approval_task = if should_interactive_approvals(global) {
        let (tx, rx) = new_approval_channel();
        request.approval_tx = Some(tx);
        Some(tokio::spawn(handle_stdio_approvals(rx, language)))
    } else {
        None
    };

    if global.no_stream {
        let response = runtime.state.orchestrator.run(request).await?;
        let final_event = FinalEvent {
            answer: response.answer.clone(),
            usage: response
                .usage
                .map(|usage| serde_json::to_value(usage).unwrap_or(Value::Null)),
            stop_reason: response.stop_reason,
        };
        if global.json {
            let payload = json!({
                "event": "final",
                "data": {
                    "answer": response.answer,
                    "usage": final_event.usage,
                    "stop_reason": final_event.stop_reason,
                    "session_id": response.session_id,
                }
            });
            println!("{}", serde_json::to_string(&payload)?);
        } else {
            println!("{}", response.answer);
        }
        return Ok(final_event);
    }

    let mut stream = runtime.state.orchestrator.stream(request).await?;
    let mut renderer = StreamRenderer::new(global.json);
    let mut final_event = FinalEvent::default();
    while let Some(item) = stream.next().await {
        let event = item.expect("infallible stream event");
        if let Some(final_payload) = renderer.render_event(&event)? {
            final_event = final_payload;
        }
    }
    renderer.finish();
    Ok(final_event)
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
        eprintln!();
        if is_zh {
            eprintln!("[审批] {}", request.summary);
        } else {
            eprintln!("[approval] {}", request.summary);
        }
        if is_zh {
            eprintln!("- 工具: {}", request.tool);
        } else {
            eprintln!("- tool: {}", request.tool);
        }
        let args = serde_json::to_string(&request.args).unwrap_or_else(|_| "{}".to_string());
        if !args.trim().is_empty() && args != "{}" {
            if is_zh {
                eprintln!("- 参数: {}", truncate_for_stderr(args, 600, is_zh));
            } else {
                eprintln!("- args: {}", truncate_for_stderr(args, 600, is_zh));
            }
        }
        eprintln!(
            "{}",
            locale::tr(
                language.as_str(),
                "是否批准？[y]=本次  [a]=本会话  [n]=拒绝",
                "approve? [y]=once  [a]=session  [n]=deny",
            )
        );

        let choice = tokio::task::spawn_blocking(|| {
            let mut buffer = String::new();
            std::io::stdin().read_line(&mut buffer).ok();
            buffer
        })
        .await
        .ok()
        .unwrap_or_default();

        let response = match choice.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" | "1" => ApprovalResponse::ApproveOnce,
            "a" | "always" | "2" => ApprovalResponse::ApproveSession,
            _ => ApprovalResponse::Deny,
        };
        let _ = request.respond_to.send(response);
    }
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

fn read_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout().flush().ok();
    let mut buffer = String::new();
    io::stdin().read_line(&mut buffer)?;
    Ok(buffer)
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

pub(crate) fn print_git_diff_summary(
    workspace_root: &std::path::Path,
    language: &str,
) -> Result<()> {
    for line in git_diff_summary_lines_with_language(workspace_root, language)? {
        println!("{line}");
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

pub(crate) fn search_workspace_files(
    workspace_root: &std::path::Path,
    query: &str,
    limit: usize,
) -> Vec<String> {
    let query = query.trim();
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let lowered = query.to_ascii_lowercase();

    // Avoid scanning huge dependency trees in common wunder repos.
    let excluded_dirs = [
        ".git",
        "target",
        "WUNDER_TEMP",
        "data",
        "frontend",
        "web",
        "node_modules",
        "参考项目",
        "backups",
    ];

    let mut matches = Vec::new();
    let walker = walkdir::WalkDir::new(workspace_root).follow_links(false);
    for entry in walker
        .into_iter()
        .filter_entry(|entry| {
            let path = entry.path();
            if path == workspace_root {
                return true;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                return true;
            };
            !excluded_dirs
                .iter()
                .any(|excluded| name.eq_ignore_ascii_case(excluded))
        })
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(workspace_root) else {
            continue;
        };
        let rel = relative.to_string_lossy().replace('\\', "/");
        if rel.is_empty() {
            continue;
        }

        if rel.to_ascii_lowercase().contains(&lowered) {
            matches.push(rel);
            if matches.len() >= limit {
                break;
            }
        }
    }

    matches.sort();
    matches.truncate(limit);
    matches
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
}
