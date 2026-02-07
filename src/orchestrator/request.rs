use super::*;

impl Orchestrator {
    fn prepare_request(
        &self,
        request: WunderRequest,
    ) -> Result<PreparedRequest, OrchestratorError> {
        let user_id = request.user_id.trim().to_string();
        if user_id.is_empty() {
            return Err(OrchestratorError::invalid_request(i18n::t(
                "error.user_id_required",
            )));
        }
        let agent_id = request
            .agent_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());
        let workspace_id = self.resolve_workspace_id(&user_id, agent_id.as_deref());
        if let Err(err) = self.workspace.ensure_user_root(&workspace_id) {
            return Err(OrchestratorError::internal(format!(
                "failed to prepare workspace: {err}"
            )));
        }
        self.workspace.touch_user_session(&workspace_id);
        let question = request.question.trim().to_string();
        if question.is_empty() {
            return Err(OrchestratorError::invalid_request(i18n::t(
                "error.question_required",
            )));
        }
        let session_id = request
            .session_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
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
        Ok(PreparedRequest {
            user_id,
            workspace_id,
            question,
            session_id,
            tool_names,
            skip_tool_calls: request.skip_tool_calls,
            model_name: request.model_name.clone(),
            config_overrides: request.config_overrides.clone(),
            agent_prompt: request.agent_prompt.clone(),
            agent_id,
            stream: request.stream,
            debug_payload: request.debug_payload,
            attachments,
            language,
            is_admin: request.is_admin,
        })
    }

    pub(crate) fn resolve_workspace_id(&self, user_id: &str, agent_id: Option<&str>) -> String {
        let agent_id = agent_id.map(str::trim).filter(|value| !value.is_empty());
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
        let prepared = self.prepare_request(request)?;
        let language = prepared.language.clone();
        let emitter = EventEmitter::new(
            prepared.session_id.clone(),
            prepared.user_id.clone(),
            None,
            None,
            self.monitor.clone(),
            prepared.is_admin,
            0,
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
        let prepared = self.prepare_request(request)?;
        let language = prepared.language.clone();
        let (queue_tx, queue_rx) = mpsc::channel::<StreamSignal>(STREAM_EVENT_QUEUE_SIZE);
        let (event_tx, event_rx) = mpsc::channel::<StreamEvent>(STREAM_EVENT_QUEUE_SIZE);
        let start_event_id = if prepared.is_admin {
            let session_id = prepared.session_id.clone();
            let storage = self.storage.clone();
            match tokio::task::spawn_blocking(move || storage.get_max_stream_event_id(&session_id))
                .await
            {
                Ok(Ok(value)) => value,
                Ok(Err(err)) => {
                    warn!(
                        "failed to load stream event offset for session {}: {err}",
                        prepared.session_id
                    );
                    0
                }
                Err(err) => {
                    warn!(
                        "failed to load stream event offset for session {}: {err}",
                        prepared.session_id
                    );
                    0
                }
            }
        } else {
            0
        };
        let emitter = EventEmitter::new(
            prepared.session_id.clone(),
            prepared.user_id.clone(),
            Some(queue_tx),
            Some(self.storage.clone()),
            self.monitor.clone(),
            prepared.is_admin,
            start_event_id,
        );
        let runner = {
            let orchestrator = self.clone();
            let emitter = emitter.clone();
            let prepared = prepared.clone();
            let language = language.clone();
            tokio::spawn(async move {
                let result = i18n::with_language(language, async {
                    orchestrator.execute_request(prepared, emitter).await
                })
                .await;
                if let Err(err) = result {
                    warn!("流式请求执行失败: {}", err);
                }
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
            .map(|event| Ok::<_, std::convert::Infallible>(event));
        Ok(stream)
    }

    pub async fn build_system_prompt(
        &self,
        config: &Config,
        tool_names: &[String],
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
        user_id: &str,
        is_admin: bool,
        workspace_id: &str,
        config_overrides: Option<&Value>,
        agent_prompt: Option<&str>,
    ) -> String {
        let allowed_tool_names =
            self.resolve_allowed_tool_names(config, tool_names, skills, user_tool_bindings);
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
                agent_prompt,
            )
            .await;
        self.append_memory_prompt(user_id, prompt, is_admin).await
    }
}
