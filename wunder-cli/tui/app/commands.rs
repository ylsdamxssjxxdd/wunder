use super::*;

impl TuiApp {
    pub(super) async fn handle_slash_command(&mut self, line: String) -> Result<()> {
        let Some(command) = slash_command::parse_slash_command(&line) else {
            self.push_log(LogKind::Error, format!("unknown command: {line}"));
            self.push_log(
                LogKind::Info,
                "type /help to list available slash commands".to_string(),
            );
            return Ok(());
        };

        if self.busy && !command.command.available_during_task() {
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
                for line in self.status_lines() {
                    self.push_log(LogKind::Info, line);
                }
            }
            SlashCommand::Session => {
                self.reload_session_stats().await;
                for line in self.session_stats_lines() {
                    self.push_log(LogKind::Info, line);
                }
            }
            SlashCommand::System => {
                self.handle_system_slash(command.args).await?;
            }
            SlashCommand::Mouse => {
                self.handle_mouse_slash(command.args);
            }
            SlashCommand::Resume => {
                self.handle_resume_slash(command.args).await?;
            }
            SlashCommand::New => {
                if self.busy {
                    self.push_log(
                        LogKind::Error,
                        "assistant is still running, wait for completion before creating a new session"
                            .to_string(),
                    );
                } else {
                    self.switch_to_new_session().await;
                }
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
            SlashCommand::Approvals => {
                self.handle_approvals_slash(command.args).await?;
            }
            SlashCommand::Plan => {
                self.handle_plan_slash(command.args).await?;
            }
            SlashCommand::Personality => {
                self.handle_personality_slash(command.args).await?;
            }
            SlashCommand::Edit => {
                self.handle_edit_slash(command.args)?;
            }
            SlashCommand::Init => {
                self.handle_init_slash(command.args)?;
            }
            SlashCommand::Agent => {
                self.handle_agent_slash(command.args).await?;
            }
            SlashCommand::Attach => {
                self.handle_attach_slash(command.args).await?;
            }
            SlashCommand::Branches => {
                self.handle_branches_slash(command.args).await?;
            }
            SlashCommand::Notify => {
                self.handle_notify_slash(command.args)?;
            }
            SlashCommand::Diff => {
                self.handle_diff_slash(command.args).await?;
            }
            SlashCommand::Review => {
                self.handle_review_slash(command.args).await?;
            }
            SlashCommand::Mention => {
                self.handle_mention_slash(command.args).await?;
            }
            SlashCommand::Skills => {
                self.handle_skills_slash(command.args).await?;
            }
            SlashCommand::Apps => {
                self.handle_apps_slash(command.args).await?;
            }
            SlashCommand::Ps => {
                self.handle_ps_slash(command.args);
            }
            SlashCommand::Clean => {
                self.handle_clean_slash(command.args);
            }
            SlashCommand::Fork => {
                self.handle_fork_slash(command.args).await?;
            }
            SlashCommand::Rename => {
                self.handle_rename_slash(command.args).await?;
            }
            SlashCommand::Compact => {
                self.handle_compact_slash().await?;
            }
            SlashCommand::Backtrack => {
                self.handle_backtrack_slash(command.args);
            }
            SlashCommand::DebugConfig => {
                self.show_debug_config_snapshot().await?;
            }
            SlashCommand::Statusline => {
                self.handle_statusline_slash(command.args);
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
        let tool_call_mode = model_entry
            .and_then(|model| model.tool_call_mode.clone())
            .unwrap_or_else(|| "tool_call".to_string());
        let max_context = model_entry
            .and_then(|model| model.max_context)
            .filter(|value| *value > 0);

        self.reload_session_stats().await;

        let payload = json!({
            "launch_dir": self.runtime.launch_dir,
            "temp_root": self.runtime.temp_root,
            "user_id": self.runtime.user_id,
            "agent_id_override": self.agent_id_override.clone(),
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
        if values.len() < 3 || values.len() > 4 {
            self.push_log(
                LogKind::Error,
                "invalid /config args, expected: /config <base_url> <api_key> <model> [max_context]"
                    .to_string(),
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
        self.push_log(LogKind::Info, "configure llm model (step 1/4)".to_string());
        self.push_log(
            LogKind::Info,
            "input base_url (empty line to cancel)".to_string(),
        );
    }

    pub(super) async fn handle_config_wizard_input(&mut self, input: &str) -> Result<()> {
        let cleaned = input.trim();
        if cleaned.eq_ignore_ascii_case("/cancel") || cleaned.eq_ignore_ascii_case("/exit") {
            self.config_wizard = None;
            self.push_log(LogKind::Info, "config cancelled".to_string());
            return Ok(());
        }

        let Some(mut wizard) = self.config_wizard.take() else {
            return Ok(());
        };

        if wizard.base_url.is_none() {
            if cleaned.is_empty() {
                self.push_log(LogKind::Info, "config cancelled".to_string());
                return Ok(());
            }
            wizard.base_url = Some(cleaned.to_string());
            self.config_wizard = Some(wizard);
            self.push_log(LogKind::Info, "input api_key (step 2/4)".to_string());
            return Ok(());
        }

        if wizard.api_key.is_none() {
            if cleaned.is_empty() {
                self.push_log(LogKind::Info, "config cancelled".to_string());
                return Ok(());
            }
            wizard.api_key = Some(cleaned.to_string());
            self.config_wizard = Some(wizard);
            self.push_log(LogKind::Info, "input model name (step 3/4)".to_string());
            return Ok(());
        }

        if wizard.model_name.is_none() {
            if cleaned.is_empty() {
                self.push_log(LogKind::Info, "config cancelled".to_string());
                return Ok(());
            }
            wizard.model_name = Some(cleaned.to_string());
            self.config_wizard = Some(wizard);
            self.push_log(
                LogKind::Info,
                "input max_context (step 4/4, optional; Enter for auto probe)".to_string(),
            );
            return Ok(());
        }

        let manual_max_context = match crate::parse_optional_max_context_value(cleaned) {
            Ok(value) => value,
            Err(err) => {
                self.push_log(LogKind::Error, err.to_string());
                self.config_wizard = Some(wizard);
                self.push_log(
                    LogKind::Info,
                    "input max_context (step 4/4, optional; Enter for auto probe)".to_string(),
                );
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
        self.push_log(LogKind::Info, "model configured".to_string());
        self.push_log(LogKind::Info, format!("- provider: {provider}"));
        self.push_log(LogKind::Info, format!("- base_url: {base_url}"));
        self.push_log(LogKind::Info, format!("- model: {model_name}"));
        if let Some(value) = resolved_max_context {
            self.push_log(LogKind::Info, format!("- max_context: {value}"));
        } else {
            self.push_log(
                LogKind::Info,
                "- max_context: auto probe unavailable (or keep existing)".to_string(),
            );
        }
        self.push_log(LogKind::Info, "- tool_call_mode: tool_call".to_string());
        Ok(())
    }

    fn handle_mouse_slash(&mut self, args: &str) {
        let cleaned = args.trim();
        if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("show") {
            let mode = match self.mouse_mode {
                MouseMode::Auto => {
                    crate::locale::tr(self.display_language.as_str(), "自动", "auto")
                }
                MouseMode::Scroll => {
                    crate::locale::tr(self.display_language.as_str(), "滚轮", "scroll")
                }
                MouseMode::Select => {
                    crate::locale::tr(self.display_language.as_str(), "选择", "select")
                }
            };
            if self.is_zh_language() {
                self.push_log(LogKind::Info, format!("鼠标模式: {mode}"));
            } else {
                self.push_log(LogKind::Info, format!("mouse mode: {mode}"));
            }
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /mouse [auto|scroll|select]  （F2 可切换）",
                    "usage: /mouse [auto|scroll|select]  (F2 optional)",
                ),
            );
            return;
        }

        if cleaned.eq_ignore_ascii_case("auto") {
            self.set_mouse_mode(MouseMode::Auto);
            return;
        }
        if cleaned.eq_ignore_ascii_case("scroll") {
            self.set_mouse_mode(MouseMode::Scroll);
            return;
        }
        if cleaned.eq_ignore_ascii_case("select")
            || cleaned.eq_ignore_ascii_case("copy")
            || cleaned.eq_ignore_ascii_case("selection")
        {
            self.set_mouse_mode(MouseMode::Select);
            return;
        }

        self.push_log(LogKind::Error, format!("invalid /mouse args: {cleaned}"));
        self.push_log(
            LogKind::Info,
            "usage: /mouse [auto|scroll|select]  (F2 optional)".to_string(),
        );
    }

    async fn handle_resume_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("list") {
            self.open_resume_picker().await?;
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
            self.runtime.load_saved_session().ok_or_else(|| {
                anyhow!(crate::locale::tr(
                    self.display_language.as_str(),
                    "未找到保存的会话",
                    "no saved session found",
                ))
            })?
        } else if let Ok(index) = cleaned.parse::<usize>() {
            let sessions = crate::list_recent_sessions(&self.runtime, 40).await?;
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

        let config = self.runtime.state.config_store.get().await;
        if !config.llm.models.contains_key(target) {
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

        let target_name = target.to_string();
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
            if self.is_zh_language() {
                self.push_log(
                    LogKind::Info,
                    format!(
                        "工具调用模式: 模型={} 模式={}",
                        self.model_name, self.tool_call_mode
                    ),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    format!(
                        "tool_call_mode: model={} mode={}",
                        self.model_name, self.tool_call_mode
                    ),
                );
            }
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /tool-call-mode <tool_call|function_call> [model]",
                    "usage: /tool-call-mode <tool_call|function_call> [model]",
                ),
            );
            return Ok(());
        }

        let mut parts = cleaned.split_whitespace();
        let Some(mode_token) = parts.next() else {
            return Ok(());
        };
        let Some(mode) = crate::parse_tool_call_mode(mode_token) else {
            if self.is_zh_language() {
                self.push_log(LogKind::Error, format!("非法模式: {mode_token}"));
            } else {
                self.push_log(LogKind::Error, format!("invalid mode: {mode_token}"));
            }
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "可选模式: tool_call, function_call",
                    "valid modes: tool_call, function_call",
                ),
            );
            return Ok(());
        };

        let model = parts.next().map(str::to_string);
        if parts.next().is_some() {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "参数过多",
                    "too many arguments",
                ),
            );
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /tool-call-mode <tool_call|function_call> [model]",
                    "usage: /tool-call-mode <tool_call|function_call> [model]",
                ),
            );
            return Ok(());
        }

        let config = self.runtime.state.config_store.get().await;
        let target_model = if let Some(model_name) = model {
            if !config.llm.models.contains_key(&model_name) {
                if self.is_zh_language() {
                    self.push_log(LogKind::Error, format!("配置中不存在模型: {model_name}"));
                } else {
                    self.push_log(
                        LogKind::Error,
                        format!("model not found in config: {model_name}"),
                    );
                }
                return Ok(());
            }
            model_name
        } else {
            self.runtime
                .resolve_model_name(self.global.model.as_deref())
                .await
                .ok_or_else(|| {
                    anyhow!(crate::locale::tr(
                        self.display_language.as_str(),
                        "尚未配置 LLM 模型",
                        "no llm model configured",
                    ))
                })?
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
        if self.is_zh_language() {
            self.push_log(
                LogKind::Info,
                format!(
                    "工具调用模式已设置: 模型={target_model} 模式={}",
                    mode.as_str()
                ),
            );
        } else {
            self.push_log(
                LogKind::Info,
                format!(
                    "tool_call_mode set: model={target_model} mode={}",
                    mode.as_str()
                ),
            );
        }
        Ok(())
    }

    async fn handle_approvals_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("show") {
            if self.is_zh_language() {
                self.push_log(LogKind::Info, format!("审批模式: {}", self.approval_mode));
            } else {
                self.push_log(
                    LogKind::Info,
                    format!("approval_mode: {}", self.approval_mode),
                );
            }
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /approvals [show|suggest|auto_edit|full_auto]",
                    "usage: /approvals [show|suggest|auto_edit|full_auto]",
                ),
            );
            return Ok(());
        }

        let Some(mode) = crate::parse_approval_mode(cleaned) else {
            if self.is_zh_language() {
                self.push_log(LogKind::Error, format!("非法审批模式: {cleaned}"));
            } else {
                self.push_log(LogKind::Error, format!("invalid approval mode: {cleaned}"));
            }
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "可选模式: suggest, auto_edit, full_auto",
                    "valid modes: suggest, auto_edit, full_auto",
                ),
            );
            return Ok(());
        };

        let mode_text = mode.as_str().to_string();
        self.runtime
            .state
            .config_store
            .update(move |config| {
                config.security.approval_mode = Some(mode_text.clone());
            })
            .await?;
        self.sync_model_status().await;
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("审批模式已设置: {}", mode.as_str()));
        } else {
            self.push_log(
                LogKind::Info,
                format!("approval_mode set: {}", mode.as_str()),
            );
        }
        Ok(())
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
        if self.busy {
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
        if self.busy {
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

    async fn handle_personality_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("show") {
            let mode = self
                .runtime
                .load_personality_mode()
                .unwrap_or_else(|| "balanced".to_string());
            if self.is_zh_language() {
                self.push_log(LogKind::Info, format!("当前回答风格: {mode}"));
                self.push_log(
                    LogKind::Info,
                    "可选: concise | balanced | detailed | clear".to_string(),
                );
            } else {
                self.push_log(LogKind::Info, format!("current response style: {mode}"));
                self.push_log(
                    LogKind::Info,
                    "options: concise | balanced | detailed | clear".to_string(),
                );
            }
            return Ok(());
        }

        if cleaned.eq_ignore_ascii_case("clear")
            || cleaned.eq_ignore_ascii_case("none")
            || cleaned.eq_ignore_ascii_case("off")
        {
            self.runtime.clear_personality_mode()?;
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "回答风格已清除（恢复 balanced）",
                    "response style cleared (fallback to balanced)",
                ),
            );
            return Ok(());
        }

        let Some(mode) = crate::normalize_personality_mode(cleaned) else {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /personality [show|concise|balanced|detailed|clear]",
                    "usage: /personality [show|concise|balanced|detailed|clear]",
                ),
            );
            return Ok(());
        };
        self.runtime.save_personality_mode(mode)?;
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("回答风格已更新: {mode}"));
        } else {
            self.push_log(LogKind::Info, format!("response style updated: {mode}"));
        }
        Ok(())
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

    async fn handle_agent_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("show") {
            let active = self.agent_id_override.as_deref().unwrap_or("-");
            if self.is_zh_language() {
                self.push_log(LogKind::Info, format!("当前 agent_id 覆盖: {active}"));
                self.push_log(
                    LogKind::Info,
                    "用法: /agent [show|list|clear|<agent_id>]".to_string(),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    format!("current agent_id override: {active}"),
                );
                self.push_log(
                    LogKind::Info,
                    "usage: /agent [show|list|clear|<agent_id>]".to_string(),
                );
            }
            return Ok(());
        }
        if cleaned.eq_ignore_ascii_case("list") {
            let active = self
                .agent_id_override
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .or_else(|| {
                    self.global
                        .agent
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string)
                });
            let agents = crate::collect_recent_agent_ids(&self.runtime, 120).await?;
            if agents.is_empty() {
                self.push_log(
                    LogKind::Info,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        "最近会话没有可用 agent_id，直接用 /agent <agent_id> 设置即可",
                        "no agent_id found in recent sessions, use /agent <agent_id> directly",
                    ),
                );
                return Ok(());
            }
            if self.is_zh_language() {
                self.push_log(LogKind::Info, "最近 agent 列表:".to_string());
            } else {
                self.push_log(LogKind::Info, "recent agents:".to_string());
            }
            for (index, agent) in agents.iter().enumerate() {
                let marker = if active
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case(agent))
                {
                    "*"
                } else {
                    " "
                };
                self.push_log(LogKind::Info, format!("{marker} {:>2}. {agent}", index + 1));
            }
            return Ok(());
        }
        if cleaned.eq_ignore_ascii_case("clear")
            || cleaned.eq_ignore_ascii_case("none")
            || cleaned.eq_ignore_ascii_case("default")
        {
            self.agent_id_override = None;
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "agent_id 覆盖已清除",
                    "agent_id override cleared",
                ),
            );
            return Ok(());
        }
        self.agent_id_override = Some(cleaned.to_string());
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("agent_id 覆盖已更新: {cleaned}"));
        } else {
            self.push_log(
                LogKind::Info,
                format!("agent_id override updated: {cleaned}"),
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
                let removed = self.pending_attachments.remove(drop_index);
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
                if let Some(existing) = self
                    .pending_attachments
                    .iter()
                    .position(|item| item.source.eq_ignore_ascii_case(prepared.source.as_str()))
                {
                    self.pending_attachments.remove(existing);
                }
                self.pending_attachments.push(prepared);
                if let Some(last) = self.pending_attachments.last() {
                    if self.is_zh_language() {
                        self.push_log(
                            LogKind::Info,
                            format!(
                                "附件已加入队列（下一轮自动发送）: {}",
                                crate::attachments::summarize_attachment(
                                    last,
                                    self.pending_attachments.len().saturating_sub(1),
                                    self.display_language.as_str()
                                )
                            ),
                        );
                    } else {
                        self.push_log(
                            LogKind::Info,
                            format!(
                                "attachment queued (auto-send on next turn): {}",
                                crate::attachments::summarize_attachment(
                                    last,
                                    self.pending_attachments.len().saturating_sub(1),
                                    self.display_language.as_str()
                                )
                            ),
                        );
                    }
                }
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

    async fn handle_branches_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if let Some(rest) = cleaned.strip_prefix("switch ") {
            return self.resume_to_session(rest.trim()).await;
        }
        if !cleaned.is_empty()
            && !cleaned.eq_ignore_ascii_case("tree")
            && !cleaned.eq_ignore_ascii_case("list")
        {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /branches [tree|list|switch <session_id>]",
                    "usage: /branches [tree|list|switch <session_id>]",
                ),
            );
            return Ok(());
        }
        let view_tree = !cleaned.eq_ignore_ascii_case("list");
        let lines = crate::collect_branch_view_rows(
            &self.runtime,
            self.display_language.as_str(),
            view_tree,
        )
        .await?;
        for line in lines {
            self.push_log(LogKind::Info, line);
        }
        Ok(())
    }

    fn handle_notify_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("show") {
            let config = self.runtime.load_turn_notification_config();
            if self.is_zh_language() {
                self.push_log(
                    LogKind::Info,
                    format!(
                        "当前回合通知: {}",
                        crate::describe_turn_notification(&config, self.display_language.as_str())
                    ),
                );
                self.push_log(
                    LogKind::Info,
                    "用法: /notify [show|off|bell|osc9|when <always|unfocused>|<command...>]"
                        .to_string(),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    format!(
                        "current turn notification: {}",
                        crate::describe_turn_notification(&config, self.display_language.as_str())
                    ),
                );
                self.push_log(
                    LogKind::Info,
                    "usage: /notify [show|off|bell|osc9|when <always|unfocused>|<command...>]"
                        .to_string(),
                );
            }
            return Ok(());
        }

        if cleaned.eq_ignore_ascii_case("off")
            || cleaned.eq_ignore_ascii_case("clear")
            || cleaned.eq_ignore_ascii_case("none")
        {
            self.runtime.clear_turn_notification_config()?;
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "回合通知已关闭",
                    "turn notifications disabled",
                ),
            );
            return Ok(());
        }

        if let Some(raw_when) = cleaned.strip_prefix("when ") {
            let Some(when) = crate::parse_turn_notification_when(raw_when) else {
                self.push_log(
                    LogKind::Info,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        "用法: /notify when <always|unfocused>",
                        "usage: /notify when <always|unfocused>",
                    ),
                );
                return Ok(());
            };
            let existing = self.runtime.load_turn_notification_config();
            let updated = crate::apply_notification_when(existing, when);
            self.runtime.save_turn_notification_config(&updated)?;
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "通知触发时机已更新",
                    "notification trigger condition updated",
                ),
            );
            self.push_log(
                LogKind::Info,
                crate::describe_turn_notification(&updated, self.display_language.as_str()),
            );
            return Ok(());
        }

        if cleaned.eq_ignore_ascii_case("bell") || cleaned.eq_ignore_ascii_case("osc9") {
            let config = if cleaned.eq_ignore_ascii_case("bell") {
                crate::runtime::TurnNotificationConfig::Bell {
                    when: crate::runtime::TurnNotificationWhen::Always,
                }
            } else {
                crate::runtime::TurnNotificationConfig::Osc9 {
                    when: crate::runtime::TurnNotificationWhen::Always,
                }
            };
            self.runtime.save_turn_notification_config(&config)?;
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "回合通知方式已更新",
                    "turn notification backend updated",
                ),
            );
            self.push_log(
                LogKind::Info,
                crate::describe_turn_notification(&config, self.display_language.as_str()),
            );
            return Ok(());
        }

        let mut argv = shell_words::split(cleaned)
            .map_err(|err| anyhow!("parse /notify args failed: {err}"))?;
        if argv.is_empty() {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /notify [show|off|bell|osc9|when <always|unfocused>|<command...>]",
                    "usage: /notify [show|off|bell|osc9|when <always|unfocused>|<command...>]",
                ),
            );
            return Ok(());
        }
        let mut when = crate::runtime::TurnNotificationWhen::Always;
        if argv.len() >= 2 && argv[argv.len() - 2].eq_ignore_ascii_case("--when") {
            if let Some(parsed) = crate::parse_turn_notification_when(argv[argv.len() - 1].as_str())
            {
                when = parsed;
                argv.truncate(argv.len() - 2);
            }
        } else if let Some(last) = argv.last() {
            if let Some(parsed) = crate::parse_turn_notification_when(last.as_str()) {
                when = parsed;
                argv.truncate(argv.len() - 1);
            }
        }
        if argv.is_empty() {
            self.push_log(LogKind::Error, "notify command is empty".to_string());
            return Ok(());
        }
        let config = crate::runtime::TurnNotificationConfig::Command { argv, when };
        self.runtime.save_turn_notification_config(&config)?;
        if self.is_zh_language() {
            self.push_log(
                LogKind::Info,
                format!(
                    "回合通知已更新: {}",
                    crate::describe_turn_notification(&config, self.display_language.as_str())
                ),
            );
        } else {
            self.push_log(
                LogKind::Info,
                format!(
                    "turn notification updated: {}",
                    crate::describe_turn_notification(&config, self.display_language.as_str())
                ),
            );
        }
        Ok(())
    }

    async fn handle_mention_slash(&mut self, args: &str) -> Result<()> {
        let query = args.trim();
        if query.is_empty() {
            self.push_log(LogKind::Info, "usage: /mention <query>".to_string());
            return Ok(());
        }
        let results = crate::search_workspace_files(self.runtime.launch_dir.as_path(), query, 30);
        if results.is_empty() {
            self.push_log(LogKind::Info, format!("no files found for: {query}"));
            return Ok(());
        }
        self.push_log(
            LogKind::Info,
            format!("mention results ({})", results.len()),
        );
        for path in results {
            self.push_log(LogKind::Info, path);
        }
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

    async fn handle_apps_slash(&mut self, args: &str) -> Result<()> {
        for line in
            crate::execute_apps_command(&self.runtime, self.display_language.as_str(), args).await?
        {
            self.push_log(LogKind::Info, line);
        }
        self.reload_popup_catalogs().await;
        Ok(())
    }

    fn handle_ps_slash(&mut self, args: &str) {
        let cleaned = args.trim();
        if !cleaned.is_empty() {
            if self.is_zh_language() {
                self.push_log(LogKind::Error, format!("无效的 /ps 参数: {cleaned}"));
                self.push_log(LogKind::Info, "用法: /ps".to_string());
            } else {
                self.push_log(LogKind::Error, format!("invalid /ps args: {cleaned}"));
                self.push_log(LogKind::Info, "usage: /ps".to_string());
            }
            return;
        }
        let sessions = crate::collect_active_monitor_sessions(&self.runtime);
        if sessions.is_empty() {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "当前没有活动中的后台会话",
                    "no active background sessions",
                ),
            );
            return;
        }
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("活动后台会话: {}", sessions.len()));
        } else {
            self.push_log(
                LogKind::Info,
                format!("active background sessions: {}", sessions.len()),
            );
        }
        for entry in sessions {
            self.push_log(
                LogKind::Info,
                crate::format_monitor_session_line(&entry, self.display_language.as_str()),
            );
        }
    }

    fn handle_clean_slash(&mut self, args: &str) {
        let cleaned = args.trim();
        if !cleaned.is_empty() {
            if self.is_zh_language() {
                self.push_log(LogKind::Error, format!("无效的 /clean 参数: {cleaned}"));
                self.push_log(LogKind::Info, "用法: /clean".to_string());
            } else {
                self.push_log(LogKind::Error, format!("invalid /clean args: {cleaned}"));
                self.push_log(LogKind::Info, "usage: /clean".to_string());
            }
            return;
        }
        let sessions = crate::collect_active_monitor_sessions(&self.runtime);
        if sessions.is_empty() {
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "当前没有活动中的后台会话",
                    "no active background sessions",
                ),
            );
            return;
        }

        let mut cancelled = 0usize;
        for entry in sessions {
            let Some(session_id) = entry.get("session_id").and_then(Value::as_str) else {
                continue;
            };
            if self.runtime.state.monitor.cancel(session_id) {
                cancelled = cancelled.saturating_add(1);
            }
        }
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("已发送取消请求: {cancelled}"));
        } else {
            self.push_log(LogKind::Info, format!("cancel requests sent: {cancelled}"));
        }
    }

    async fn handle_fork_slash(&mut self, args: &str) -> Result<()> {
        if self.busy {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "助手仍在运行，请等待完成后再执行 /fork",
                    "assistant is still running, wait for completion before running /fork",
                ),
            );
            return Ok(());
        }
        let title = args.trim();
        let (new_session, copied) = crate::fork_session_with_history(
            &self.runtime,
            self.session_id.as_str(),
            (!title.is_empty()).then_some(title),
        )
        .await?;
        self.switch_to_existing_session(new_session.as_str())
            .await?;
        if self.is_zh_language() {
            self.push_log(
                LogKind::Info,
                format!("已分叉会话: {new_session}（复制 {copied} 条历史）"),
            );
        } else {
            self.push_log(
                LogKind::Info,
                format!("forked session: {new_session} (copied {copied} history entries)"),
            );
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
        if self.busy {
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

    async fn show_debug_config_snapshot(&mut self) -> Result<()> {
        let payload = crate::collect_debug_config_payload(
            &self.runtime,
            &self.global,
            self.session_id.as_str(),
        )
        .await;
        for line in serde_json::to_string_pretty(&payload)?.lines() {
            self.push_log(LogKind::Info, line.to_string());
        }
        Ok(())
    }

    fn handle_statusline_slash(&mut self, args: &str) {
        let cleaned = args.trim();
        if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("show") {
            if self.statusline_items.is_empty() {
                self.push_log(
                    LogKind::Info,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        "状态栏方案: 默认",
                        "status line preset: default",
                    ),
                );
            } else if self.is_zh_language() {
                self.push_log(
                    LogKind::Info,
                    format!("状态栏方案: {}", self.statusline_items.join(", ")),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    format!("status line preset: {}", self.statusline_items.join(", ")),
                );
            }
            self.push_log(
                LogKind::Info,
                if self.is_zh_language() {
                    format!("预览: {}", self.status_line().trim())
                } else {
                    format!("preview: {}", self.status_line().trim())
                },
            );
            self.push_log(
                LogKind::Info,
                if self.is_zh_language() {
                    format!("可选项: {}", STATUSLINE_ITEM_KEYS.join(", "))
                } else {
                    format!("available: {}", STATUSLINE_ITEM_KEYS.join(", "))
                },
            );
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /statusline [show|set <items>|reset]",
                    "usage: /statusline [show|set <items>|reset]",
                ),
            );
            return;
        }

        if cleaned.eq_ignore_ascii_case("reset") {
            self.statusline_items.clear();
            self.persist_statusline_items();
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "状态栏已恢复默认",
                    "status line preset reset to default",
                ),
            );
            return;
        }

        let mut parts = cleaned.splitn(2, char::is_whitespace);
        let action = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default().trim();
        if !action.eq_ignore_ascii_case("set") {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "无效的 /statusline 参数",
                    "invalid /statusline args",
                ),
            );
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /statusline [show|set <items>|reset]",
                    "usage: /statusline [show|set <items>|reset]",
                ),
            );
            return;
        }
        if value.is_empty() {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "请提供至少一个状态栏项",
                    "please provide at least one status line item",
                ),
            );
            return;
        }

        let mut unknown = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut selected = Vec::new();
        for token in value.split(|ch: char| ch == ',' || ch == '|' || ch.is_whitespace()) {
            let trimmed = token.trim();
            if trimmed.is_empty() {
                continue;
            }
            match normalize_statusline_item(trimmed) {
                Some(item) => {
                    if seen.insert(item.clone()) {
                        selected.push(item);
                    }
                }
                None => unknown.push(trimmed.to_string()),
            }
        }
        if !unknown.is_empty() {
            self.push_log(
                LogKind::Error,
                if self.is_zh_language() {
                    format!("未知状态栏项: {}", unknown.join(", "))
                } else {
                    format!("unknown status line items: {}", unknown.join(", "))
                },
            );
            self.push_log(
                LogKind::Info,
                if self.is_zh_language() {
                    format!("可选项: {}", STATUSLINE_ITEM_KEYS.join(", "))
                } else {
                    format!("available: {}", STATUSLINE_ITEM_KEYS.join(", "))
                },
            );
            return;
        }
        if selected.is_empty() {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "未解析到有效状态栏项",
                    "no valid status line items parsed",
                ),
            );
            return;
        }
        self.statusline_items = selected;
        self.persist_statusline_items();
        if self.is_zh_language() {
            self.push_log(
                LogKind::Info,
                format!("状态栏已更新: {}", self.statusline_items.join(", ")),
            );
        } else {
            self.push_log(
                LogKind::Info,
                format!("status line updated: {}", self.statusline_items.join(", ")),
            );
        }
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
        let config = self.runtime.state.config_store.get().await;
        self.approval_mode = self
            .global
            .approval_mode
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
        self.model_name = self
            .runtime
            .resolve_model_name(self.global.model.as_deref())
            .await
            .unwrap_or_else(|| "<none>".to_string());
        let model_entry = config.llm.models.get(&self.model_name);
        self.tool_call_mode = model_entry
            .and_then(|model| model.tool_call_mode.clone())
            .unwrap_or_else(|| "tool_call".to_string());
        self.model_max_context = model_entry
            .and_then(|model| model.max_context)
            .filter(|value| *value > 0);
        self.model_max_rounds = model_entry
            .and_then(|model| model.max_rounds)
            .unwrap_or(crate::CLI_MIN_MAX_ROUNDS)
            .max(crate::CLI_MIN_MAX_ROUNDS);
    }

    fn session_stats_lines(&self) -> Vec<String> {
        let is_zh = self.is_zh_language();
        let mut lines = vec![
            if is_zh {
                "会话".to_string()
            } else {
                "session".to_string()
            },
            if is_zh {
                format!("- 会话 ID: {}", self.session_id)
            } else {
                format!("- id: {}", self.session_id)
            },
            if is_zh {
                format!("- 模型: {}", self.model_name)
            } else {
                format!("- model: {}", self.model_name)
            },
        ];

        if let Some(total) = self.model_max_context {
            let used = self.session_stats.context_used_tokens.max(0) as u64;
            let left =
                crate::context_left_percent(self.session_stats.context_used_tokens, Some(total))
                    .unwrap_or(0);
            if is_zh {
                lines.push(format!("- 上下文: {used}/{total} (剩余 {left}%)"));
            } else {
                lines.push(format!("- context: {used}/{total} ({left}% left)"));
            }
        } else if is_zh {
            lines.push(format!(
                "- 上下文: {}/未知",
                self.session_stats.context_used_tokens.max(0)
            ));
        } else {
            lines.push(format!(
                "- context: {}/unknown",
                self.session_stats.context_used_tokens.max(0)
            ));
        }

        if is_zh {
            lines.push(format!("- 模型调用: {}", self.session_stats.model_calls));
            lines.push(format!("- 工具调用: {}", self.session_stats.tool_calls));
            lines.push(format!("- 工具结果: {}", self.session_stats.tool_results));
            lines.push(format!(
                "- token 占用: input={} output={} total={}",
                self.session_stats.total_input_tokens,
                self.session_stats.total_output_tokens,
                self.session_stats.total_tokens
            ));
        } else {
            lines.push(format!("- model_calls: {}", self.session_stats.model_calls));
            lines.push(format!("- tool_calls: {}", self.session_stats.tool_calls));
            lines.push(format!(
                "- tool_results: {}",
                self.session_stats.tool_results
            ));
            lines.push(format!(
                "- token_usage: input={} output={} total={}",
                self.session_stats.total_input_tokens,
                self.session_stats.total_output_tokens,
                self.session_stats.total_tokens
            ));
        }
        lines
    }

    async fn handle_system_slash(&mut self, args: &str) -> Result<()> {
        let cleaned = args.trim();
        if cleaned.eq_ignore_ascii_case("clear") {
            self.runtime.clear_extra_prompt()?;
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "额外提示词已清除",
                    "extra prompt cleared",
                ),
            );
        } else if let Some(rest) = cleaned.strip_prefix("set ") {
            let prompt = rest.trim();
            if prompt.is_empty() {
                self.push_log(
                    LogKind::Error,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        "额外提示词为空",
                        "extra prompt is empty",
                    ),
                );
                self.push_log(
                    LogKind::Info,
                    crate::locale::tr(
                        self.display_language.as_str(),
                        "用法: /system [set <extra_prompt>|clear]",
                        "usage: /system [set <extra_prompt>|clear]",
                    ),
                );
                return Ok(());
            }
            self.runtime.save_extra_prompt(prompt)?;
            if self.is_zh_language() {
                self.push_log(
                    LogKind::Info,
                    format!("额外提示词已保存（{} 字符）", prompt.chars().count()),
                );
            } else {
                self.push_log(
                    LogKind::Info,
                    format!("extra prompt saved ({} chars)", prompt.chars().count()),
                );
            }
        } else if !cleaned.is_empty() && !cleaned.eq_ignore_ascii_case("show") {
            self.push_log(
                LogKind::Error,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "无效的 /system 参数",
                    "invalid /system args",
                ),
            );
            self.push_log(
                LogKind::Info,
                crate::locale::tr(
                    self.display_language.as_str(),
                    "用法: /system [set <extra_prompt>|clear]",
                    "usage: /system [set <extra_prompt>|clear]",
                ),
            );
            return Ok(());
        }

        let prompt = crate::build_current_system_prompt(&self.runtime, &self.global).await?;
        let extra_prompt = self.runtime.load_extra_prompt();
        self.push_log(
            LogKind::Info,
            crate::locale::tr(self.display_language.as_str(), "系统提示词", "system"),
        );
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("- 会话: {}", self.session_id));
        } else {
            self.push_log(LogKind::Info, format!("- session: {}", self.session_id));
        }
        self.push_log(
            LogKind::Info,
            if self.is_zh_language() {
                format!(
                    "- 额外提示词: {}",
                    extra_prompt
                        .as_ref()
                        .map(|value| format!("已启用（{} 字符）", value.chars().count()))
                        .unwrap_or_else(|| "无".to_string())
                )
            } else {
                format!(
                    "- extra_prompt: {}",
                    extra_prompt
                        .as_ref()
                        .map(|value| format!("enabled ({} chars)", value.chars().count()))
                        .unwrap_or_else(|| "none".to_string())
                )
            },
        );
        let personality_mode = self
            .runtime
            .load_personality_mode()
            .unwrap_or_else(|| "balanced".to_string());
        if self.is_zh_language() {
            self.push_log(LogKind::Info, format!("- 回答风格: {personality_mode}"));
        } else {
            self.push_log(LogKind::Info, format!("- personality: {personality_mode}"));
        }
        self.push_log(
            LogKind::Info,
            crate::locale::tr(
                self.display_language.as_str(),
                "--- 系统提示词开始 ---",
                "--- system prompt ---",
            ),
        );
        for line in prompt.lines() {
            self.push_log(LogKind::Info, line.to_string());
        }
        self.push_log(
            LogKind::Info,
            crate::locale::tr(
                self.display_language.as_str(),
                "--- 系统提示词结束 ---",
                "--- end system prompt ---",
            ),
        );
        Ok(())
    }

    pub(super) async fn switch_to_new_session(&mut self) {
        self.session_id = uuid::Uuid::new_v4().simple().to_string();
        self.runtime.save_session(&self.session_id).ok();
        self.input.clear();
        self.input_cursor = 0;
        self.history_cursor = None;
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
        self.approval_rx = None;
        self.active_approval = None;
        self.approval_queue.clear();
        self.approval_selected_index = 0;
        self.ctrl_c_hint_deadline = None;
        self.focus_area = FocusArea::Input;
        self.transcript_selected = None;
        self.resume_picker = None;
        self.session_stats = crate::SessionStatsSnapshot::default();
        self.reload_session_stats().await;
        self.push_log(
            LogKind::Info,
            format!("switched to session: {}", self.session_id),
        );
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
                format!(
                    "- agent_id 覆盖: {}",
                    self.agent_id_override.as_deref().unwrap_or("-")
                )
            } else {
                format!(
                    "- agent_id_override: {}",
                    self.agent_id_override.as_deref().unwrap_or("-")
                )
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
