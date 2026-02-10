use anyhow::{anyhow, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, error::TryRecvError, UnboundedReceiver};
use wunder_server::schemas::StreamEvent;

use crate::args::GlobalArgs;
use crate::runtime::CliRuntime;
use crate::slash_command::{self, ParsedSlashCommand, SlashCommand};

const MAX_LOG_ENTRIES: usize = 1200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Info,
    User,
    Assistant,
    Tool,
    Error,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub kind: LogKind,
    pub text: String,
}

enum StreamMessage {
    Event(StreamEvent),
    Error(String),
    Done,
}

#[derive(Debug, Clone, Default)]
struct ConfigWizardState {
    base_url: Option<String>,
    api_key: Option<String>,
}

pub struct TuiApp {
    runtime: CliRuntime,
    global: GlobalArgs,
    session_id: String,
    input: String,
    logs: Vec<LogEntry>,
    busy: bool,
    should_quit: bool,
    history: Vec<String>,
    history_cursor: Option<usize>,
    history_draft: String,
    active_assistant: Option<usize>,
    stream_rx: Option<UnboundedReceiver<StreamMessage>>,
    model_name: String,
    tool_call_mode: String,
    last_usage: Option<String>,
    config_wizard: Option<ConfigWizardState>,
    stream_saw_output: bool,
    stream_saw_final: bool,
}

