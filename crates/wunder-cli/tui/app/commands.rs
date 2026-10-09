use super::*;

/// The footer/status facts, read from the engine config in one pass. `&CliRuntime`
/// is enough, which is what lets the first frame skip this entirely.
pub(super) async fn compute_model_status(
    runtime: &crate::runtime::CliRuntime,
    requested_model: Option<&str>,
    approval_flag: Option<crate::args::ApprovalModeArg>,
) -> ModelStatusSnapshot {
    let config = runtime.state.config_store.get().await;
    let approval_mode = approval_flag
        .map(|mode| mode.as_str().to_string())
        .or_else(|| {
            config
                .security
                .approval_mode
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "full_auto".to_string());
    let model_name = runtime
        .resolve_model_name(requested_model)
        .await
        .unwrap_or_else(|| "<none>".to_string());
    let model_entry = config.llm.models.get(&model_name);
    ModelStatusSnapshot {
        model_name,
        tool_call_mode: crate::runtime::effective_tool_call_mode(model_entry).to_string(),
        reasoning_effort: model_entry
            .and_then(|model| model.reasoning_effort.clone())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| runtime.user_config.values.model_reasoning_effort.clone())
            .unwrap_or_else(|| DEFAULT_REASONING_EFFORT.to_string()),
        approval_mode,
        max_context: Some(crate::runtime::effective_max_context(model_entry)),
        max_rounds: model_entry
            .and_then(|model| model.max_rounds)
            .unwrap_or(crate::CLI_MIN_MAX_ROUNDS)
            .max(crate::CLI_MIN_MAX_ROUNDS),
    }
}

/// Everything the popup catalogs need, read from config and the local stores.
pub(super) async fn load_popup_catalog(
    runtime: &crate::runtime::CliRuntime,
) -> (Vec<String>, Vec<String>, HashSet<String>) {
    let payload = runtime
        .state
        .user_tool_store
        .load_user_tools(&runtime.user_id);
    let enabled_skill_names = payload
        .skills
        .enabled
        .into_iter()
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect::<HashSet<_>>();
    let mut app_hints = Vec::new();
    for server in payload.mcp_servers {
        let name = server.name.trim();
        if !name.is_empty() {
            app_hints.push(format!("${name}"));
        }
    }
    app_hints.sort_by_key(|value| value.to_ascii_lowercase());
    app_hints.dedup_by(|left, right| left.eq_ignore_ascii_case(right));

    let (_, specs) = crate::load_user_skill_specs(runtime).await;
    let mut skill_hints = specs
        .into_iter()
        .map(|spec| format!("#{}", spec.name))
        .collect::<Vec<_>>();
    skill_hints.sort_by_key(|value| value.to_ascii_lowercase());
    skill_hints.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    (app_hints, skill_hints, enabled_skill_names)
}

/// The reasoning-effort words the ladder accepts, so a config typo cannot
/// silently become a model name.
pub(super) fn is_reasoning_effort_word(value: &str) -> bool {
    REASONING_EFFORT_LADDER
        .iter()
        .any(|word| word.eq_ignore_ascii_case(value.trim()))
}

/// One notch up (`delta > 0`) or down (`delta < 0`) the ladder. `None` means
/// the value is already at that end, so the caller can say so instead of
/// silently doing nothing. An unrecognized current value starts from the
/// default rung, which keeps `Alt+.` useful after a hand-edited config.
pub(super) fn next_reasoning_effort(current: &str, delta: isize) -> Option<&'static str> {
    let index = REASONING_EFFORT_LADDER
        .iter()
        .position(|word| word.eq_ignore_ascii_case(current.trim()))
        .unwrap_or_else(|| {
            REASONING_EFFORT_LADDER
                .iter()
                .position(|word| *word == DEFAULT_REASONING_EFFORT)
                .unwrap_or(0)
        });
    let next = if delta < 0 {
        index.checked_sub(1)?
    } else {
        let candidate = index + 1;
        if candidate >= REASONING_EFFORT_LADDER.len() {
            return None;
        }
        candidate
    };
    Some(REASONING_EFFORT_LADDER[next])
}

