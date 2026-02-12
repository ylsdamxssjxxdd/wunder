mod args;
mod render;
mod runtime;
mod slash_command;
mod tui;

use anyhow::{anyhow, Context, Result};
use args::{
    AskCommand, ChatCommand, Cli, Command, ConfigCommand, ConfigSubcommand, DoctorCommand,
    ExecCommand, GlobalArgs, McpAddCommand, McpCommand, McpGetCommand, McpListCommand,
    McpNameCommand, McpSubcommand, ResumeCommand, SetToolCallModeCommand, SkillNameCommand,
    SkillsCommand, SkillsSubcommand, ToolCallModeArg, ToolCommand, ToolRunCommand, ToolSubcommand,
};
use chrono::{Local, TimeZone};
use clap::Parser;
use futures::StreamExt;
use render::{FinalEvent, StreamRenderer};
use runtime::CliRuntime;
use serde_json::{json, Value};
use wunder_server::storage::ChatSessionRecord;
use slash_command::{ParsedSlashCommand, SlashCommand};
use std::collections::HashSet;
use std::io::{self, IsTerminal, Read, Write};
use tracing_subscriber::EnvFilter;
use wunder_server::a2a_store::A2aStore;
use wunder_server::config::{Config, LlmModelConfig};
use wunder_server::llm::{is_openai_compatible_provider, probe_openai_context_window};
use wunder_server::schemas::WunderRequest;
use wunder_server::skills::load_skills;
use wunder_server::tools::{
    build_tool_roots, collect_available_tool_names, execute_tool, resolve_tool_name, ToolContext,
};
use wunder_server::user_tools::UserMcpServer;

const CLI_MIN_MAX_ROUNDS: u32 = 8;
const CLI_CONTEXT_PROBE_TIMEOUT_S: u64 = 15;
const CONFIG_SLASH_USAGE: &str = "/config [<base_url> <api_key> <model> [max_context|auto]]";

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
        Command::Mcp(cmd) => handle_mcp(runtime, cmd).await,
        Command::Skills(cmd) => handle_skills(runtime, cmd).await,
        Command::Config(cmd) => handle_config(runtime, global, cmd).await,
        Command::Doctor(cmd) => handle_doctor(runtime, global, cmd).await,
    }
}