impl TuiApp {
    pub async fn new(
        runtime: CliRuntime,
        global: GlobalArgs,
        session_override: Option<String>,
    ) -> Result<Self> {
        let session_id =
            session_override.unwrap_or_else(|| runtime.resolve_session(global.session.as_deref()));
        runtime.save_session(&session_id).ok();

        let mut app = Self {
            runtime,
            global,
            session_id,
            input: String::new(),
            logs: Vec::new(),
            busy: false,
            should_quit: false,
            history: Vec::new(),
            history_cursor: None,
            history_draft: String::new(),
            active_assistant: None,
            stream_rx: None,
            model_name: "<none>".to_string(),
            tool_call_mode: "tool_call".to_string(),
            last_usage: None,
            config_wizard: None,
            stream_saw_output: false,
            stream_saw_final: false,
        };
        app.sync_model_status().await;
        app.push_log(
            LogKind::Info,
            "wunder-cli tui mode. type /help for commands.".to_string(),
        );
        Ok(app)
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn status_line(&self) -> String {
        let busy = if self.busy { "working" } else { "idle" };
        let usage = self
            .last_usage
            .as_deref()
            .map(|value| format!(" usage:{value}"))
            .unwrap_or_default();
        format!(
            "wunder-cli  session:{}  model:{}  mode:{}  state:{}{}  (Ctrl+C exit)",
            short_session_id(&self.session_id),
            self.model_name,
            self.tool_call_mode,
            busy,
            usage,
        )
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn cursor_offset(&self) -> usize {
        self.input.chars().count()
    }

    pub fn visible_logs(&self, max_entries: usize) -> Vec<LogEntry> {
        let len = self.logs.len();
        if len <= max_entries {
            return self.logs.clone();
        }
        self.logs[len - max_entries..].to_vec()
    }

    pub fn popup_lines(&self) -> Vec<String> {
        let trimmed = self.input.trim_start();
        if !trimmed.starts_with('/') {
            return Vec::new();
        }
        let body = trimmed.trim_start_matches('/');
        if body.contains(char::is_whitespace) {
            return Vec::new();
        }
        slash_command::popup_lines(body, 7)
    }

    pub fn drain_stream_events(&mut self) {
        loop {
            let Some(receiver) = self.stream_rx.as_mut() else {
                break;
            };
            match receiver.try_recv() {
                Ok(message) => self.handle_stream_message(message),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.stream_rx = None;
                    self.busy = false;
                    self.active_assistant = None;
                    break;
                }
            }
        }
    }

    pub async fn on_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('d') => {
                    self.should_quit = true;
                    return Ok(());
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc => {
                self.input.clear();
                self.history_cursor = None;
            }
            KeyCode::Enter => {
                let raw_line = std::mem::take(&mut self.input);
                self.history_cursor = None;
                if self.config_wizard.is_some() {
                    self.submit_line(raw_line).await?;
                } else {
                    let line = raw_line.trim().to_string();
                    if !line.is_empty() {
                        self.submit_line(line).await?;
                    }
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Tab => {
                self.apply_first_suggestion();
            }
            KeyCode::Up => {
                self.history_up();
            }
            KeyCode::Down => {
                self.history_down();
            }
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.input.push(ch);
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub async fn submit_line(&mut self, line: String) -> Result<()> {
        let cleaned = line.trim().to_string();
        if self.config_wizard.is_some() {
            return self.handle_config_wizard_input(&cleaned).await;
        }
        if cleaned.is_empty() {
            return Ok(());
        }

        self.push_history(&cleaned);

        if cleaned.starts_with('/') {
            return self.handle_slash_command(cleaned).await;
        }

        if self.busy {
            self.push_log(
                LogKind::Error,
                "assistant is still running, wait for completion before sending a new prompt"
                    .to_string(),
            );
            return Ok(());
        }

        self.last_usage = None;
        self.stream_saw_output = false;
        self.stream_saw_final = false;
        self.push_log(LogKind::User, cleaned.clone());
        self.busy = true;
        self.active_assistant = None;

        let request =
            crate::build_wunder_request(&self.runtime, &self.global, &cleaned, &self.session_id)
                .await;
        let orchestrator = self.runtime.state.orchestrator.clone();
        let (tx, rx) = mpsc::unbounded_channel::<StreamMessage>();
        self.stream_rx = Some(rx);

        tokio::spawn(async move {
            match orchestrator.stream(request).await {
                Ok(mut stream) => {
                    while let Some(item) = stream.next().await {
                        let event = item.expect("infallible stream event");
                        if tx.send(StreamMessage::Event(event)).is_err() {
                            return;
                        }
                    }
                }
                Err(err) => {
                    let _ = tx.send(StreamMessage::Error(err.to_string()));
                }
            }
            let _ = tx.send(StreamMessage::Done);
        });

        Ok(())
    }

    fn apply_first_suggestion(&mut self) {
        let trimmed = self.input.trim_start();
        if !trimmed.starts_with('/') {
            return;
        }
        let body = trimmed.trim_start_matches('/');
        if body.contains(char::is_whitespace) {
            return;
        }
        if let Some(suggestion) = slash_command::first_command_completion(body) {
            self.input = format!("/{suggestion} ");
        }
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        if self.history_cursor.is_none() {
            self.history_draft = self.input.clone();
            self.history_cursor = Some(self.history.len().saturating_sub(1));
        } else if let Some(cursor) = self.history_cursor {
            self.history_cursor = Some(cursor.saturating_sub(1));
        }
        if let Some(cursor) = self.history_cursor {
            self.input = self.history.get(cursor).cloned().unwrap_or_default();
        }
    }

    fn history_down(&mut self) {
        let Some(cursor) = self.history_cursor else {
            return;
        };
        let next = cursor + 1;
        if next >= self.history.len() {
            self.history_cursor = None;
            self.input = self.history_draft.clone();
            return;
        }
        self.history_cursor = Some(next);
        self.input = self.history.get(next).cloned().unwrap_or_default();
    }

    fn push_history(&mut self, value: &str) {
        if self
            .history
            .last()
            .map(|existing| existing == value)
            .unwrap_or(false)
        {
            return;
        }
        self.history.push(value.to_string());
    }

    fn handle_stream_message(&mut self, message: StreamMessage) {
        match message {
            StreamMessage::Event(event) => self.apply_stream_event(event),
            StreamMessage::Error(err) => {
                self.push_log(LogKind::Error, err);
                self.busy = false;
                self.active_assistant = None;
                self.stream_rx = None;
                self.stream_saw_output = false;
                self.stream_saw_final = false;
            }
            StreamMessage::Done => {
                if !self.stream_saw_output && !self.stream_saw_final {
                    self.push_log(
                        LogKind::Error,
                        "stream ended without model output or final answer".to_string(),
                    );
                }
                self.busy = false;
                self.active_assistant = None;
                self.stream_rx = None;
                self.stream_saw_output = false;
                self.stream_saw_final = false;
            }
        }
    }

    fn apply_stream_event(&mut self, event: StreamEvent) {
        match event.event.as_str() {
            "llm_output_delta" => {
                if let Some(delta) = event.data.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        self.stream_saw_output = true;
                        let index = self.ensure_assistant_entry();
                        if let Some(entry) = self.logs.get_mut(index) {
                            entry.text.push_str(delta);
                        }
                    }
                }
            }
            "llm_output" => {
                if let Some(content) = event.data.get("content").and_then(Value::as_str) {
                    if !content.is_empty() {
                        self.stream_saw_output = true;
                        let index = self.ensure_assistant_entry();
                        if let Some(entry) = self.logs.get_mut(index) {
                            if entry.text.is_empty() {
                                entry.text = content.to_string();
                            }
                        }
                    }
                }
            }
            "progress" => {
                let stage = event
                    .data
                    .get("stage")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let summary = event
                    .data
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let message = format!("[progress] {stage} {summary}").trim().to_string();
                if !message.is_empty() {
                    self.push_log(LogKind::Info, message);
                }
            }
            "tool_call" => {
                let tool = event
                    .data
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let args = event
                    .data
                    .get("args")
                    .map(compact_json)
                    .unwrap_or_else(|| "{}".to_string());
                self.push_log(LogKind::Tool, format!("[tool_call] {tool} {args}"));
            }
            "tool_result" => {
                let tool = event
                    .data
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let result = event
                    .data
                    .get("result")
                    .map(compact_json)
                    .unwrap_or_else(|| compact_json(&event.data));
                self.push_log(LogKind::Tool, format!("[tool_result] {tool} {result}"));
            }
            "error" => {
                self.push_log(
                    LogKind::Error,
                    format!("[error] {}", parse_error_message(&event.data)),
                );
            }
            "final" => {
                self.stream_saw_final = true;
                let answer = event
                    .data
                    .get("answer")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !answer.is_empty() {
                    self.stream_saw_output = true;
                    let index = self.ensure_assistant_entry();
                    if let Some(entry) = self.logs.get_mut(index) {
                        if entry.text.trim().is_empty() {
                            entry.text = answer.to_string();
                        }
                    }
                }
                self.last_usage = event
                    .data
                    .get("usage")
                    .and_then(|value| value.get("total_tokens"))
                    .and_then(Value::as_u64)
                    .map(|total| total.to_string());
            }
            _ => {}
        }
    }

    async fn handle_slash_command(&mut self, line: String) -> Result<()> {
        let Some(command) = slash_command::parse_slash_command(&line) else {
            self.push_log(LogKind::Error, format!("unknown command: {line}"));
            self.push_log(
                LogKind::Info,
                "type /help to list available slash commands".to_string(),
            );
            return Ok(());
        };

        match command.command {
            SlashCommand::Help => {
                for help in slash_command::help_lines() {
                    self.push_log(LogKind::Info, help);
                }
            }
            SlashCommand::Status => {
                for line in self.status_lines() {
                    self.push_log(LogKind::Info, line);
                }
            }
            SlashCommand::Session => {
                self.push_log(
                    LogKind::Info,
                    format!("current session: {}", self.session_id),
                );
            }
            SlashCommand::New => {
                self.session_id = uuid::Uuid::new_v4().simple().to_string();
                self.runtime.save_session(&self.session_id).ok();
                self.push_log(
                    LogKind::Info,
                    format!("switched to session: {}", self.session_id),
                );
            }
            SlashCommand::Config => {
                self.apply_config_from_slash(command).await?;
            }
            SlashCommand::ConfigShow => {
                self.show_config_snapshot().await?;
            }
            SlashCommand::Model => {
                self.handle_model_slash(command.args).await?;
            }
            SlashCommand::ToolCallMode => {
                self.handle_tool_call_mode_slash(command.args).await?;
            }
            SlashCommand::Exit | SlashCommand::Quit => {
                self.should_quit = true;
            }
        }

        Ok(())
    }

    async fn show_config_snapshot(&mut self) -> Result<()> {
        let config = self.runtime.state.config_store.get().await;
        let model = self
            .runtime
            .resolve_model_name(self.global.model.as_deref())
            .await;
        let tool_call_mode = model
            .as_ref()
            .and_then(|name| config.llm.models.get(name))
            .and_then(|model| model.tool_call_mode.clone())
            .unwrap_or_else(|| "tool_call".to_string());

        let payload = json!({
            "launch_dir": self.runtime.launch_dir,
            "temp_root": self.runtime.temp_root,
            "user_id": self.runtime.user_id,
            "workspace_root": config.workspace.root,
            "storage_backend": config.storage.backend,
            "db_path": config.storage.db_path,
            "model": model,
            "tool_call_mode": tool_call_mode,
            "override_path": self.runtime.temp_root.join("config/wunder.override.yaml"),
        });

        for line in serde_json::to_string_pretty(&payload)?.lines() {
            self.push_log(LogKind::Info, line.to_string());
        }
        Ok(())
    }

    async fn apply_config_from_slash(&mut self, command: ParsedSlashCommand<'_>) -> Result<()> {
        let args = command.args.trim();
        if args.is_empty() {
            self.start_config_wizard();
            return Ok(());
        }

        let values =
            shell_words::split(args).map_err(|err| anyhow!("parse /config args failed: {err}"))?;
        if values.len() != 3 {
            self.push_log(
                LogKind::Error,
                "invalid /config args, expected: /config <base_url> <api_key> <model>".to_string(),
            );
            return Ok(());
        }

        let base_url = values[0].trim().to_string();
        let api_key = values[1].trim().to_string();
        let model_name = values[2].trim().to_string();
        if base_url.is_empty() || api_key.is_empty() || model_name.is_empty() {
            self.push_log(LogKind::Error, "config values cannot be empty".to_string());
            return Ok(());
        }

        self.apply_model_config(base_url, api_key, model_name).await
    }

    fn start_config_wizard(&mut self) {
        self.config_wizard = Some(ConfigWizardState::default());
        self.push_log(LogKind::Info, "configure llm model (step 1/3)".to_string());
        self.push_log(
            LogKind::Info,
            "input base_url (empty line to cancel)".to_string(),
        );
    }

    async fn handle_config_wizard_input(&mut self, input: &str) -> Result<()> {
        let cleaned = input.trim();
        if cleaned.is_empty()
            || cleaned.eq_ignore_ascii_case("/cancel")
            || cleaned.eq_ignore_ascii_case("/exit")
        {
            self.config_wizard = None;
            self.push_log(LogKind::Info, "config cancelled".to_string());
            return Ok(());
        }

        let Some(mut wizard) = self.config_wizard.take() else {
            return Ok(());
        };

        if wizard.base_url.is_none() {
            wizard.base_url = Some(cleaned.to_string());
            self.config_wizard = Some(wizard);
            self.push_log(LogKind::Info, "input api_key (step 2/3)".to_string());
            return Ok(());
        }

        if wizard.api_key.is_none() {
            wizard.api_key = Some(cleaned.to_string());
            self.config_wizard = Some(wizard);
            self.push_log(LogKind::Info, "input model name (step 3/3)".to_string());
            return Ok(());
        }

        let base_url = wizard.base_url.unwrap_or_default();
        let api_key = wizard.api_key.unwrap_or_default();
        let model_name = cleaned.to_string();
        self.apply_model_config(base_url, api_key, model_name).await
    }

    async fn apply_model_config(
        &mut self,
        base_url: String,
        api_key: String,
        model_name: String,
    ) -> Result<()> {
        self.config_wizard = None;
        let provider = crate::infer_provider_from_base_url(&base_url);
        let model_for_update = model_name.clone();
        let provider_for_update = provider.clone();
        let base_url_for_update = base_url.clone();
        let api_key_for_update = api_key.clone();

        self.runtime
            .state
            .config_store
            .update(move |config| {
                let entry = config
                    .llm
                    .models
                    .entry(model_for_update.clone())
                    .or_insert_with(|| {
                        crate::build_cli_llm_model_config(
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

        self.sync_model_status().await;
        self.push_log(LogKind::Info, "model configured".to_string());
        self.push_log(LogKind::Info, format!("- provider: {provider}"));
        self.push_log(LogKind::Info, format!("- base_url: {base_url}"));
        self.push_log(LogKind::Info, format!("- model: {model_name}"));
        self.push_log(LogKind::Info, "- tool_call_mode: tool_call".to_string());
        Ok(())
    }

    async fn handle_model_slash(&mut self, args: &str) -> Result<()> {
        let target = args.trim();
        if target.is_empty() {
            self.show_model_status().await;
            return Ok(());
        }

        let config = self.runtime.state.config_store.get().await;
        if !config.llm.models.contains_key(target) {
            self.push_log(LogKind::Error, format!("model not found: {target}"));
            let models = crate::sorted_model_names(&config);
            if models.is_empty() {
                self.push_log(
                    LogKind::Info,
                    "no models configured. run /config first.".to_string(),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    format!("available models: {}", models.join(", ")),
                );
            }
            return Ok(());
        }

        let target_name = target.to_string();
        self.runtime
            .state
            .config_store
            .update(move |config| {
                config.llm.default = target_name.clone();
            })
            .await?;

        self.sync_model_status().await;
        self.push_log(LogKind::Info, format!("model set: {target}"));
        self.show_model_status().await;
        Ok(())
    }

    async fn show_model_status(&mut self) {
        let config = self.runtime.state.config_store.get().await;
        let active_model = self
            .runtime
            .resolve_model_name(self.global.model.as_deref())
            .await
            .unwrap_or_else(|| "<none>".to_string());
        self.push_log(LogKind::Info, format!("current model: {active_model}"));

        let models = crate::sorted_model_names(&config);
        if models.is_empty() {
            self.push_log(
                LogKind::Info,
                "no models configured. run /config first.".to_string(),
            );
            return;
        }

        self.push_log(LogKind::Info, "available models:".to_string());
        for name in models {
            let marker = if name == active_model { "*" } else { " " };
            let mode = config
                .llm
                .models
                .get(&name)
                .and_then(|model| model.tool_call_mode.as_deref())
                .unwrap_or("tool_call");
            self.push_log(LogKind::Info, format!("{marker} {name} ({mode})"));
        }
    }

    async fn handle_tool_call_mode_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.is_empty() {
            self.push_log(
                LogKind::Info,
                format!(
                    "tool_call_mode: model={} mode={}",
                    self.model_name, self.tool_call_mode
                ),
            );
            self.push_log(
                LogKind::Info,
                "usage: /tool-call-mode <tool_call|function_call> [model]".to_string(),
            );
            return Ok(());
        }

        let mut parts = cleaned.split_whitespace();
        let Some(mode_token) = parts.next() else {
            return Ok(());
        };
        let Some(mode) = crate::parse_tool_call_mode(mode_token) else {
            self.push_log(LogKind::Error, format!("invalid mode: {mode_token}"));
            self.push_log(
                LogKind::Info,
                "valid modes: tool_call, function_call".to_string(),
            );
            return Ok(());
        };

        let model = parts.next().map(str::to_string);
        if parts.next().is_some() {
            self.push_log(LogKind::Error, "too many arguments".to_string());
            self.push_log(
                LogKind::Info,
                "usage: /tool-call-mode <tool_call|function_call> [model]".to_string(),
            );
            return Ok(());
        }

        let config = self.runtime.state.config_store.get().await;
        let target_model = if let Some(model_name) = model {
            if !config.llm.models.contains_key(&model_name) {
                self.push_log(
                    LogKind::Error,
                    format!("model not found in config: {model_name}"),
                );
                return Ok(());
            }
            model_name
        } else {
            self.runtime
                .resolve_model_name(self.global.model.as_deref())
                .await
                .ok_or_else(|| anyhow!("no llm model configured"))?
        };

        let mode_text = mode.as_str().to_string();
        let target_model_for_update = target_model.clone();
        self.runtime
            .state
            .config_store
            .update(move |config| {
                if config.llm.default.trim().is_empty() {
                    config.llm.default = target_model_for_update.clone();
                }
                if let Some(entry) = config.llm.models.get_mut(&target_model_for_update) {
                    entry.tool_call_mode = Some(mode_text.clone());
                }
            })
            .await?;

        self.sync_model_status().await;
        self.push_log(
            LogKind::Info,
            format!(
                "tool_call_mode set: model={target_model} mode={}",
                mode.as_str()
            ),
        );
        Ok(())
    }

    async fn sync_model_status(&mut self) {
        let config = self.runtime.state.config_store.get().await;
        self.model_name = self
            .runtime
            .resolve_model_name(self.global.model.as_deref())
            .await
            .unwrap_or_else(|| "<none>".to_string());
        self.tool_call_mode = config
            .llm
            .models
            .get(&self.model_name)
            .and_then(|model| model.tool_call_mode.clone())
            .unwrap_or_else(|| "tool_call".to_string());
    }

    fn status_lines(&self) -> Vec<String> {
        vec![
            "status".to_string(),
            format!("- session: {}", self.session_id),
            format!("- model: {}", self.model_name),
            format!("- tool_call_mode: {}", self.tool_call_mode),
            format!("- workspace: {}", self.runtime.launch_dir.to_string_lossy()),
            format!("- temp_root: {}", self.runtime.temp_root.to_string_lossy()),
        ]
    }

    fn ensure_assistant_entry(&mut self) -> usize {
        if let Some(index) = self.active_assistant {
            return index;
        }
        let index = self.push_log(LogKind::Assistant, String::new());
        self.active_assistant = Some(index);
        index
    }

    fn push_log(&mut self, kind: LogKind, text: String) -> usize {
        self.logs.push(LogEntry { kind, text });
        if self.logs.len() > MAX_LOG_ENTRIES {
            self.logs.remove(0);
            if let Some(index) = self.active_assistant.as_mut() {
                *index = index.saturating_sub(1);
            }
        }
        self.logs.len().saturating_sub(1)
    }
}

fn short_session_id(session_id: &str) -> String {
    session_id.chars().take(8).collect()
}

fn parse_error_message(data: &Value) -> String {
    let nested_message = data
        .get("data")
        .and_then(Value::as_object)
        .and_then(|inner| inner.get("message"))
        .and_then(Value::as_str);
    data.as_str()
        .or_else(|| data.get("message").and_then(Value::as_str))
        .or_else(|| data.get("detail").and_then(Value::as_str))
        .or_else(|| data.get("error").and_then(Value::as_str))
        .or(nested_message)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| compact_json(data))
}

fn compact_json(value: &Value) -> String {
    const MAX_INLINE_JSON_CHARS: usize = 200;
    let mut text = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    if text.len() > MAX_INLINE_JSON_CHARS {
        text.truncate(MAX_INLINE_JSON_CHARS);
        text.push_str("...");
    }
    text
}