impl TuiApp {
    /// `!<command>`: run one shell command without a model turn. The engine's
    /// `execute_command` tool does the work, so the workspace boundary, the
    /// approval policy and the command log are exactly a tool call's.
    pub(super) async fn run_direct_shell_command(&mut self, command: String, echo: String) {
        let config = self.runtime.state.config_store.get().await;
        if !crate::direct_shell_is_allowed(config.security.approval_mode.as_deref()) {
            let policy = crate::args::ApprovalModeArg::from_engine_mode(
                config.security.approval_mode.as_deref().unwrap_or(""),
            );
            let word = policy.policy_word();
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    &format!("当前审批策略为 {word}，直接执行命令需要批准；用 /permissions never 放开，或用 /permissions 查看当前策略"),
                    &format!("the current approval policy is {word}, so a directly executed command needs approval; run /permissions never to open it, or /permissions to review"),
                ),
            );
            return;
        }
        drop(config);

        if !echo.trim().is_empty() {
            self.push_log(LogKind::User, echo);
        }
        self.push_log(LogKind::Tool, format!("$ {command}"));
        let args = serde_json::json!({ "content": command });
        let result = crate::run_tool_outside_turn(
            &self.runtime,
            self.session_id.as_str(),
            "execute_command",
            args,
        )
        .await;

        match result {
            Ok(payload) => {
                let is_zh = self.is_zh_language();
                match crate::tool_display::summarize_tool_result(&payload, is_zh) {
                    Some(summary) => {
                        if let Some(text) = summary.summary.filter(|text| !text.trim().is_empty()) {
                            self.push_log(LogKind::Tool, text);
                        }
                        for line in summary.details {
                            let text = match line.label {
                                Some(label) => format!("{label}: {}", line.text),
                                None => line.text,
                            };
                            self.push_log(LogKind::Tool, text);
                        }
                    }
                    None => {
                        let text = payload
                            .get("detail")
                            .or_else(|| payload.get("content"))
                            .and_then(|value| value.as_str())
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if !text.is_empty() {
                            self.push_log(LogKind::Tool, text);
                        }
                    }
                }
            }
            Err(err) => {
                self.push_log(LogKind::Error, err.to_string());
            }
        }
        self.reload_session_stats().await;
    }

    pub(super) async fn handle_slash_command(&mut self, line: String) -> Result<()> {
        let Some(command) = slash_command::parse_slash_command(&line) else {
            self.push_log(LogKind::Error, format!("unknown command: {line}"));
            self.push_log(
                LogKind::Info,
                "type /help to list available slash commands".to_string(),
            );
            return Ok(());
        };

        if self.thread_is_running(self.session_id.as_str())
            && !command.command.available_during_task()
        {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "助手仍在运行，该命令需等待当前轮次完成后再执行",
                    "assistant is still running; wait for the current turn to finish before running this command",
                ),
            );
            return Ok(());
        }

        match command.command {
            SlashCommand::Help => {
                for help in slash_command::help_lines_with_language(self.display_language.as_str())
                {
                    self.push_log(LogKind::Info, help);
                }
            }
            SlashCommand::Status => {
                self.reload_session_stats().await;
                for line in self.status_lines() {
                    self.push_log(LogKind::Info, line);
                }
            }
            SlashCommand::Resume => {
                self.handle_resume_slash(command.args).await?;
            }
            SlashCommand::New => {
                // Deliberately not gated by `busy`: a fresh thread does not disturb the
                // running one, it only moves it to the background.
                self.switch_to_new_session().await;
            }
            SlashCommand::Clear => {
                self.clear_transcript();
                self.switch_to_new_session().await;
            }
            SlashCommand::Config => {
                self.apply_config_from_slash(command).await?;
            }
            SlashCommand::Model => {
                self.handle_model_slash(command.args).await?;
            }
            SlashCommand::Permissions => {
                self.handle_permissions_slash(command.args).await?;
            }
            SlashCommand::Plan => {
                self.handle_plan_slash(command.args).await?;
            }
            SlashCommand::Goal => {
                self.handle_goal_slash(command.args).await?;
            }
            SlashCommand::Edit => {
                self.handle_edit_slash(command.args)?;
            }
            SlashCommand::Init => {
                self.handle_init_slash(command.args)?;
            }
            SlashCommand::Attach => {
                self.handle_attach_slash(command.args).await?;
            }
            SlashCommand::Diff => {
                self.handle_diff_slash(command.args).await?;
            }
            SlashCommand::Review => {
                self.handle_review_slash(command.args).await?;
            }
            SlashCommand::Skills => {
                self.handle_skills_slash(command.args).await?;
            }
            SlashCommand::Rename => {
                self.handle_rename_slash(command.args).await?;
            }
            SlashCommand::Compact => {
                self.handle_compact_slash().await?;
            }
            SlashCommand::Mcp => {
                self.handle_mcp_slash(command.args).await?;
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
        let model_entry = model.as_ref().and_then(|name| config.llm.models.get(name));
        let tool_call_mode = crate::runtime::effective_tool_call_mode(model_entry).to_string();
        let max_context = Some(crate::runtime::effective_max_context(model_entry));

        self.reload_session_stats().await;

        let payload = json!({
            "launch_dir": self.runtime.launch_dir,
            "temp_root": self.runtime.temp_root,
            "user_id": self.runtime.user_id,
            "queued_attachments": self.pending_attachments.len(),
            "turn_notification": crate::serialize_turn_notification(
                &self.runtime.load_turn_notification_config()
            ),
            "workspace_root": config.workspace.root,
            "storage_backend": config.storage.backend,
            "db_path": config.storage.db_path,
            "model": model,
            "tool_call_mode": tool_call_mode,
            "approval_mode": self.approval_mode,
            "max_rounds": self.model_max_rounds,
            "max_context": max_context,
            "context_used": self.session_stats.context_used_tokens.max(0),
            "context_left_percent": crate::context_left_percent(
                self.session_stats.context_used_tokens,
                max_context,
            ),
            "config_path": std::env::var("WUNDER_CONFIG_PATH").unwrap_or_default(),
        });

        for line in serde_json::to_string_pretty(&payload)?.lines() {
            self.push_config_log(line.to_string());
        }
        Ok(())
    }

    fn push_config_log(&mut self, text: impl Into<String>) {
        self.push_log(LogKind::Tool, text.into());
    }

    async fn apply_config_from_slash(&mut self, command: ParsedSlashCommand<'_>) -> Result<()> {
        let args = command.args.trim();
        if args.is_empty() {
            self.start_config_wizard();
            return Ok(());
        }

        // `show` / `edit` are the sub-verbs of `/config`; a bare `/config`
        // stays the model wizard. Anything else is the one-line endpoint form.
        if args.eq_ignore_ascii_case("show") {
            self.show_config_snapshot().await?;
            return Ok(());
        }
        if args.eq_ignore_ascii_case("edit") {
            self.edit_user_config_file().await?;
            return Ok(());
        }

        let values =
            shell_words::split(args).map_err(|err| anyhow!("parse /config args failed: {err}"))?;
        if values.len() < 3 || values.len() > 4 {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "/config 参数不正确，应为：/config <base_url> <api_key> <model> [max_context]",
                    "invalid /config args, expected: /config <base_url> <api_key> <model> [max_context]",
                ),
            );
            return Ok(());
        }

        let base_url = values[0].trim().to_string();
        let api_key = values[1].trim().to_string();
        let model_name = values[2].trim().to_string();
        if base_url.is_empty() || api_key.is_empty() || model_name.is_empty() {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "配置项不能为空",
                    "config values cannot be empty",
                ),
            );
            return Ok(());
        }

        let manual_max_context = if values.len() == 4 {
            match crate::parse_optional_max_context_value(values[3].as_str()) {
                Ok(value) => value,
                Err(err) => {
                    self.push_log(LogKind::Error, err.to_string());
                    return Ok(());
                }
            }
        } else {
            None
        };

        self.apply_model_config(base_url, api_key, model_name, manual_max_context)
            .await
    }

    fn start_config_wizard(&mut self) {
        self.config_wizard = Some(ConfigWizardState::default());
        self.push_config_log(crate::locale::tr(
            self.display_language.as_str(),
            "配置模型（第 1/4 步）",
            "configure llm model (step 1/4)",
        ));
        self.push_config_log(crate::locale::tr(
            self.display_language.as_str(),
            "请输入 base_url（留空可取消）",
            "input base_url (empty line to cancel)",
        ));
    }

    pub(super) fn cancel_config_wizard(&mut self) -> bool {
        if self.config_wizard.take().is_none() {
            return false;
        }
        self.input.clear();
        self.input_cursor = 0;
        self.pending_paste.clear();
        self.pending_large_pastes.clear();
        self.history_cursor = None;
        self.push_config_log(crate::locale::tr(
            self.display_language.as_str(),
            "已取消配置",
            "config cancelled",
        ));
        true
    }

    pub(super) async fn handle_config_wizard_input(&mut self, input: &str) -> Result<()> {
        let cleaned = input.trim();
        if cleaned.eq_ignore_ascii_case("/cancel") || cleaned.eq_ignore_ascii_case("/exit") {
            self.cancel_config_wizard();
            return Ok(());
        }

        let Some(mut wizard) = self.config_wizard.take() else {
            return Ok(());
        };

        if wizard.base_url.is_none() {
            if cleaned.is_empty() {
                self.cancel_config_wizard();
                return Ok(());
            }
            wizard.base_url = Some(cleaned.to_string());
            self.config_wizard = Some(wizard);
            self.push_config_log(crate::locale::tr(
                self.display_language.as_str(),
                "请输入 api_key（第 2/4 步）",
                "input api_key (step 2/4)",
            ));
            return Ok(());
        }

        if wizard.api_key.is_none() {
            if cleaned.is_empty() {
                self.cancel_config_wizard();
                return Ok(());
            }
            wizard.api_key = Some(cleaned.to_string());
            self.config_wizard = Some(wizard);
            self.push_config_log(crate::locale::tr(
                self.display_language.as_str(),
                "请输入模型名（第 3/4 步）",
                "input model name (step 3/4)",
            ));
            return Ok(());
        }

        if wizard.model_name.is_none() {
            if cleaned.is_empty() {
                self.cancel_config_wizard();
                return Ok(());
            }
            wizard.model_name = Some(cleaned.to_string());
            self.config_wizard = Some(wizard);
            self.push_config_log(crate::locale::tr(
                self.display_language.as_str(),
                "请输入 max_context（第 4/4 步，可选；直接回车自动探测）",
                "input max_context (step 4/4, optional; Enter for auto probe)",
            ));
            return Ok(());
        }

        let manual_max_context = match crate::parse_optional_max_context_value(cleaned) {
            Ok(value) => value,
            Err(err) => {
                self.push_log(LogKind::Error, err.to_string());
                self.config_wizard = Some(wizard);
                self.push_config_log(crate::locale::tr(
                    self.display_language.as_str(),
                    "请输入 max_context（第 4/4 步，可选；直接回车自动探测）",
                    "input max_context (step 4/4, optional; Enter for auto probe)",
                ));
                return Ok(());
            }
        };

        let base_url = wizard.base_url.unwrap_or_default();
        let api_key = wizard.api_key.unwrap_or_default();
        let model_name = wizard.model_name.unwrap_or_default();
        self.apply_model_config(base_url, api_key, model_name, manual_max_context)
            .await
    }

    async fn apply_model_config(
        &mut self,
        base_url: String,
        api_key: String,
        model_name: String,
        manual_max_context: Option<u32>,
    ) -> Result<()> {
        self.config_wizard = None;
        let (provider, resolved_max_context) = crate::apply_cli_model_config(
            &self.runtime,
            &base_url,
            &api_key,
            &model_name,
            manual_max_context,
            self.display_language.as_str(),
        )
        .await?;

        self.sync_model_status().await;
        self.reload_session_stats().await;
        self.push_config_log(crate::locale::tr(
            self.display_language.as_str(),
            "模型配置完成",
            "model configured",
        ));
        if self.is_zh_language() {
            self.push_config_log(format!("- 服务商: {provider}"));
            self.push_config_log(format!("- base_url: {base_url}"));
            self.push_config_log(format!("- 模型: {model_name}"));
        } else {
            self.push_config_log(format!("- provider: {provider}"));
            self.push_config_log(format!("- base_url: {base_url}"));
            self.push_config_log(format!("- model: {model_name}"));
        }
        if let Some(value) = resolved_max_context {
            if self.is_zh_language() {
                self.push_config_log(format!("- 最大上下文: {value}"));
            } else {
                self.push_config_log(format!("- max_context: {value}"));
            }
        } else {
            self.push_config_log(crate::locale::tr(
                self.display_language.as_str(),
                "- 最大上下文: 自动探测不可用（或保持原配置）",
                "- max_context: auto probe unavailable (or keep existing)",
            ));
        }
        let configured_mode = {
            let config = self.runtime.state.config_store.get().await;
            crate::runtime::effective_tool_call_mode(config.llm.models.get(&model_name))
        };
        if self.is_zh_language() {
            self.push_config_log(format!("- 工具调用模式: {configured_mode}"));
        } else {
            self.push_config_log(format!("- tool_call_mode: {configured_mode}"));
        }
        Ok(())
    }

    async fn handle_resume_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        let all_workspaces = cleaned
            .split_whitespace()
            .any(|token| token.eq_ignore_ascii_case("--all") || token.eq_ignore_ascii_case("all"));
        let cleaned = cleaned
            .split_whitespace()
            .filter(|token| {
                !token.eq_ignore_ascii_case("--all") && !token.eq_ignore_ascii_case("all")
            })
            .collect::<Vec<_>>()
            .join(" ");
        let cleaned = cleaned.trim();
        let workspace = if all_workspaces {
            None
        } else {
            Some(self.runtime.workspace_id())
        };

        if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("list") {
            self.open_resume_picker(all_workspaces).await?;
            if self.resume_picker.is_some() {
                self.push_log(
                    LogKind::Info,
                    "resume picker opened (Up/Down to choose, Enter to resume, Esc to cancel)"
                        .to_string(),
                );
            }
            return Ok(());
        }

        let target = if cleaned.eq_ignore_ascii_case("last") {
            let sessions =
                crate::list_recent_sessions_in(&self.runtime, 1, None, workspace).await?;
            sessions
                .first()
                .map(|item| item.session_id.clone())
                .ok_or_else(|| {
                    anyhow!(crate::locale::tr(
                        self.display_language.as_str(),
                        "当前工作区没有历史线程",
                        "no thread recorded in this workspace",
                    ))
                })?
        } else if let Ok(index) = cleaned.parse::<usize>() {
            let sessions =
                crate::list_recent_sessions_in(&self.runtime, 40, None, workspace).await?;
            let Some(item) = sessions.get(index.saturating_sub(1)) else {
                self.push_log(
                    LogKind::Error,
                    format!("session index out of range: {index}"),
                );
                return Ok(());
            };
            item.session_id.clone()
        } else {
            cleaned.to_string()
        };

        self.resume_to_session(target.as_str()).await
    }

    async fn handle_model_slash(&mut self, args: &str) -> Result<()> {
        let target = args.trim();
        if target.is_empty() {
            self.show_model_status().await;
            return Ok(());
        }

        // `/model <name> [effort]` and `/model <effort>`: a token that names a
        // configured model is a model switch, an effort word sets the strength.
        let tokens =
            shell_words::split(target).map_err(|err| anyhow!("parse /model args failed: {err}"))?;
        let config = self.runtime.state.config_store.get().await;
        let first_is_model = config.llm.models.contains_key(target);
        if !first_is_model && tokens.len() <= 1 && is_reasoning_effort_word(target) {
            self.set_reasoning_effort(target).await;
            return Ok(());
        }
        if !first_is_model {
            if tokens.len() >= 2 && config.llm.models.contains_key(tokens[0].as_str()) {
                let effort = tokens[1].clone();
                self.switch_model(tokens[0].clone()).await?;
                self.set_reasoning_effort(effort.as_str()).await;
                return Ok(());
            }
            if self.is_zh_language() {
                self.push_log(LogKind::Error, format!("模型不存在: {target}"));
            } else {
                self.push_log(LogKind::Error, format!("model not found: {target}"));
            }
            let models = crate::sorted_model_names(&config);
            if models.is_empty() {
                self.push_log(
                    LogKind::Info,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        "尚未配置模型，请先运行 /config",
                        "no models configured. run /config first.",
                    ),
                );
            } else if self.is_zh_language() {
                self.push_log(LogKind::Info, format!("可用模型: {}", models.join(", ")));
            } else {
                self.push_log(
                    LogKind::Info,
                    format!("available models: {}", models.join(", ")),
                );
            }
            return Ok(());
        }

        self.switch_model(target.to_string()).await?;
        if let Some(effort) = tokens.get(1) {
            self.set_reasoning_effort(effort.as_str()).await;
        }
        self.show_model_status().await;
        Ok(())
    }

    async fn switch_model(&mut self, target: String) -> Result<()> {
        let target_name = target.clone();
        self.runtime
            .state
            .config_store
            .update(move |config| {
                config.llm.default = target_name.clone();
            })
            .await?;

        self.sync_model_status().await;
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("模型已切换: {target}"));
        } else {
            self.push_log(LogKind::Info, format!("model set: {target}"));
        }
        Ok(())
    }

    /// Set the reasoning effort for the active model. The engine config is
    /// regenerated on every start, so the durable copy is `config.toml`.
    async fn set_reasoning_effort(&mut self, effort: &str) {
        let cleaned = effort.trim().to_ascii_lowercase();
        if !is_reasoning_effort_word(cleaned.as_str()) {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    &format!("非法推理强度: {effort}（可选 low, medium, high）"),
                    &format!("invalid reasoning effort: {effort} (low, medium, high)"),
                ),
            );
            return;
        }

        let model = self.model_name.clone();
        let value = cleaned.clone();
        self.runtime
            .state
            .config_store
            .update(move |config| {
                let target = if config.llm.models.contains_key(model.as_str()) {
                    model.clone()
                } else {
                    config.llm.default.trim().to_string()
                };
                if !target.is_empty() {
                    config
                        .llm
                        .models
                        .entry(target)
                        .or_default()
                        .reasoning_effort = Some(value);
                }
            })
            .await
            .ok();

        self.reasoning_effort = cleaned.clone();
        self.persist_user_config_value("model_reasoning_effort", cleaned.as_str());
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("推理强度: {cleaned}"));
        } else {
            self.push_log(LogKind::Info, format!("reasoning effort: {cleaned}"));
        }
    }

    /// `Alt+,` / `Alt+.`: step the reasoning effort one notch, clamped at the ends.
    pub(super) async fn step_reasoning_effort(&mut self, delta: isize) {
        match next_reasoning_effort(self.reasoning_effort.as_str(), delta) {
            Some(next) => self.set_reasoning_effort(next).await,
            None => {
                let current = self.reasoning_effort.clone();
                self.push_log(
                    LogKind::Info,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        &format!("推理强度已是边界值: {current}"),
                        &format!("reasoning effort is already at its limit: {current}"),
                    ),
                );
            }
        }
    }

    async fn show_model_status(&mut self) {
        let config = self.runtime.state.config_store.get().await;
        let active_model = self
            .runtime
            .resolve_model_name(self.global.model.as_deref())
            .await
            .unwrap_or_else(|| "<none>".to_string());
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("当前模型: {active_model}"));
        } else {
            self.push_log(LogKind::Info, format!("current model: {active_model}"));
        }

        let models = crate::sorted_model_names(&config);
        if models.is_empty() {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "尚未配置模型，请先运行 /config",
                    "no models configured. run /config first.",
                ),
            );
            return;
        }

        self.push_log(
            LogKind::Info,
            crate::locale::tr(
                self.display_language.as_str(),
                "可用模型：",
                "available models:",
            ),
        );
        for name in models {
            let marker = if name == active_model { "*" } else { " " };
            // Cloud entries (plan §6.2) carry a marker so the selection list
            // separates the cloud channel from the local models at a glance.
            let cloud_marker = if name.starts_with(wunder_server::cloud::CLOUD_MODEL_PREFIX) {
                if self.is_zh_language() {
                    "[云] "
                } else {
                    "[cloud] "
                }
            } else {
                ""
            };
            let mode = crate::runtime::effective_tool_call_mode(
                config.llm.models.get(&name),
            );
            self.push_log(
                LogKind::Info,
                format!("{marker} {cloud_marker}{name} ({mode})"),
            );
        }
    }

    async fn handle_permissions_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("show") {
            for line in self.permission_lines() {
                self.push_log(LogKind::Info, line);
            }
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /permissions [never|on-request|suggest|read-only|workspace-write|danger-full-access]",
                    "usage: /permissions [never|on-request|suggest|read-only|workspace-write|danger-full-access]",
                ),
            );
            return Ok(());
        }

        if let Some(sandbox) = crate::args::SandboxModeArg::from_word(cleaned) {
            self.set_sandbox_mode(sandbox).await;
            return Ok(());
        }

        let Some(policy) = crate::parse_approval_mode(cleaned) else {
            if self.is_zh_language() {
                self.push_log(LogKind::Error, format!("非法权限策略: {cleaned}"));
            } else {
                self.push_log(
                    LogKind::Error,
                    format!("invalid permission policy: {cleaned}"),
                );
            }
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "可选审批: never, on-request, suggest；可选沙箱: read-only, workspace-write, danger-full-access",
                    "approval: never, on-request, suggest; sandbox: read-only, workspace-write, danger-full-access",
                ),
            );
            return Ok(());
        };

        let mode_text = policy.as_str().to_string();
        self.runtime
            .state
            .config_store
            .update(move |config| {
                config.security.approval_mode = Some(mode_text.clone());
            })
            .await?;
        // The engine config is regenerated on every start, so the durable copy
        // of this choice belongs in the user's config.toml.
        self.persist_user_config_value("approval_policy", policy.policy_word());
        self.sync_model_status().await;
        if self.is_zh_language() {
            self.push_log(
                LogKind::Info,
                format!("审批策略已设置: {}", policy.policy_word()),
            );
        } else {
            self.push_log(
                LogKind::Info,
                format!("approval policy set: {}", policy.policy_word()),
            );
        }
        Ok(())
    }

    /// The two knobs `/permissions` reports, in the codex words the user typed.
    fn permission_lines(&self) -> Vec<String> {
        let config_sandbox = self
            .runtime
            .user_config
            .values
            .sandbox_mode
            .clone()
            .unwrap_or_else(|| "workspace-write".to_string());
        let policy = crate::args::ApprovalModeArg::from_engine_mode(self.approval_mode.as_str());
        let workspace = self.runtime.launch_dir.to_string_lossy().to_string();
        if self.is_zh_language() {
            vec![
                "权限".to_string(),
                format!("- 沙箱: {config_sandbox}"),
                format!("- 审批策略: {}", policy.policy_word()),
                format!("- 工作区边界: {workspace}"),
            ]
        } else {
            vec![
                "permissions".to_string(),
                format!("- sandbox: {config_sandbox}"),
                format!("- approval_policy: {}", policy.policy_word()),
                format!("- workspace boundary: {workspace}"),
            ]
        }
    }

    async fn set_sandbox_mode(&mut self, sandbox: crate::args::SandboxModeArg) {
        if sandbox == crate::args::SandboxModeArg::DangerFullAccess {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "危险：danger-full-access 关闭工作区边界，本机任意路径都可读写",
                    "danger: danger-full-access removes the workspace boundary, every path on this machine becomes writable",
                ),
            );
        }

        let mut config = self.runtime.state.config_store.get().await;
        // A policy the user stated - on the command line or in config.toml -
        // owns the approval dimension; the sandbox word then only moves the
        // boundary. Only the implicit CLI default lets read-only set the gate.
        let approval_is_explicit = self.global.approval_mode.is_some()
            || self.runtime.user_config.values.approval_policy.is_some();
        crate::runtime::apply_sandbox_mode(&mut config, sandbox, approval_is_explicit);
        let approval_mode = config.security.approval_mode.clone();
        let allow_paths = config.security.allow_paths.clone();
        self.runtime
            .state
            .config_store
            .update(move |config| {
                config.security.approval_mode = approval_mode;
                config.security.allow_paths = allow_paths;
            })
            .await
            .ok();

        self.persist_user_config_value("sandbox_mode", sandbox.as_str());
        self.approval_mode = self
            .runtime
            .state
            .config_store
            .get()
            .await
            .security
            .approval_mode
            .clone()
            .unwrap_or_else(|| "full_auto".to_string());
        self.sync_model_status().await;
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("沙箱已设置: {}", sandbox.as_str()));
        } else {
            self.push_log(LogKind::Info, format!("sandbox set: {}", sandbox.as_str()));
        }
    }

    /// `/config edit`: hand `~/.wunder/config.toml` to the user's editor, then
    /// re-project it so the running session picks up the change immediately.
    async fn edit_user_config_file(&mut self) -> Result<()> {
        let path = self.runtime.user_config.user_path.clone();
        let seed = std::fs::read_to_string(&path).unwrap_or_default();
        let edited = match crate::open_external_editor(&self.runtime, Some(seed.as_str())) {
            Ok(text) => text,
            Err(err) => {
                self.push_log(LogKind::Error, err.to_string());
                return Ok(());
            }
        };
        if edited == seed {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "配置未变化",
                    "config unchanged",
                ),
            );
            return Ok(());
        }
        std::fs::write(&path, edited.as_bytes())
            .map_err(|err| anyhow!("write {} failed: {err}", path.display()))?;

        match crate::user_config::load_report(
            self.runtime.wunder_home.as_path(),
            self.runtime.launch_dir.as_path(),
            self.global.profile.as_deref(),
            self.global.strict_config,
        ) {
            Ok(report) => {
                let values = report.values.clone();
                self.runtime.user_config = report;
                // Re-project onto the engine config so the running session sees
                // the edited values instead of waiting for a restart.
                self.runtime
                    .state
                    .config_store
                    .update(move |config| {
                        crate::runtime::apply_user_config(config, &values, None);
                    })
                    .await?;
                self.sync_model_status().await;
                self.push_log(
                    LogKind::Info,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        "配置已保存并生效",
                        "config saved and applied",
                    ),
                );
            }
            Err(err) => {
                self.push_log(
                    LogKind::Error,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        &format!("配置已保存但无法解析，启动时会退回默认值: {err}"),
                        &format!("config saved but unreadable, startup will fall back to defaults: {err}"),
                    ),
                );
            }
        }
        Ok(())
    }

    /// Write one key into `~/.wunder/config.toml`, reporting a failure without
    /// pretending the session change did not happen.
    fn persist_user_config_value(&mut self, key: &str, value: &str) {
        if let Err(err) =
            crate::user_config::save_user_value(self.runtime.wunder_home.as_path(), key, value)
        {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    &format!("已在本次会话生效，但写入配置失败: {err}"),
                    &format!("applies to this session, but saving config failed: {err}"),
                ),
            );
        }
    }

    async fn handle_diff_slash(&mut self, args: &str) -> Result<()> {
        let root = self.runtime.launch_dir.clone();
        let language = self.display_language.clone();
        let action = match crate::parse_diff_slash_action(args) {
            Ok(action) => action,
            Err(err) => {
                self.push_log(LogKind::Error, err.to_string());
                return Ok(());
            }
        };
        let lines = tokio::task::spawn_blocking(move || match action {
            crate::DiffSlashAction::Summary => {
                crate::git_diff_summary_lines_with_language(root.as_path(), language.as_str())
                    .unwrap_or_else(|err| vec![err.to_string()])
            }
            crate::DiffSlashAction::Files => {
                crate::diff_files_lines_with_language(root.as_path(), language.as_str())
            }
            crate::DiffSlashAction::Show(target) => crate::diff_file_lines_with_language(
                root.as_path(),
                target.as_str(),
                language.as_str(),
            ),
            crate::DiffSlashAction::Hunks(target) => crate::diff_hunk_lines_with_language(
                root.as_path(),
                target.as_str(),
                language.as_str(),
            ),
            crate::DiffSlashAction::Stage(target) => {
                match crate::run_git_file_action(root.as_path(), target.as_str(), "stage") {
                    Ok(()) => vec![crate::locale::tr(
                        language.as_str(),
                        "已 stage 目标文件",
                        "file staged",
                    )],
                    Err(err) => vec![format!("[error] {err}")],
                }
            }
            crate::DiffSlashAction::Unstage(target) => {
                match crate::run_git_file_action(root.as_path(), target.as_str(), "unstage") {
                    Ok(()) => vec![crate::locale::tr(
                        language.as_str(),
                        "已取消 stage",
                        "file unstaged",
                    )],
                    Err(err) => vec![format!("[error] {err}")],
                }
            }
            crate::DiffSlashAction::Revert(target) => {
                match crate::run_git_file_action(root.as_path(), target.as_str(), "revert") {
                    Ok(()) => vec![crate::locale::tr(
                        language.as_str(),
                        "已回滚目标文件到 HEAD",
                        "file reverted to HEAD",
                    )],
                    Err(err) => vec![format!("[error] {err}")],
                }
            }
        })
        .await
        .map_err(|err| anyhow!("diff task cancelled: {err}"))?;
        for line in lines {
            self.push_log(LogKind::Info, line);
        }
        Ok(())
    }

    async fn handle_review_slash(&mut self, args: &str) -> Result<()> {
        if self.thread_is_running(self.session_id.as_str()) {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "助手仍在运行，请等待完成后再执行 /review",
                    "assistant is still running, wait for completion before running /review",
                ),
            );
            return Ok(());
        }

        let root = self.runtime.launch_dir.clone();
        let focus = args.trim().to_string();
        let focus_for_prompt = focus.clone();
        let language = self.display_language.clone();
        let prompt = match tokio::task::spawn_blocking(move || {
            crate::build_review_prompt_with_language(
                root.as_path(),
                &focus_for_prompt,
                language.as_str(),
            )
        })
        .await
        {
            Ok(Ok(prompt)) => prompt,
            Ok(Err(err)) => {
                self.push_log(LogKind::Error, err.to_string());
                return Ok(());
            }
            Err(err) => {
                if self.is_zh_language() {
                    self.push_log(LogKind::Error, format!("review 任务已取消: {err}"));
                } else {
                    self.push_log(LogKind::Error, format!("review task cancelled: {err}"));
                }
                return Ok(());
            }
        };

        let user_echo = if focus.is_empty() {
            "/review".to_string()
        } else {
            format!("/review {focus}")
        };
        self.start_stream_request(prompt, user_echo, None).await
    }

    async fn handle_plan_slash(&mut self, args: &str) -> Result<()> {
        if self.thread_is_running(self.session_id.as_str()) {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "助手仍在运行，请等待完成后再执行 /plan",
                    "assistant is still running, wait for completion before running /plan",
                ),
            );
            return Ok(());
        }
        let prompt = crate::build_plan_prompt_with_language(self.display_language.as_str(), args);
        let cleaned = args.trim();
        let user_echo = if cleaned.is_empty() {
            "/plan".to_string()
        } else {
            format!("/plan {cleaned}")
        };
        self.start_stream_request(prompt, user_echo, None).await
    }

    async fn handle_goal_slash(&mut self, args: &str) -> Result<()> {
        let command = wunder_server::goal::parse_goal_command(args);
        let state = self.runtime.state.clone();
        let service = state.kernel.orchestrator.goal_handle();
        match wunder_server::goal::execute_goal_command(
            service.as_ref(),
            state.storage.clone(),
            &self.runtime.user_id,
            &self.session_id,
            command,
        )
        .await
        {
            Ok((reply, goal)) => {
                for line in reply.lines().filter(|line| !line.is_empty()) {
                    self.push_log(LogKind::Info, line.to_string());
                }
                if goal.is_some() {
                    self.push_goal_log(goal.as_ref());
                }
            }
            Err(error) => {
                self.push_log(LogKind::Info, error.to_string());
            }
        }
        Ok(())
    }

    fn push_goal_log(&mut self, goal: Option<&serde_json::Value>) {
        let Some(goal) = goal else {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "当前没有目标",
                    "no goal set",
                )
                .to_string(),
            );
            return;
        };
        let zh = self.is_zh_language();
        let mut lines = vec![format!(
            "- {}: {}",
            if zh { "目标" } else { "goal" },
            goal.get("objective").and_then(Value::as_str).unwrap_or(""),
        )];
        if let Some(phase) = goal.get("phase").and_then(Value::as_str) {
            lines.push(format!(
                "- {}: {}",
                if zh { "阶段" } else { "phase" },
                phase
            ));
        }
        if let Some(message) = goal.get("blocked_message").and_then(Value::as_str) {
            if !message.is_empty() {
                lines.push(format!(
                    "- {}: {}",
                    if zh { "阻塞原因" } else { "blocked" },
                    message
                ));
            }
        }
        if let (Some(started), Some(cap)) = (
            goal.get("rounds_started").and_then(Value::as_i64),
            goal.get("max_goal_rounds").and_then(Value::as_i64),
        ) {
            lines.push(format!("- rounds: {started}/{cap}"));
        }
        for line in lines {
            self.push_log(LogKind::Info, line);
        }
    }

    fn handle_init_slash(&mut self, args: &str) -> Result<()> {
        let force = args.trim().eq_ignore_ascii_case("force");
        if !args.trim().is_empty() && !force {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /init [force]",
                    "usage: /init [force]",
                ),
            );
            return Ok(());
        }

        let path = self.runtime.launch_dir.join("AGENTS.md");
        if path.exists() && !force {
            if self.is_zh_language() {
                self.push_log(
                    LogKind::Info,
                    format!("AGENTS.md 已存在: {}", path.to_string_lossy()),
                );
                self.push_log(LogKind::Info, "如需覆盖请使用: /init force".to_string());
            } else {
                self.push_log(
                    LogKind::Info,
                    format!("AGENTS.md already exists: {}", path.to_string_lossy()),
                );
                self.push_log(LogKind::Info, "use /init force to overwrite".to_string());
            }
            return Ok(());
        }

        fs::write(
            &path,
            crate::init_agents_template_text(self.display_language.as_str()),
        )?;
        if self.is_zh_language() {
            self.push_log(
                LogKind::Info,
                format!("已生成 AGENTS.md: {}", path.to_string_lossy()),
            );
        } else {
            self.push_log(
                LogKind::Info,
                format!("generated AGENTS.md: {}", path.to_string_lossy()),
            );
        }
        Ok(())
    }

    async fn handle_attach_slash(&mut self, args: &str) -> Result<()> {
        let action = match crate::attachments::parse_attach_action(args) {
            Ok(action) => action,
            Err(_) => {
                self.push_log(
                    LogKind::Info,
                    crate::attachments::attach_usage(self.display_language.as_str()),
                );
                return Ok(());
            }
        };

        match action {
            crate::attachments::AttachAction::Show => {
                if self.pending_attachments.is_empty() {
                    self.push_log(
                        LogKind::Info,
                        crate::locale::tr(
                            self.display_language.as_str(),
                            "当前没有待发送附件",
                            "no queued attachments",
                        ),
                    );
                } else {
                    self.push_log(
                        LogKind::Info,
                        crate::locale::tr(
                            self.display_language.as_str(),
                            "待发送附件:",
                            "queued attachments:",
                        ),
                    );
                    let lines = self
                        .pending_attachments
                        .iter()
                        .enumerate()
                        .map(|(index, item)| {
                            crate::attachments::summarize_attachment(
                                item,
                                index,
                                self.display_language.as_str(),
                            )
                        })
                        .collect::<Vec<_>>();
                    for line in lines {
                        self.push_log(LogKind::Info, line);
                    }
                }
                self.push_log(
                    LogKind::Info,
                    crate::attachments::attach_usage(self.display_language.as_str()),
                );
            }
            crate::attachments::AttachAction::Clear => {
                self.pending_attachments.clear();
                self.push_log(
                    LogKind::Info,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        "附件队列已清空",
                        "attachment queue cleared",
                    ),
                );
            }
            crate::attachments::AttachAction::Drop(index) => {
                let drop_index = index.saturating_sub(1);
                if drop_index >= self.pending_attachments.len() {
                    if self.is_zh_language() {
                        self.push_log(LogKind::Error, format!("附件编号超出范围: {index}"));
                    } else {
                        self.push_log(
                            LogKind::Error,
                            format!("attachment index out of range: {index}"),
                        );
                    }
                    return Ok(());
                }
                let removed = self
                    .remove_pending_attachment_at(drop_index)
                    .expect("attachment index already validated");
                let removed_name = removed
                    .payload
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("attachment");
                if self.is_zh_language() {
                    self.push_log(LogKind::Info, format!("已移除附件: {removed_name}"));
                } else {
                    self.push_log(LogKind::Info, format!("attachment removed: {removed_name}"));
                }
            }
            crate::attachments::AttachAction::Add(path) => {
                let prepared = match crate::attachments::prepare_attachment_from_path(
                    &self.runtime,
                    path.as_str(),
                )
                .await
                {
                    Ok(prepared) => prepared,
                    Err(err) => {
                        self.push_log(LogKind::Error, err.to_string());
                        return Ok(());
                    }
                };
                self.queue_prepared_attachment(prepared, true);
            }
        }
        Ok(())
    }

    fn handle_edit_slash(&mut self, args: &str) -> Result<()> {
        let seed = if args.trim().is_empty() {
            self.input.clone()
        } else {
            args.trim().to_string()
        };
        let edited = match crate::open_external_editor(&self.runtime, Some(seed.as_str())) {
            Ok(text) => text,
            Err(err) => {
                self.push_log(LogKind::Error, err.to_string());
                return Ok(());
            }
        };
        let cleaned = edited.trim().to_string();
        if cleaned.is_empty() {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "编辑结果为空，已取消",
                    "editor output is empty, cancelled",
                ),
            );
            return Ok(());
        }
        self.input = cleaned;
        self.input_cursor = self.input.len();
        self.focus_area = FocusArea::Input;
        self.push_log(
            LogKind::Info,
            crate::locale::tr(
                self.display_language.as_str(),
                "已将编辑器内容回填到输入框",
                "editor content loaded into input box",
            ),
        );
        Ok(())
    }

    async fn handle_skills_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("list") {
            self.show_skills_catalog().await;
            return Ok(());
        }
        if cleaned.eq_ignore_ascii_case("root") {
            let root = self
                .runtime
                .state
                .user_tool_store
                .get_skill_root(&self.runtime.user_id);
            if self.is_zh_language() {
                self.push_log(
                    LogKind::Info,
                    format!("技能目录: {}", root.to_string_lossy()),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    format!("skill root: {}", root.to_string_lossy()),
                );
            }
            return Ok(());
        }

        let mut parts = cleaned.splitn(2, char::is_whitespace);
        let action = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default().trim();
        if action.eq_ignore_ascii_case("enable") {
            self.toggle_skill_state(value, true).await?;
            return Ok(());
        }
        if action.eq_ignore_ascii_case("disable") {
            self.toggle_skill_state(value, false).await?;
            return Ok(());
        }

        self.push_log(
            LogKind::Info,
            crate::locale::tr(
                self.display_language.as_str(),
                "用法: /skills [list|enable <name>|disable <name>|root]",
                "usage: /skills [list|enable <name>|disable <name>|root]",
            ),
        );
        Ok(())
    }

    async fn show_skills_catalog(&mut self) {
        let payload = self
            .runtime
            .state
            .user_tool_store
            .load_user_tools(&self.runtime.user_id);
        let enabled_set = payload
            .skills
            .enabled
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let (skill_root, specs) = crate::load_user_skill_specs(&self.runtime).await;

        if self.is_zh_language() {
            self.push_log(
                LogKind::Info,
                format!("技能目录: {}", skill_root.to_string_lossy()),
            );
        } else {
            self.push_log(
                LogKind::Info,
                format!("skill root: {}", skill_root.to_string_lossy()),
            );
        }

        if specs.is_empty() {
            if self.is_zh_language() {
                self.push_log(
                    LogKind::Info,
                    format!("在 {} 未找到技能", skill_root.to_string_lossy()),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    format!("no skills found in {}", skill_root.to_string_lossy()),
                );
            }
            return;
        }

        for spec in specs {
            let state = if enabled_set.contains(&spec.name) {
                crate::locale::tr(self.display_language.as_str(), "启用", "enabled")
            } else {
                crate::locale::tr(self.display_language.as_str(), "禁用", "disabled")
            };
            self.push_log(
                LogKind::Info,
                format!("{} [{}] {}", spec.name, state, spec.path),
            );
        }
    }

    async fn toggle_skill_state(&mut self, target: &str, enable: bool) -> Result<()> {
        let skill_name = target.trim().to_string();
        if skill_name.is_empty() {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "技能名称不能为空",
                    "skill name cannot be empty",
                ),
            );
            return Ok(());
        }

        if enable {
            let (_, specs) = crate::load_user_skill_specs(&self.runtime).await;
            let known = specs
                .into_iter()
                .map(|spec| spec.name)
                .collect::<std::collections::HashSet<_>>();
            if !known.contains(&skill_name) {
                if self.is_zh_language() {
                    self.push_log(LogKind::Error, format!("未找到技能: {skill_name}"));
                } else {
                    self.push_log(LogKind::Error, format!("skill not found: {skill_name}"));
                }
                return Ok(());
            }
        }

        let payload = self
            .runtime
            .state
            .user_tool_store
            .load_user_tools(&self.runtime.user_id);
        let mut enabled = payload.skills.enabled;
        enabled.retain(|name| name.trim() != skill_name.as_str());
        if enable {
            enabled.push(skill_name.clone());
        }
        let enabled = normalize_name_list_for_tui(enabled);
        self.runtime.state.user_tool_store.update_skills(
            &self.runtime.user_id,
            enabled,
            payload.skills.shared,
        )?;
        self.runtime
            .state
            .user_tool_manager
            .clear_skill_cache(Some(&self.runtime.user_id));
        self.reload_popup_catalogs().await;

        if enable {
            if self.is_zh_language() {
                self.push_log(LogKind::Info, format!("技能已启用: {skill_name}"));
            } else {
                self.push_log(LogKind::Info, format!("skill enabled: {skill_name}"));
            }
        } else if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("技能已禁用: {skill_name}"));
        } else {
            self.push_log(LogKind::Info, format!("skill disabled: {skill_name}"));
        }
        Ok(())
    }

    async fn handle_rename_slash(&mut self, args: &str) -> Result<()> {
        let title = args.trim();
        if title.is_empty() {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /rename <title>",
                    "usage: /rename <title>",
                ),
            );
            return Ok(());
        }
        let saved =
            crate::rename_session_title(&self.runtime, self.session_id.as_str(), title).await?;
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("会话已重命名: {saved}"));
        } else {
            self.push_log(LogKind::Info, format!("session renamed: {saved}"));
        }
        Ok(())
    }

    async fn handle_compact_slash(&mut self) -> Result<()> {
        if self.thread_is_running(self.session_id.as_str()) {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "助手仍在运行，请等待完成后再执行 /compact",
                    "assistant is still running, wait for completion before running /compact",
                ),
            );
            return Ok(());
        }
        let (new_session, summary) = crate::compact_session_into_branch(
            &self.runtime,
            self.session_id.as_str(),
            self.display_language.as_str(),
        )
        .await?;
        self.switch_to_existing_session(new_session.as_str())
            .await?;
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("已创建压缩分支会话: {new_session}"));
            self.push_log(
                LogKind::Info,
                format!("摘要长度: {} 字符", summary.chars().count()),
            );
        } else {
            self.push_log(
                LogKind::Info,
                format!("created compacted branch session: {new_session}"),
            );
            self.push_log(
                LogKind::Info,
                format!("summary size: {} chars", summary.chars().count()),
            );
        }
        Ok(())
    }

    async fn handle_mcp_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        let language = self.display_language.clone();
        let is_zh = self.is_zh_language();

        let usage = crate::locale::tr(
            language.as_str(),
            "用法: /mcp [list|get <name>|add <name> <endpoint> [transport]|enable <name>|disable <name>|remove <name>|login <name> [bearer-token|token|api-key] <secret>|logout <name>|test <name>|<name>]",
            "usage: /mcp [list|get <name>|add <name> <endpoint> [transport]|enable <name>|disable <name>|remove <name>|login <name> [bearer-token|token|api-key] <secret>|logout <name>|test <name>|<name>]",
        );
        if cleaned.eq_ignore_ascii_case("help") || cleaned.eq_ignore_ascii_case("?") {
            self.push_log(LogKind::Info, usage.to_string());
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    language.as_str(),
                    "示例: /mcp list, /mcp add docs https://example.com/mcp, /mcp login docs --bearer-token <TOKEN>",
                    "examples: /mcp list, /mcp add docs https://example.com/mcp, /mcp login docs --bearer-token <TOKEN>",
                ),
            );
            return Ok(());
        }

        let values = if cleaned.is_empty() {
            Vec::new()
        } else {
            match shell_words::split(cleaned) {
                Ok(values) => values,
                Err(err) => {
                    self.push_log(
                        LogKind::Error,
                        if is_zh {
                            format!("解析 /mcp 参数失败: {err}")
                        } else {
                            format!("parse /mcp args failed: {err}")
                        },
                    );
                    self.push_log(LogKind::Info, usage.to_string());
                    return Ok(());
                }
            }
        };
        let action = values
            .first()
            .map(|value| value.trim().to_ascii_lowercase())
            .unwrap_or_default();

        if action == "add" {
            if values.len() < 3 || values.len() > 4 {
                self.push_log(LogKind::Error, usage.to_string());
                return Ok(());
            }
            let name = values[1].trim();
            let endpoint = values[2].trim();
            let transport = values
                .get(3)
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .unwrap_or("streamable-http");
            if name.is_empty() || endpoint.is_empty() {
                self.push_log(LogKind::Error, usage.to_string());
                return Ok(());
            }
            let mut payload = self
                .runtime
                .state
                .user_tool_store
                .load_user_tools(&self.runtime.user_id);
            payload
                .mcp_servers
                .retain(|server| !server.name.trim().eq_ignore_ascii_case(name));
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
            self.runtime
                .state
                .user_tool_store
                .update_mcp_servers(&self.runtime.user_id, payload.mcp_servers)?;
            self.reload_popup_catalogs().await;
            if is_zh {
                self.push_log(LogKind::Info, format!("已添加 MCP 服务器: {name}"));
            } else {
                self.push_log(LogKind::Info, format!("mcp server added: {name}"));
            }
            return Ok(());
        }

        if action == "enable" || action == "disable" {
            if values.len() != 2 {
                self.push_log(LogKind::Error, usage.to_string());
                return Ok(());
            }
            let target = values[1].trim();
            if target.is_empty() {
                self.push_log(LogKind::Error, usage.to_string());
                return Ok(());
            }
            let enabled = action == "enable";
            let mut payload = self
                .runtime
                .state
                .user_tool_store
                .load_user_tools(&self.runtime.user_id);
            let Some(index) = find_mcp_server_index_for_tui(&payload.mcp_servers, target) else {
                if is_zh {
                    self.push_log(LogKind::Error, format!("未找到 MCP 服务器: {target}"));
                } else {
                    self.push_log(LogKind::Error, format!("mcp server not found: {target}"));
                }
                return Ok(());
            };
            payload.mcp_servers[index].enabled = enabled;
            self.runtime
                .state
                .user_tool_store
                .update_mcp_servers(&self.runtime.user_id, payload.mcp_servers)?;
            self.reload_popup_catalogs().await;
            if is_zh {
                self.push_log(
                    LogKind::Info,
                    format!(
                        "MCP 服务器已{}: {target}",
                        if enabled { "启用" } else { "禁用" }
                    ),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    format!(
                        "mcp server {}: {target}",
                        if enabled { "enabled" } else { "disabled" }
                    ),
                );
            }
            return Ok(());
        }

        if action == "remove" {
            if values.len() != 2 {
                self.push_log(LogKind::Error, usage.to_string());
                return Ok(());
            }
            let target = values[1].trim();
            if target.is_empty() {
                self.push_log(LogKind::Error, usage.to_string());
                return Ok(());
            }
            let mut payload = self
                .runtime
                .state
                .user_tool_store
                .load_user_tools(&self.runtime.user_id);
            let before = payload.mcp_servers.len();
            payload
                .mcp_servers
                .retain(|server| !server.name.trim().eq_ignore_ascii_case(target));
            if before == payload.mcp_servers.len() {
                if is_zh {
                    self.push_log(LogKind::Error, format!("未找到 MCP 服务器: {target}"));
                } else {
                    self.push_log(LogKind::Error, format!("mcp server not found: {target}"));
                }
                return Ok(());
            }
            self.runtime
                .state
                .user_tool_store
                .update_mcp_servers(&self.runtime.user_id, payload.mcp_servers)?;
            self.reload_popup_catalogs().await;
            if is_zh {
                self.push_log(LogKind::Info, format!("已移除 MCP 服务器: {target}"));
            } else {
                self.push_log(LogKind::Info, format!("mcp server removed: {target}"));
            }
            return Ok(());
        }

        if action == "login" {
            if values.len() < 3 || values.len() > 4 {
                self.push_log(LogKind::Error, usage.to_string());
                return Ok(());
            }
            let target = values[1].trim();
            if target.is_empty() {
                self.push_log(LogKind::Error, usage.to_string());
                return Ok(());
            }
            let (auth_key, auth_value) = if values.len() == 3 {
                ("bearer_token", values[2].clone())
            } else {
                let Some(key) = mcp_auth_key_from_alias_for_tui(values[2].as_str()) else {
                    if is_zh {
                        self.push_log(
                            LogKind::Error,
                            format!(
                                "非法鉴权类型: {}（支持 bearer-token/token/api-key）",
                                values[2]
                            ),
                        );
                    } else {
                        self.push_log(
                            LogKind::Error,
                            format!(
                                "invalid auth type: {} (supported: bearer-token/token/api-key)",
                                values[2]
                            ),
                        );
                    }
                    return Ok(());
                };
                (key, values[3].clone())
            };
            let auth_value = auth_value.trim().to_string();
            if auth_value.is_empty() {
                self.push_log(LogKind::Error, usage.to_string());
                return Ok(());
            }
            let mut payload = self
                .runtime
                .state
                .user_tool_store
                .load_user_tools(&self.runtime.user_id);
            let Some(index) = find_mcp_server_index_for_tui(&payload.mcp_servers, target) else {
                if is_zh {
                    self.push_log(LogKind::Error, format!("未找到 MCP 服务器: {target}"));
                } else {
                    self.push_log(LogKind::Error, format!("mcp server not found: {target}"));
                }
                return Ok(());
            };
            payload.mcp_servers[index].auth = Some(json!({
                auth_key: auth_value,
            }));
            self.runtime
                .state
                .user_tool_store
                .update_mcp_servers(&self.runtime.user_id, payload.mcp_servers)?;
            self.reload_popup_catalogs().await;
            let auth_name = mcp_auth_key_label_for_tui(auth_key, is_zh);
            if is_zh {
                self.push_log(
                    LogKind::Info,
                    format!("已更新 MCP 鉴权凭据: {target} ({auth_name})"),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    format!("mcp auth updated: {target} ({auth_name})"),
                );
            }
            return Ok(());
        }

        if action == "logout" {
            if values.len() != 2 {
                self.push_log(LogKind::Error, usage.to_string());
                return Ok(());
            }
            let target = values[1].trim();
            if target.is_empty() {
                self.push_log(LogKind::Error, usage.to_string());
                return Ok(());
            }
            let mut payload = self
                .runtime
                .state
                .user_tool_store
                .load_user_tools(&self.runtime.user_id);
            let Some(index) = find_mcp_server_index_for_tui(&payload.mcp_servers, target) else {
                if is_zh {
                    self.push_log(LogKind::Error, format!("未找到 MCP 服务器: {target}"));
                } else {
                    self.push_log(LogKind::Error, format!("mcp server not found: {target}"));
                }
                return Ok(());
            };
            payload.mcp_servers[index].auth = None;
            self.runtime
                .state
                .user_tool_store
                .update_mcp_servers(&self.runtime.user_id, payload.mcp_servers)?;
            self.reload_popup_catalogs().await;
            if is_zh {
                self.push_log(LogKind::Info, format!("已清除 MCP 鉴权凭据: {target}"));
            } else {
                self.push_log(LogKind::Info, format!("mcp auth cleared: {target}"));
            }
            return Ok(());
        }

        if action == "test" {
            if values.len() != 2 {
                self.push_log(LogKind::Error, usage.to_string());
                return Ok(());
            }
            let lines = crate::execute_apps_command(
                &self.runtime,
                language.as_str(),
                format!("test {}", values[1].trim()).as_str(),
            )
            .await?;
            for line in lines {
                self.push_log(LogKind::Info, line);
            }
            return Ok(());
        }

        let lookup_target = if action == "get" || action == "info" {
            if values.len() != 2 {
                self.push_log(LogKind::Error, usage.to_string());
                return Ok(());
            }
            values[1].trim().to_string()
        } else {
            cleaned.to_string()
        };

        let mut payload = self
            .runtime
            .state
            .user_tool_store
            .load_user_tools(&self.runtime.user_id);
        payload
            .mcp_servers
            .sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));

        if payload.mcp_servers.is_empty() {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "尚未配置 MCP 服务器。使用 `wunder-cli mcp add` 新增。",
                    "No MCP servers configured. Use `wunder-cli mcp add` to add one.",
                ),
            );
            return Ok(());
        }

        if cleaned.is_empty() || action == "list" {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(self.display_language.as_str(), "MCP 配置", "mcp"),
            );
            for server in payload.mcp_servers {
                for line in format_mcp_server_lines_for_tui(&server, is_zh, false) {
                    self.push_log(LogKind::Info, line);
                }
            }
            return Ok(());
        }

        let Some(index) =
            find_mcp_server_index_for_tui(&payload.mcp_servers, lookup_target.as_str())
        else {
            if is_zh {
                self.push_log(
                    LogKind::Error,
                    format!("未找到 MCP 服务器: {}", lookup_target.as_str()),
                );
                self.push_log(LogKind::Info, "提示: 用 /mcp 列出所有服务器".to_string());
            } else {
                self.push_log(
                    LogKind::Error,
                    format!("mcp server not found: {}", lookup_target.as_str()),
                );
                self.push_log(
                    LogKind::Info,
                    "hint: run /mcp to list all servers".to_string(),
                );
            }
            return Ok(());
        };
        let server = payload.mcp_servers.swap_remove(index);

        self.push_log(
            LogKind::Info,
            if is_zh {
                format!("MCP 服务器: {}", server.name)
            } else {
                format!("mcp server: {}", server.name)
            },
        );
        for line in format_mcp_server_lines_for_tui(&server, is_zh, true) {
            self.push_log(LogKind::Info, line);
        }
        Ok(())
    }

    pub(super) async fn sync_model_status(&mut self) {
        self.display_language = crate::locale::resolve_cli_language(&self.global);
        let snapshot = self.read_model_status().await;
        self.apply_model_status(snapshot);
    }

    /// Read the status facts without touching the app, so startup can compute
    /// them off the first frame.
    pub(super) async fn read_model_status(&self) -> ModelStatusSnapshot {
        compute_model_status(
            &self.runtime,
            self.global.model.as_deref(),
            self.global.approval_mode,
        )
        .await
    }

    pub(super) fn apply_model_status(&mut self, snapshot: ModelStatusSnapshot) {
        self.approval_mode = snapshot.approval_mode;
        self.model_name = snapshot.model_name;
        self.tool_call_mode = snapshot.tool_call_mode;
        self.reasoning_effort = snapshot.reasoning_effort;
        self.model_max_context = snapshot.max_context;
        self.model_max_rounds = snapshot.max_rounds;
    }

    /// `/new` opens a fresh thread. The thread on screen is detached exactly the way a
    /// switch detaches it: its stream keeps running, and its transcript, cards, draft and
    /// pending approvals stay attached to *its* session id until it is opened again.
    pub(super) async fn switch_to_new_session(&mut self) {
        let previous_session_id = self.session_id.clone();
        let previous_is_running = self.thread_is_running(previous_session_id.as_str());
        if self.thread_ui_cache.len() >= crate::tui::thread_registry::MAX_THREAD_PROJECTIONS - 1 {
            let evict = self.thread_ui_cache.iter().position(|(id, _)| {
                !self.thread_is_running(id)
                    && !self.pending_thread_terminals.contains_key(id)
                    && self.thread_registry.pending_approval_count(id) == 0
            });
            let Some(index) = evict else {
                self.push_log(
                    LogKind::Error,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        "线程显示缓存已满，请先结束一个运行中的线程",
                        "thread display cache is full; finish a running thread first",
                    ),
                );
                return;
            };
            self.thread_ui_cache.remove(index);
        }

        self.thread_registry.save_view_state(
            previous_session_id.as_str(),
            self.input.clone(),
            self.transcript_offset_from_bottom,
        );
        let previous_state = ThreadUiState::take(self);
        self.thread_ui_cache
            .push_back((previous_session_id.clone(), previous_state));
        if let Some(request) = self.active_approval.take() {
            self.thread_registry
                .queue_approval(previous_session_id.as_str(), request);
        }
        for request in self.approval_queue.drain(..) {
            self.thread_registry
                .queue_approval(previous_session_id.as_str(), request);
        }

        self.session_id = uuid::Uuid::new_v4().simple().to_string();
        // A thread this process just created is writable by definition.
        self.watch_only_reason = None;
        self.thread_registry.activate(self.session_id.as_str());
        self.busy = self.thread_has_live_work(self.session_id.as_str());
        self.input_cursor = 0;
        self.history_cursor = None;
        self.history_search = None;
        self.config_wizard = None;
        self.last_usage = None;
        self.reset_turn_metrics_snapshot();
        self.active_assistant = None;
        self.active_reasoning = None;
        self.stream_saw_output = false;
        self.stream_saw_final = false;
        self.stream_received_content_delta = false;
        self.stream_tool_markup_open = false;
        self.tool_phase_notice_emitted = false;
        self.reset_stream_catchup_state();
        self.reset_plain_char_burst();
        self.active_approval = None;
        self.approval_selected_index = 0;
        self.ctrl_c_hint_deadline = None;
        self.focus_area = FocusArea::Input;
        self.transcript_selected = None;
        self.resume_picker = None;
        self.active_inquiry_panel = None;
        self.inquiry_selected_index = 0;
        self.command_sessions = CommandSessionDisplayState::default();
        self.command_log_indices.clear();
        self.tool_log_indices.clear();
        self.pending_temp_tool_cells.clear();
        self.expanded_cards.clear();
        self.reset_scrollback_archive();
        self.invalidate_transcript_metrics();
        self.session_stats = crate::SessionStatsSnapshot::default();
        self.reload_session_stats().await;
        self.push_log(
            LogKind::Info,
            format!("switched to session: {}", self.session_id),
        );
        if previous_is_running {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "上一个线程仍在后台运行，可用 ← 或 /threads 回到它",
                    "the previous thread is still running in the background; use ← or /threads to return",
                ),
            );
        }
    }

    fn status_lines(&self) -> Vec<String> {
        let is_zh = self.is_zh_language();
        vec![
            if is_zh {
                "状态".to_string()
            } else {
                "status".to_string()
            },
            if is_zh {
                format!("- 会话: {}", self.session_id)
            } else {
                format!("- session: {}", self.session_id)
            },
            if is_zh {
                format!("- 模型: {}", self.model_name)
            } else {
                format!("- model: {}", self.model_name)
            },
            if is_zh {
                format!("- 工具调用模式: {}", self.tool_call_mode)
            } else {
                format!("- tool_call_mode: {}", self.tool_call_mode)
            },
            if is_zh {
                format!("- 审批模式: {}", self.approval_mode)
            } else {
                format!("- approval_mode: {}", self.approval_mode)
            },
            if is_zh {
                format!(
                    "- 沙箱: {}",
                    self.runtime
                        .user_config
                        .values
                        .sandbox_mode
                        .clone()
                        .unwrap_or_else(|| "workspace-write".to_string())
                )
            } else {
                format!(
                    "- sandbox: {}",
                    self.runtime
                        .user_config
                        .values
                        .sandbox_mode
                        .clone()
                        .unwrap_or_else(|| "workspace-write".to_string())
                )
            },
            if is_zh {
                format!(
                    "- 上下文: {} token",
                    self.session_stats.context_used_tokens.max(0)
                )
            } else {
                format!(
                    "- context_tokens: {}",
                    self.session_stats.context_used_tokens.max(0)
                )
            },
            if is_zh {
                format!(
                    "- token 用量: input={} output={} total={}",
                    self.session_stats.total_input_tokens,
                    self.session_stats.total_output_tokens,
                    self.session_stats.total_tokens
                )
            } else {
                format!(
                    "- token_usage: input={} output={} total={}",
                    self.session_stats.total_input_tokens,
                    self.session_stats.total_output_tokens,
                    self.session_stats.total_tokens
                )
            },
            if is_zh {
                format!("- 模型调用: {}", self.session_stats.model_calls)
            } else {
                format!("- model_calls: {}", self.session_stats.model_calls)
            },
            if is_zh {
                format!(
                    "- 工具调用: {} (结果 {})",
                    self.session_stats.tool_calls, self.session_stats.tool_results
                )
            } else {
                format!(
                    "- tool_calls: {} (results {})",
                    self.session_stats.tool_calls, self.session_stats.tool_results
                )
            },
            if is_zh {
                format!("- 待发送附件: {}", self.pending_attachments.len())
            } else {
                format!("- queued_attachments: {}", self.pending_attachments.len())
            },
            if is_zh {
                format!(
                    "- 回合通知: {}",
                    crate::describe_turn_notification(
                        &self.runtime.load_turn_notification_config(),
                        self.display_language.as_str()
                    )
                )
            } else {
                format!(
                    "- turn_notify: {}",
                    crate::describe_turn_notification(
                        &self.runtime.load_turn_notification_config(),
                        self.display_language.as_str()
                    )
                )
            },
            if is_zh {
                format!("- 最大轮次: {}", self.model_max_rounds)
            } else {
                format!("- max_rounds: {}", self.model_max_rounds)
            },
            format!(
                "{} {}",
                if is_zh {
                    "- 鼠标模式:"
                } else {
                    "- mouse_mode:"
                },
                match self.mouse_mode {
                    MouseMode::Auto => {
                        if is_zh {
                            "auto(自动)"
                        } else {
                            "auto"
                        }
                    }
                    MouseMode::Scroll => {
                        if is_zh {
                            "scroll(滚轮)"
                        } else {
                            "scroll"
                        }
                    }
                    MouseMode::Select => {
                        if is_zh {
                            "select(选择)"
                        } else {
                            "select"
                        }
                    }
                }
            ),
            if is_zh {
                format!("- 工作目录: {}", self.runtime.launch_dir.to_string_lossy())
            } else {
                format!("- workspace: {}", self.runtime.launch_dir.to_string_lossy())
            },
            if is_zh {
                format!("- 临时目录: {}", self.runtime.temp_root.to_string_lossy())
            } else {
                format!("- temp_root: {}", self.runtime.temp_root.to_string_lossy())
            },
        ]
    }
}