async fn run_default(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    prompt: Option<String>,
) -> Result<()> {
    if let Some(prompt) = prompt {
        let prompt = resolve_prompt_text(Some(prompt))?;
        let session_id = global
            .session
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        run_prompt_once(runtime, global, &prompt, &session_id).await?;
        return Ok(());
    }

    if !io::stdin().is_terminal() {
        let prompt = resolve_prompt_text(None)?;
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
    let prompt = resolve_prompt_text(command.prompt)?;
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
    if command.last && command.prompt.is_none() {
        // Clap cannot express this positional behavior directly.
        command.prompt = command.session_id.take();
    }

    let session_id = if command.last {
        runtime
            .load_saved_session()
            .ok_or_else(|| anyhow!("no saved session found, start with `wunder-cli chat` first"))?
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
        Some(prompt) => Some(resolve_prompt_text(Some(prompt))?),
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
    let mut session_id =
        session_override.unwrap_or_else(|| runtime.resolve_session(global.session.as_deref()));
    runtime.save_session(&session_id).ok();

    let mut first = first_prompt
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    println!("wunder-cli interactive mode. type /help for commands.");
    println!("session: {session_id}");

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
            println!("[error] unknown command: {trimmed}");
            println!("type /help to list available slash commands");
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
    match command.command {
        SlashCommand::Help => {
            for line in slash_command::help_lines() {
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
            println!("/mouse is available in TUI mode only (default `wunder-cli` on TTY)");
            Ok(false)
        }
        SlashCommand::Resume => {
            handle_slash_resume(runtime, session_id, command.args).await?;
            Ok(false)
        }
        SlashCommand::New => {
            *session_id = uuid::Uuid::new_v4().simple().to_string();
            runtime.save_session(session_id).ok();
            println!("switched to session: {session_id}");
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

pub(crate) async fn list_recent_sessions(
    runtime: &CliRuntime,
    limit: usize,
) -> Result<Vec<ResumeSessionSummary>> {
    let user_store = runtime.state.user_store.clone();
    let user_id = runtime.user_id.clone();
    let limit = (limit.max(1).min(200)) as i64;
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

pub(crate) async fn session_exists(runtime: &CliRuntime, session_id: &str) -> Result<bool> {
    let user_store = runtime.state.user_store.clone();
    let user_id = runtime.user_id.clone();
    let session_id = session_id.to_string();
    tokio::task::spawn_blocking(move || {
        user_store
            .get_chat_session(&user_id, &session_id)
            .map(|record| record.is_some())
    })
    .await
    .map_err(|err| anyhow!("session query cancelled: {err}"))?
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

    println!("status");
    println!("- session: {session_id}");
    println!("- model: {model_name}");
    println!("- tool_call_mode: {tool_call_mode}");
    println!("- max_rounds: {max_rounds}");
    if let Some(total) = max_context {
        let used = stats.context_used_tokens.max(0) as u64;
        let left = context_left_percent(stats.context_used_tokens, Some(total)).unwrap_or(0);
        println!("- context: {used}/{total} ({left}% left)");
    } else {
        println!("- context: {}/unknown", stats.context_used_tokens.max(0));
    }
    println!("- workspace: {}", config.workspace.root);
    println!("- temp_root: {}", runtime.temp_root.to_string_lossy());
    println!("- db_path: {}", config.storage.db_path);
    Ok(())
}

async fn print_session_stats(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    session_id: &str,
) -> Result<()> {
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

    println!("session");
    println!("- id: {session_id}");
    println!("- model: {model_name}");
    if let Some(total) = max_context {
        let used = stats.context_used_tokens.max(0) as u64;
        let left = context_left_percent(stats.context_used_tokens, Some(total)).unwrap_or(0);
        println!("- context: {used}/{total} ({left}% left)");
    } else {
        println!("- context: {}/unknown", stats.context_used_tokens.max(0));
    }
    println!("- model_calls: {}", stats.model_calls);
    println!("- tool_calls: {}", stats.tool_calls);
    println!("- tool_results: {}", stats.tool_results);
    println!(
        "- token_usage: input={} output={} total={}",
        stats.total_input_tokens, stats.total_output_tokens, stats.total_tokens
    );
    Ok(())
}

async fn handle_slash_resume(
    runtime: &CliRuntime,
    session_id: &mut String,
    args: &str,
) -> Result<()> {
    let cleaned = args.trim();
    if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("list") {
        let sessions = list_recent_sessions(runtime, 20).await?;
        if sessions.is_empty() {
            println!("[info] no historical sessions found");
            println!("tip: start chatting first, then use /resume to switch");
            return Ok(());
        }

        println!("resume");
        for (index, item) in sessions.iter().enumerate() {
            let marker = if item.session_id == *session_id { "*" } else { " " };
            let when = format_session_time(item.updated_at.max(item.last_message_at));
            println!(
                "{marker} {:>2}. {}  {}  {}",
                index + 1,
                item.session_id,
                when,
                item.title,
            );
        }
        println!("usage: /resume <session_id|index|last>");
        return Ok(());
    }

    let target = if cleaned.eq_ignore_ascii_case("last") {
        runtime
            .load_saved_session()
            .ok_or_else(|| anyhow!("no saved session found"))?
    } else if let Ok(index) = cleaned.parse::<usize>() {
        let sessions = list_recent_sessions(runtime, 20).await?;
        let Some(item) = sessions.get(index.saturating_sub(1)) else {
            println!("[error] session index out of range: {index}");
            return Ok(());
        };
        item.session_id.clone()
    } else {
        cleaned.to_string()
    };

    if target == *session_id {
        println!("already using session: {target}");
        return Ok(());
    }

    if !session_exists(runtime, &target).await? {
        println!("[error] session not found: {target}");
        println!("tip: run /resume list to inspect available sessions");
        return Ok(());
    }

    *session_id = target;
    runtime.save_session(session_id).ok();
    let history_count = load_session_history_entries(runtime, session_id, 0)
        .await
        .map(|entries| entries.len())
        .unwrap_or(0);
    println!("resumed session: {session_id} ({history_count} messages restored)");
    Ok(())
}

async fn handle_slash_system(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    session_id: &str,
    args: &str,
) -> Result<()> {
    let cleaned = args.trim();
    if cleaned.eq_ignore_ascii_case("clear") {
        runtime.clear_extra_prompt()?;
        println!("extra prompt cleared");
        return Ok(());
    }
    if let Some(rest) = cleaned.strip_prefix("set ") {
        let prompt = rest.trim();
        if prompt.is_empty() {
            println!("[error] extra prompt is empty");
            println!("usage: /system [set <extra_prompt>|clear]");
            return Ok(());
        }
        runtime.save_extra_prompt(prompt)?;
        println!("extra prompt saved ({} chars)", prompt.chars().count());
    } else if !cleaned.is_empty() && !cleaned.eq_ignore_ascii_case("show") {
        println!("[error] invalid /system args");
        println!("usage: /system [set <extra_prompt>|clear]");
        return Ok(());
    }

    let prompt = build_current_system_prompt(runtime, global).await?;
    let extra = runtime.load_extra_prompt();
    println!("system");
    println!("- session: {session_id}");
    println!(
        "- extra_prompt: {}",
        extra
            .as_ref()
            .map(|value| format!("enabled ({} chars)", value.chars().count()))
            .unwrap_or_else(|| "none".to_string())
    );
    println!("--- system prompt ---");
    println!("{prompt}");
    println!("--- end system prompt ---");
    Ok(())
}

pub(crate) async fn build_current_system_prompt(
    runtime: &CliRuntime,
    global: &GlobalArgs,
) -> Result<String> {
    let config = runtime.state.config_store.get().await;
    let model_name = runtime.resolve_model_name(global.model.as_deref()).await;
    let request_overrides =
        build_request_overrides(&config, model_name.as_deref(), global.tool_call_mode);
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
    let target = args.trim();
    if target.is_empty() {
        show_model_status(runtime, global).await?;
        return Ok(());
    }

    let config = runtime.state.config_store.get().await;
    if !config.llm.models.contains_key(target) {
        println!("[error] model not found: {target}");
        let models = sorted_model_names(&config);
        if models.is_empty() {
            println!("no models configured. run /config first.");
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

    println!("model set: {target}");
    show_model_status(runtime, global).await?;
    Ok(())
}

async fn show_model_status(runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    let config = runtime.state.config_store.get().await;
    let active_model = runtime
        .resolve_model_name(global.model.as_deref())
        .await
        .unwrap_or_else(|| "<none>".to_string());
    println!("current model: {active_model}");

    let models = sorted_model_names(&config);
    if models.is_empty() {
        println!("no models configured. run /config first.");
        return Ok(());
    }

    println!("available models:");
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
            .unwrap_or_else(|| "unknown".to_string());
        println!("{marker} {name} ({mode}, max_rounds={max_rounds}, max_context={max_context})");
    }
    Ok(())
}

async fn handle_slash_tool_call_mode(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    args: &str,
) -> Result<()> {
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
        println!("tool_call_mode: model={model_name} mode={mode}");
        println!("usage: /tool-call-mode <tool_call|function_call> [model]");
        return Ok(());
    }

    let mut parts = cleaned.split_whitespace();
    let Some(mode_token) = parts.next() else {
        return Ok(());
    };
    let Some(mode) = parse_tool_call_mode(mode_token) else {
        println!("[error] invalid mode: {mode_token}");
        println!("valid modes: tool_call, function_call");
        return Ok(());
    };

    let model = parts.next().map(str::to_string);
    if parts.next().is_some() {
        println!("[error] too many arguments");
        println!("usage: /tool-call-mode <tool_call|function_call> [model]");
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

fn sorted_model_names(config: &Config) -> Vec<String> {
    let mut names: Vec<String> = config.llm.models.keys().cloned().collect();
    names.sort();
    names
}

async fn handle_exec(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: ExecCommand,
) -> Result<()> {
    if command.command.is_empty() {
        return Err(anyhow!("command is required"));
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
    let args: Value = serde_json::from_str(command.args.trim())
        .with_context(|| format!("invalid json for --args: {}", command.args.trim()))?;
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

async fn handle_mcp(runtime: &CliRuntime, command: McpCommand) -> Result<()> {
    match command.command {
        McpSubcommand::List(cmd) => mcp_list(runtime, cmd).await,
        McpSubcommand::Get(cmd) => mcp_get(runtime, cmd).await,
        McpSubcommand::Add(cmd) => mcp_add(runtime, cmd).await,
        McpSubcommand::Remove(cmd) => mcp_remove(runtime, cmd).await,
        McpSubcommand::Enable(cmd) => mcp_toggle(runtime, cmd, true).await,
        McpSubcommand::Disable(cmd) => mcp_toggle(runtime, cmd, false).await,
    }
}

async fn mcp_list(runtime: &CliRuntime, command: McpListCommand) -> Result<()> {
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
        println!("No MCP servers configured. Use `wunder-cli mcp add` to add one.");
        return Ok(());
    }
    for server in payload.mcp_servers {
        let state = format_mcp_state(&server);
        println!("{} ({state})", server.name);
        println!("  transport: {}", server.transport);
        println!("  endpoint: {}", server.endpoint);
        if !server.allow_tools.is_empty() {
            println!("  allow_tools: {}", server.allow_tools.join(", "));
        }
        println!("  remove: wunder-cli mcp remove {}", server.name);
    }
    Ok(())
}

async fn mcp_get(runtime: &CliRuntime, command: McpGetCommand) -> Result<()> {
    let payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    let server = payload
        .mcp_servers
        .into_iter()
        .find(|server| server.name.trim() == command.name.trim())
        .ok_or_else(|| anyhow!("mcp server not found: {}", command.name.trim()))?;

    if command.json {
        println!("{}", serde_json::to_string_pretty(&server)?);
        return Ok(());
    }

    println!("{}", server.name);
    println!("  status: {}", format_mcp_state(&server));
    println!("  transport: {}", server.transport);
    println!("  endpoint: {}", server.endpoint);
    let description = if server.description.trim().is_empty() {
        "-"
    } else {
        server.description.as_str()
    };
    println!("  description: {description}");
    let display_name = if server.display_name.trim().is_empty() {
        "-"
    } else {
        server.display_name.as_str()
    };
    println!("  display_name: {display_name}");
    if !server.allow_tools.is_empty() {
        println!("  allow_tools: {}", server.allow_tools.join(", "));
    }
    if !server.shared_tools.is_empty() {
        println!("  shared_tools: {}", server.shared_tools.join(", "));
    }
    println!("  remove: wunder-cli mcp remove {}", server.name);
    Ok(())
}

async fn mcp_add(runtime: &CliRuntime, command: McpAddCommand) -> Result<()> {
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
    println!("mcp server added: {}", command.name.trim());
    Ok(())
}

async fn mcp_remove(runtime: &CliRuntime, command: McpNameCommand) -> Result<()> {
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
        println!("mcp server not found: {}", command.name.trim());
    } else {
        println!("mcp server removed: {}", command.name.trim());
    }
    Ok(())
}

async fn mcp_toggle(runtime: &CliRuntime, command: McpNameCommand, enabled: bool) -> Result<()> {
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
        let state = if enabled { "enabled" } else { "disabled" };
        println!("mcp server {state}: {}", command.name.trim());
    } else {
        println!("mcp server not found: {}", command.name.trim());
    }
    Ok(())
}

fn format_mcp_state(server: &UserMcpServer) -> &'static str {
    if server.enabled {
        "enabled"
    } else {
        "disabled"
    }
}

async fn handle_skills(runtime: &CliRuntime, command: SkillsCommand) -> Result<()> {
    match command.command {
        SkillsSubcommand::List => skills_list(runtime).await,
        SkillsSubcommand::Enable(cmd) => skills_toggle(runtime, cmd, true).await,
        SkillsSubcommand::Disable(cmd) => skills_toggle(runtime, cmd, false).await,
    }
}

async fn skills_list(runtime: &CliRuntime) -> Result<()> {
    let payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    let enabled_set: HashSet<String> = payload.skills.enabled.into_iter().collect();

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
    if specs.is_empty() {
        println!("no skills found in {}", skill_root.to_string_lossy());
        return Ok(());
    }
    for spec in specs {
        let enabled = if enabled_set.contains(&spec.name) {
            "enabled"
        } else {
            "disabled"
        };
        println!("{} [{}]", spec.name, enabled);
    }
    Ok(())
}

async fn skills_toggle(
    runtime: &CliRuntime,
    command: SkillNameCommand,
    enable: bool,
) -> Result<()> {
    let payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    let mut enabled = payload.skills.enabled;
    let target = command.name.trim().to_string();
    enabled.retain(|name| name.trim() != target);
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
        println!("skill enabled: {target}");
    } else {
        println!("skill disabled: {target}");
    }
    Ok(())
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
    }
}

async fn config_setup_from_slash(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    args: &str,
) -> Result<()> {
    let cleaned = args.trim();
    if cleaned.is_empty() {
        return config_interactive_setup(runtime, global).await;
    }

    let values = match shell_words::split(cleaned) {
        Ok(parts) => parts,
        Err(err) => {
            println!("[error] failed to parse /config args: {err}");
            println!("usage: {CONFIG_SLASH_USAGE}");
            return Ok(());
        }
    };

    if !(values.len() == 3 || values.len() == 4) {
        println!("[error] invalid /config args");
        println!("usage: {CONFIG_SLASH_USAGE}");
        return Ok(());
    }

    let base_url = values[0].trim();
    let api_key = values[1].trim();
    let model_name = values[2].trim();
    if base_url.is_empty() || api_key.is_empty() || model_name.is_empty() {
        println!("[error] /config requires non-empty base_url, api_key and model");
        println!("usage: {CONFIG_SLASH_USAGE}");
        return Ok(());
    }

    let manual_max_context = if let Some(raw) = values.get(3) {
        match parse_optional_max_context_value(raw) {
            Ok(value) => value,
            Err(err) => {
                println!("[error] {err}");
                println!("usage: {CONFIG_SLASH_USAGE}");
                return Ok(());
            }
        }
    } else {
        None
    };

    let (provider, resolved_max_context) =
        apply_cli_model_config(runtime, base_url, api_key, model_name, manual_max_context).await?;

    println!("model configured");
    println!("- provider: {provider}");
    println!("- base_url: {base_url}");
    println!("- model: {model_name}");
    if let Some(value) = resolved_max_context {
        println!("- max_context: {value}");
    } else {
        println!("- max_context: auto probe unavailable (or keep existing)");
    }
    println!("- tool_call_mode: tool_call");
    Ok(())
}

pub(crate) async fn apply_cli_model_config(
    runtime: &CliRuntime,
    base_url: &str,
    api_key: &str,
    model_name: &str,
    manual_max_context: Option<u32>,
) -> Result<(String, Option<u32>)> {
    let base_url = base_url.trim().to_string();
    let api_key = api_key.trim().to_string();
    let model_name = model_name.trim().to_string();
    if base_url.is_empty() || api_key.is_empty() || model_name.is_empty() {
        return Err(anyhow!("base_url, api_key and model are required"));
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
    let current = runtime.state.config_store.get().await;
    let model = if let Some(model) = command.model.clone() {
        let cleaned = model.trim().to_string();
        if !current.llm.models.contains_key(&cleaned) {
            return Err(anyhow!("model not found in config: {cleaned}"));
        }
        cleaned
    } else {
        runtime
            .resolve_model_name(global.model.as_deref())
            .await
            .ok_or_else(|| anyhow!("no llm model configured"))?
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

    println!(
        "tool_call_mode set: model={model} mode={}",
        command.mode.as_str()
    );
    Ok(())
}

async fn config_interactive_setup(runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    if let Some(model) = runtime.resolve_model_name(global.model.as_deref()).await {
        println!("current model: {model}");
    }
    println!("configure llm model (press Enter on required field to cancel)");

    let Some(base_url) = prompt_config_value("base_url: ")? else {
        println!("config cancelled");
        return Ok(());
    };
    let Some(api_key) = prompt_config_value("api_key: ")? else {
        println!("config cancelled");
        return Ok(());
    };
    let Some(model_name) = prompt_config_value("model: ")? else {
        println!("config cancelled");
        return Ok(());
    };
    let manual_max_context = parse_optional_max_context_value(
        read_line("max_context (optional, Enter for auto probe): ")?.as_str(),
    )?;

    let (provider, resolved_max_context) = apply_cli_model_config(
        runtime,
        &base_url,
        &api_key,
        &model_name,
        manual_max_context,
    )
    .await?;

    println!("model configured");
    println!("- provider: {provider}");
    println!("- base_url: {base_url}");
    println!("- model: {model_name}");
    if let Some(value) = resolved_max_context {
        println!("- max_context: {value}");
    } else {
        println!("- max_context: auto probe unavailable (or keep existing)");
    }
    println!("- tool_call_mode: tool_call");
    Ok(())
}

fn parse_optional_max_context_value(raw: &str) -> Result<Option<u32>> {
    let cleaned = raw.trim();
    if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }
    let value = cleaned
        .parse::<u32>()
        .map_err(|_| anyhow!("max_context must be a positive integer"))?;
    if value == 0 {
        return Err(anyhow!("max_context must be greater than 0"));
    }
    Ok(Some(value))
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

    println!("wunder-cli doctor");
    println!("- launch_dir: {}", runtime.launch_dir.to_string_lossy());
    println!("- temp_root: {}", runtime.temp_root.to_string_lossy());
    println!("- project_root: {}", runtime.repo_root.to_string_lossy());
    println!("- user_id: {}", runtime.user_id);
    println!("- workspace_root: {}", config.workspace.root);
    println!("- db_path: {}", config.storage.db_path);
    println!("- model: {}", model.unwrap_or_else(|| "<none>".to_string()));

    for (name, path, should_exist) in checks {
        let exists = if path.trim().is_empty() {
            false
        } else {
            std::path::Path::new(path.as_str()).exists()
        };
        let status = if !should_exist || exists {
            "ok"
        } else {
            "missing"
        };
        println!("- {name}: [{status}] {path}");
    }

    if command.verbose {
        let payload = json!({
            "skills_paths": config.skills.paths,
            "allow_paths": config.security.allow_paths,
            "allow_commands": config.security.allow_commands,
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
) -> WunderRequest {
    let config = runtime.state.config_store.get().await;
    let model_name = runtime.resolve_model_name(global.model.as_deref()).await;
    let request_overrides =
        build_request_overrides(&config, model_name.as_deref(), global.tool_call_mode);

    WunderRequest {
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
    }
}

async fn run_prompt_once(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    prompt: &str,
    session_id: &str,
) -> Result<FinalEvent> {
    let request = build_wunder_request(runtime, global, prompt, session_id).await;

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

fn build_request_overrides(
    config: &Config,
    model_name: Option<&str>,
    tool_call_mode: Option<ToolCallModeArg>,
) -> Option<Value> {
    let selected_model = resolve_selected_model(config, model_name)?;
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
        return None;
    }

    Some(json!({
        "llm": {
            "models": {
                selected_model: model_overrides
            }
        }
    }))
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

fn resolve_prompt_text(prompt: Option<String>) -> Result<String> {
    match prompt {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed == "-" {
                read_stdin_all()
            } else if trimmed.is_empty() {
                Err(anyhow!("prompt is empty"))
            } else {
                Ok(trimmed.to_string())
            }
        }
        None => read_stdin_all(),
    }
}

fn read_stdin_all() -> Result<String> {
    if io::stdin().is_terminal() {
        return Err(anyhow!("prompt is required"));
    }
    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;
    let text = buffer.trim();
    if text.is_empty() {
        Err(anyhow!("stdin is empty"))
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

        let overrides = build_request_overrides(&config, None, None).expect("overrides expected");
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

        let overrides = build_request_overrides(&config, None, None).expect("overrides expected");
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

        let overrides = build_request_overrides(&config, None, Some(ToolCallModeArg::FunctionCall))
            .expect("overrides expected");
        assert_eq!(
            overrides["llm"]["models"][model_name]["tool_call_mode"],
            json!("function_call")
        );
        assert!(overrides["llm"]["models"][model_name]["max_rounds"].is_null());

        assert!(build_request_overrides(&config, None, None).is_none());
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
