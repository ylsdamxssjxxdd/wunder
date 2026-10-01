use super::*;
use crate::core::long_task;
use crate::orchestrator_constants::MAX_USER_INPUT_TEXT_CHARS;
use crate::request_limits::measure_request_text_input_chars;

impl Orchestrator {
    async fn prepare_request(
        &self,
        request: WunderRequest,
    ) -> Result<PreparedRequest, OrchestratorError> {
        let user_id = request.user_id.trim().to_string();
        if user_id.is_empty() {
            return Err(OrchestratorError::invalid_request(i18n::t(
                "error.user_id_required",
            )));
        }
        if let Err(err) = self.inner_visible.sync_user_state(&user_id).await {
            return Err(OrchestratorError::internal(format!(
                "failed to sync inner-visible state: {err}"
            )));
        }
        let agent_id = request
            .agent_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());
        let workspace_id = self.resolve_workspace_id(
            &user_id,
            agent_id.as_deref(),
            request.workspace_container_id,
        );
        if let Err(err) = self.workspace.ensure_user_root(&workspace_id) {
            return Err(OrchestratorError::internal(format!(
                "failed to prepare workspace: {err}"
            )));
        }
        self.workspace.touch_user_session(&workspace_id);
        let question = request.question.trim().to_string();
        let client_message_id = request
            .client_message_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.chars().take(128).collect::<String>());
        let has_attachments = request
            .attachments
            .as_ref()
            .map(|items| {
                items.iter().any(|item| {
                    item.content
                        .as_deref()
                        .map(|value| !value.trim().is_empty())
                        .unwrap_or(false)
                        || item
                            .public_path
                            .as_deref()
                            .map(|value| !value.trim().is_empty())
                            .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if question.is_empty() && !has_attachments {
            return Err(OrchestratorError::invalid_request(i18n::t(
                "error.question_required",
            )));
        }
        validate_request_text_input_size(&question, request.attachments.as_deref())?;
        let session_id = request
            .session_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        // Capture the exclusive durable baseline before accept_thread_turn.
        // The accept transaction appends the first turn/item changes after
        // this cursor; start must replay them instead of starting at the
        // post-accept cursor.
        let resume_from_seq = {
            let storage = self.storage.clone();
            let thread = session_id.clone();
            crate::core::blocking::run_db("thread_log.resume_baseline", move || {
                storage.latest_thread_change_seq_by_session(&thread)
            })
            .await
            .map_err(|err| OrchestratorError::internal(err.to_string()))?
        };
        let tool_names = if request.tool_names.is_empty() {
            None
        } else {
            Some(request.tool_names.clone())
        };
        let language = request
            .language
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(i18n::get_default_language);
        let attachments = request
            .attachments
            .clone()
            .filter(|items| !items.is_empty());
        // Only the internal scheduler can supply an accepted identity. This flag is
        // serde-skipped; external config overrides cannot claim a different turn.
        let reserved = request
            .enforce_runtime_queue
            .then(|| request.config_overrides.as_ref())
            .flatten()
            .and_then(|fields| {
                let id = fields
                    .get("__thread_log_turn_id")?
                    .as_str()?
                    .parse::<Uuid>()
                    .ok()?;
                let round = fields.get("__thread_log_user_round")?.as_i64()?;
                (round > 0).then_some((id, round))
            });
        let (id, round) = if let Some(reserved) = reserved {
            reserved
        } else {
            let storage = self.storage.clone();
            let owner = user_id.clone();
            let thread = session_id.clone();
            let input = json!({"role":"user", "content":question, "attachments":attachments,
                "client_message_id":client_message_id,
                "root_user_round":crate::services::goal::goal_continuation_user_round(request.config_overrides.as_ref())});
            let accepted = crate::core::blocking::run_db("thread_log.accept", move || {
                storage.accept_thread_turn(&owner, &thread, &input)
            })
            .await
            .map_err(|err| OrchestratorError::internal(err.to_string()))?;
            if accepted["created"] == false {
                return Err(OrchestratorError::invalid_request(
                    "client_message_id already accepted".to_string(),
                ));
            }
            // Wake active change feeders (other devices or an already-open
            // watch) so the new user turn surfaces without waiting for their
            // next poll tick.
            let cursor_storage = self.storage.clone();
            let cursor_session = session_id.clone();
            if let Ok(cursor) = crate::core::blocking::run_db(
                "thread_log.accept_cursor",
                move || cursor_storage.latest_thread_change_seq_by_session(&cursor_session),
            )
            .await
            {
                self.change_hub.publish(&session_id, cursor);
            }
            (
                accepted["turn_id"]
                    .as_str()
                    .unwrap_or_default()
                    .parse::<Uuid>()
                    .map_err(|err| OrchestratorError::internal(err.to_string()))?,
                accepted["user_turn_index"].as_i64().unwrap_or(1),
            )
        };
        let thread_turn_id = Some(id);
        let thread_user_round = Some(round);
        Ok(PreparedRequest {
            user_id,
            workspace_id,
            question,
            client_message_id,
            session_id,
            tool_names,
            skip_tool_calls: request.skip_tool_calls,
            model_name: request.model_name.clone(),
            config_overrides: request.config_overrides.clone(),
            agent_prompt: request.agent_prompt.clone(),
            preview_skill: request.preview_skill,
            agent_id,
            stream: request.stream,
            attachments,
            language,
            allow_queue: request.allow_queue,
            is_admin: request.is_admin,
            enforce_runtime_queue: request.enforce_runtime_queue,
            approval_tx: request.approval_tx.clone(),
            thread_turn_id,
            thread_user_round,
            thread_resume_from_seq: resume_from_seq.max(0),
            change_stream: request
                .config_overrides
                .as_ref()
                .and_then(|fields| fields.get("__change_stream").and_then(Value::as_bool))
                .unwrap_or(false),
        })
    }

    pub(crate) fn resolve_workspace_id(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        workspace_container_id: Option<i32>,
    ) -> String {
        if let Some(container_id) =
            workspace_container_id.map(crate::storage::normalize_workspace_container_id)
        {
            return self
                .workspace
                .scoped_user_id_by_container(user_id, container_id);
        }
        let agent_id = agent_id.map(str::trim).filter(|value| !value.is_empty());
        if agent_id.is_none() || is_default_agent_alias(agent_id) {
            if let Ok(record) = crate::user_store::build_default_agent_record_from_storage(
                self.storage.as_ref(),
                user_id,
            ) {
                return self
                    .workspace
                    .scoped_user_id_by_container(user_id, record.sandbox_container_id);
            }
            return self.workspace.scoped_user_id_by_container(
                user_id,
                crate::storage::DEFAULT_SANDBOX_CONTAINER_ID,
            );
        }
        if let Some(agent_id) = agent_id {
            if let Ok(Some(record)) = self.storage.get_user_agent_by_id(agent_id) {
                return self
                    .workspace
                    .scoped_user_id_by_container(user_id, record.sandbox_container_id);
            }
        }
        self.workspace.scoped_user_id(user_id, agent_id)
    }

    pub async fn run(&self, request: WunderRequest) -> Result<WunderResponse> {
        let prepared = self.prepare_request(request).await?;
        let language = prepared.language.clone();
        let emitter = EventEmitter::new(
            prepared.session_id.clone(),
            prepared.user_id.clone(),
            None,
            Some(self.storage.clone()),
            self.monitor.clone(),
            prepared.is_admin,
            0,
            prepared.client_message_id.clone(),
        )
        .with_change_hub(self.change_hub.clone());
        emitter.bind_turn(
            &prepared.thread_turn_id.expect("accepted turn").to_string(),
            prepared.thread_user_round.expect("accepted round"),
        );
        let response = i18n::with_language(language, async {
            self.execute_request(prepared, emitter).await
        })
        .await?;
        Ok(response)
    }

    pub async fn stream(
        &self,
        request: WunderRequest,
    ) -> Result<impl Stream<Item = Result<StreamEvent, std::convert::Infallible>>> {
        let prepared = self.prepare_request(request).await?;
        let language = prepared.language.clone();
        let (queue_tx, queue_rx) = mpsc::channel::<StreamSignal>(STREAM_EVENT_QUEUE_SIZE);
        let (event_tx, event_rx) = mpsc::channel::<StreamEvent>(STREAM_EVENT_QUEUE_SIZE);
        let session_id = prepared.session_id.clone();
        let storage = self.storage.clone();
        let start_event_id =
            match crate::core::blocking::run_db("orchestrator.request.stream_offset", move || {
                storage.get_max_stream_event_id(&session_id)
            })
            .await
            {
                Ok(value) => value,
                Err(err) => {
                    warn!(
                        "failed to load stream event offset for session {}: {err}",
                        prepared.session_id
                    );
                    0
                }
            };
        let mut emitter = EventEmitter::new(
            prepared.session_id.clone(),
            prepared.user_id.clone(),
            Some(queue_tx),
            Some(self.storage.clone()),
            self.monitor.clone(),
            prepared.is_admin,
            start_event_id,
            prepared.client_message_id.clone(),
        )
        .with_change_hub(self.change_hub.clone());
        if prepared.change_stream {
            emitter = emitter.with_change_stream();
        }
        emitter.bind_turn(
            &prepared.thread_turn_id.expect("accepted turn").to_string(),
            prepared.thread_user_round.expect("accepted round"),
        );
        if prepared.change_stream {
            // The change-stream ack anchors the client's feeder cursor. Emitted
            // through the pump so it is ordered before every turn event.
            let cursor_storage = self.storage.clone();
            let cursor_session = prepared.session_id.clone();
            let change_cursor = match crate::core::blocking::run_db(
                "orchestrator.request.change_cursor",
                move || cursor_storage.latest_thread_change_seq_by_session(&cursor_session),
            )
            .await
            {
                Ok(value) => value,
                Err(err) => {
                    warn!(
                        "failed to load change cursor for session {}: {err}",
                        prepared.session_id
                    );
                    0
                }
            };
            emitter
                .emit(
                    "thread_turn_started",
                    json!({
                        "turn_id": prepared.thread_turn_id.expect("accepted turn").to_string(),
                        "user_round": prepared.thread_user_round.expect("accepted round"),
                        "change_cursor": prepared.thread_resume_from_seq,
                        "resume_from_seq": prepared.thread_resume_from_seq,
                        "content": prepared.question,
                        "client_message_id": prepared.client_message_id,
                    }),
                )
                .await;
        }
        let _ = self.thread_runtime.attach_subscriber(&prepared.session_id);
        let runner = {
            let orchestrator = self.clone();
            let emitter = emitter.clone();
            let prepared = prepared.clone();
            let language = language.clone();
            long_task::spawn("orchestrator.request.runner", async move {
                let _ = i18n::with_language(language, async {
                    orchestrator.execute_request(prepared, emitter).await
                })
                .await;
            })
        };
        self.spawn_stream_pump(
            prepared.session_id.clone(),
            queue_rx,
            event_tx,
            emitter,
            runner,
            start_event_id,
        );
        let stream = tokio_stream::wrappers::ReceiverStream::new(event_rx)
            .map(Ok::<_, std::convert::Infallible>);
        Ok(stream)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn build_system_prompt(
        &self,
        config: &Config,
        tool_names: &[String],
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
        user_id: &str,
        agent_id: Option<&str>,
        _is_admin: bool,
        workspace_id: &str,
        config_overrides: Option<&Value>,
        agent_prompt: Option<&str>,
        preview_skill: bool,
    ) -> String {
        let allow_vision = self
            .resolve_llm_config(config, None)
            .ok()
            .map(|(_, llm_config)| llm_config.support_vision.unwrap_or(false))
            .unwrap_or(false);
        let allowed_tool_names = self.filter_tools_for_model_capability(
            self.resolve_allowed_tool_names(config, tool_names, skills, user_tool_bindings),
            allow_vision,
        );
        let tool_call_mode = self.resolve_tool_call_mode(config, None);
        let prompt = self
            .build_system_prompt_with_allowed(
                config,
                config_overrides,
                &allowed_tool_names,
                tool_call_mode,
                skills,
                user_tool_bindings,
                user_id,
                workspace_id,
                agent_id,
                agent_prompt,
                preview_skill,
            )
            .await;
        self.append_memory_prompt(user_id, agent_id, prompt, None, None, None)
            .await
    }
}

fn validate_request_text_input_size(
    question: &str,
    attachments: Option<&[AttachmentPayload]>,
) -> Result<(), OrchestratorError> {
    let actual_chars = measure_request_text_input_chars(question, attachments);
    if actual_chars <= MAX_USER_INPUT_TEXT_CHARS {
        return Ok(());
    }
    let message = i18n::t_with_params(
        "error.user_input_too_long",
        &std::collections::HashMap::from([
            (
                "max_chars".to_string(),
                MAX_USER_INPUT_TEXT_CHARS.to_string(),
            ),
            ("actual_chars".to_string(), actual_chars.to_string()),
        ]),
    );
    Err(OrchestratorError::invalid_request_with_detail(
        message,
        json!({
            "field": "input_text",
            "max_chars": MAX_USER_INPUT_TEXT_CHARS,
            "actual_chars": actual_chars,
        }),
    ))
}

fn is_default_agent_alias(agent_id: Option<&str>) -> bool {
    let Some(cleaned) = agent_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    cleaned.eq_ignore_ascii_case("__default__") || cleaned.eq_ignore_ascii_case("default")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_request_text_input_size_accepts_limit_boundary() {
        let question = "x".repeat(MAX_USER_INPUT_TEXT_CHARS);
        assert!(validate_request_text_input_size(&question, None).is_ok());
    }

    #[test]
    fn validate_request_text_input_size_rejects_oversized_question() {
        let question = "x".repeat(MAX_USER_INPUT_TEXT_CHARS + 1);
        let err = validate_request_text_input_size(&question, None).expect_err("oversized input");
        assert_eq!(err.code(), "INVALID_REQUEST");
        let payload = err.to_payload();
        assert_eq!(payload["detail"]["field"], "input_text");
        assert_eq!(
            payload["detail"]["max_chars"],
            json!(MAX_USER_INPUT_TEXT_CHARS)
        );
        assert_eq!(
            payload["detail"]["actual_chars"],
            json!(MAX_USER_INPUT_TEXT_CHARS + 1)
        );
    }

    #[test]
    fn measure_request_text_input_chars_ignores_image_attachments() {
        let attachments = vec![
            AttachmentPayload {
                name: Some("image.png".to_string()),
                content: Some("data:image/png;base64,AAAA".to_string()),
                content_type: Some("image/png".to_string()),
                public_path: None,
            },
            AttachmentPayload {
                name: Some("note.txt".to_string()),
                content: Some("hello".to_string()),
                content_type: Some("text/plain".to_string()),
                public_path: None,
            },
        ];
        assert_eq!(
            measure_request_text_input_chars("abc", Some(&attachments)),
            8
        );
    }

    #[test]
    fn validate_request_text_input_size_rejects_oversized_text_attachment() {
        let attachments = vec![AttachmentPayload {
            name: Some("huge.txt".to_string()),
            content: Some("x".repeat(MAX_USER_INPUT_TEXT_CHARS + 16)),
            content_type: Some("text/plain".to_string()),
            public_path: None,
        }];
        let err =
            validate_request_text_input_size("short", Some(&attachments)).expect_err("oversized");
        assert_eq!(err.code(), "INVALID_REQUEST");
        let payload = err.to_payload();
        assert_eq!(payload["detail"]["field"], "input_text");
        assert_eq!(
            payload["detail"]["actual_chars"]
                .as_u64()
                .unwrap_or_default() as usize,
            "short".chars().count() + MAX_USER_INPUT_TEXT_CHARS + 16
        );
    }
}
